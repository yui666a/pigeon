pub mod cloud_policy;
pub mod context_file;
pub mod digest;
pub mod extractor;
pub mod scanner;

use crate::classifier::TextGenerator;
use crate::db::{cloud_rules, directories, project_contexts, project_files, projects};
use crate::error::AppError;
use crate::models::directory::ProjectFileEntry;
use chrono::Utc;
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Serialize)]
pub struct RescanOutcome {
    pub status: String,    // 'ok' | 'missing' | 'inaccessible' | 'error' | 'unlinked'
    pub regenerated: bool, // auto セクションを再生成したか
    pub file_count: usize,
}

/// 案件ディレクトリの再スキャン一式。
/// ロックは「DBスナップショット取得」「結果書き込み」の2回だけ短く取り、
/// ファイルI/OとLLM呼び出しはロック外で行う（classify_commands と同じ様式）。
pub async fn rescan_project(
    db: &Mutex<Connection>,
    generator: &dyn TextGenerator,
    project_id: &str,
    cloud: bool,
) -> Result<RescanOutcome, AppError> {
    // --- 1. スナップショット取得（ロック内） ---
    let (dir, project_name, prev_inventory_hash, rules) = {
        let conn = db.lock().map_err(AppError::lock_err)?;
        let dir = match directories::get_directory_by_project(&conn, project_id)? {
            Some(d) => d,
            None => {
                return Ok(RescanOutcome {
                    status: "unlinked".to_string(),
                    regenerated: false,
                    file_count: 0,
                })
            }
        };
        let project = projects::get_project(&conn, project_id)?;
        let prev = project_contexts::get_context(&conn, project_id)?
            .and_then(|c| c.inventory_hash);
        let rules = cloud_rules::list_rules(&conn, &dir.id)?;
        (dir, project.name, prev, rules)
    };

    let root = Path::new(&dir.path);

    // --- 2. スキャン（ロック外） ---
    let scan = match scanner::scan_directory(root) {
        Ok(s) => s,
        Err(AppError::DirectoryScan(msg)) => {
            // "missing" / "inaccessible" / "error" を文言から判別（scanner が付与）
            let status = if msg.contains("[missing]") {
                "missing"
            } else if msg.contains("[inaccessible]") {
                "inaccessible"
            } else {
                "error"
            };
            let conn = db.lock().map_err(AppError::lock_err)?;
            directories::set_status(&conn, &dir.id, status)?;
            // キャッシュは消さない（スペック§8: 分類に使い続ける）
            return Ok(RescanOutcome {
                status: status.to_string(),
                regenerated: false,
                file_count: 0,
            });
        }
        Err(e) => return Err(e),
    };

    // --- 3. インベントリ書き込み（ロック内） ---
    {
        let mut conn = db.lock().map_err(AppError::lock_err)?;
        project_files::replace_inventory(&mut conn, &dir.id, &scan.files)?;
        directories::set_status(&conn, &dir.id, "ok")?;
        directories::touch_scanned(&conn, &dir.id)?;
    }

    // --- 4. 構成不変なら自己修復のみ（md外部編集の取り込み） ---
    if prev_inventory_hash.as_deref() == Some(scan.inventory_hash.as_str()) {
        if let Some(md) = context_file::read_context_file(root)? {
            let cached =
                context_file::build_cached_context(&md, context_file::MAX_CACHED_CONTEXT_CHARS);
            let hash = extractor::sha256_hex(md.as_bytes());
            let conn = db.lock().map_err(AppError::lock_err)?;
            project_contexts::update_cache_only(&conn, project_id, &cached, &hash)?;
        }
        return Ok(RescanOutcome {
            status: "ok".to_string(),
            regenerated: false,
            file_count: scan.files.len(),
        });
    }

    // --- 5. ダイジェスト入力の組み立て（送信可否 + 100KB 上限を適用） ---
    let visible_files: Vec<ProjectFileEntry> = if cloud {
        // スペック§5不変条件1: 未許可ファイルは名前も含めない
        scan.files
            .iter()
            .filter(|f| cloud_policy::is_cloud_allowed(&rules, &f.relative_path))
            .cloned()
            .collect()
    } else {
        scan.files.clone()
    };

    let mut texts: Vec<(String, String)> = Vec::new();
    let mut budget = extractor::MAX_EXTRACT_BYTES_PER_PROJECT;
    for f in &visible_files {
        if f.content_kind != "text" || f.extract_status != "ok" || budget == 0 {
            continue;
        }
        if let Ok(extracted) = extractor::extract_text(&root.join(&f.relative_path)) {
            let take = extracted.text.len().min(budget);
            let mut text = extracted.text;
            text.truncate(take);
            budget -= take;
            texts.push((f.relative_path.clone(), text));
        }
    }

    // 入力が空（cloud で許可ゼロ等）なら生成をスキップして前回を維持（スペック§3）
    if visible_files.is_empty() {
        return Ok(RescanOutcome {
            status: "ok".to_string(),
            regenerated: false,
            file_count: scan.files.len(),
        });
    }

    // --- 6. LLM でダイジェスト生成（ロック外）。失敗時は前回を維持 ---
    let input = digest::build_digest_input(&project_name, &visible_files, &texts);
    let digest_body = match digest::generate_digest(generator, &input).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[warn] digest generation failed for {}: {}", project_id, e);
            return Ok(RescanOutcome {
                status: "ok".to_string(),
                regenerated: false,
                file_count: scan.files.len(),
            });
        }
    };

    // --- 7. PIGEON-CONTEXT.md 更新（ユーザー欄不可侵） ---
    let existing = context_file::read_context_file(root)?;
    let auto_body = format!(
        "## 案件コンテキスト（自動生成 {}）\n\n{}",
        Utc::now().format("%Y-%m-%d"),
        digest_body.trim()
    );
    let new_md =
        context_file::upsert_auto_section(existing.as_deref(), &project_name, &auto_body);
    context_file::write_context_file(root, &new_md)?;

    // --- 8. キャッシュ更新（ロック内） ---
    let cached =
        context_file::build_cached_context(&new_md, context_file::MAX_CACHED_CONTEXT_CHARS);
    let context_hash = extractor::sha256_hex(new_md.as_bytes());
    {
        let conn = db.lock().map_err(AppError::lock_err)?;
        project_contexts::upsert_generated(
            &conn,
            project_id,
            &cached,
            &context_hash,
            &scan.inventory_hash,
        )?;
    }

    Ok(RescanOutcome {
        status: "ok".to_string(),
        regenerated: true,
        file_count: scan.files.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::TextGenerator;
    use crate::error::AppError;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct MockGenerator;

    #[async_trait]
    impl TextGenerator for MockGenerator {
        async fn generate_text(&self, _s: &str, _u: &str) -> Result<String, AppError> {
            Ok("- 会場: 〇〇ホール".to_string())
        }
    }

    struct FailGenerator;

    #[async_trait]
    impl TextGenerator for FailGenerator {
        async fn generate_text(&self, _s: &str, _u: &str) -> Result<String, AppError> {
            Err(AppError::OllamaConnection("down".to_string()))
        }
    }

    fn setup(dir_path: &str) -> Mutex<rusqlite::Connection> {
        let conn = crate::test_helpers::setup_db();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', '春公演')",
            [],
        )
        .unwrap();
        crate::db::directories::link_directory(&conn, "p1", dir_path).unwrap();
        Mutex::new(conn)
    }

    #[tokio::test]
    async fn test_rescan_generates_context_file_and_cache() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("香盤表.md"), "第1幕").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        let outcome = rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();
        assert_eq!(outcome.status, "ok");
        assert!(outcome.regenerated);
        assert_eq!(outcome.file_count, 1);

        // PIGEON-CONTEXT.md が生成されている
        let md = std::fs::read_to_string(dir.path().join("PIGEON-CONTEXT.md")).unwrap();
        assert!(md.contains("〇〇ホール"));
        assert!(md.contains(context_file::AUTO_MARKER));

        // キャッシュも更新されている
        let conn = db.lock().unwrap();
        let ctx = crate::db::project_contexts::get_context(&conn, "p1").unwrap().unwrap();
        assert!(ctx.cached_context.unwrap().contains("〇〇ホール"));
        assert!(ctx.inventory_hash.is_some());
    }

    #[tokio::test]
    async fn test_rescan_unchanged_skips_regeneration() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        let first = rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();
        assert!(first.regenerated);
        let second = rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();
        assert!(!second.regenerated, "構成が同じならLLMを呼ばない");
    }

    #[tokio::test]
    async fn test_rescan_preserves_user_section_on_regeneration() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();

        // ユーザーが自由記入欄を編集
        let md_path = dir.path().join("PIGEON-CONTEXT.md");
        let md = std::fs::read_to_string(&md_path).unwrap();
        let edited = md.replace("# 春公演", "# 春公演\n会場担当: 伊藤さん");
        std::fs::write(&md_path, edited).unwrap();

        // ファイル追加 → 再生成
        std::fs::write(dir.path().join("b.txt"), "y").unwrap();
        rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();

        let md = std::fs::read_to_string(&md_path).unwrap();
        assert!(md.contains("会場担当: 伊藤さん"), "ユーザー欄は不可侵");
    }

    #[tokio::test]
    async fn test_rescan_missing_directory_sets_status_and_keeps_cache() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());
        rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();

        // ディレクトリ消失（外付けHDD未接続を模擬）
        drop(dir);
        let outcome = rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();
        assert_eq!(outcome.status, "missing");

        let conn = db.lock().unwrap();
        let ctx = crate::db::project_contexts::get_context(&conn, "p1").unwrap().unwrap();
        assert!(ctx.cached_context.is_some(), "キャッシュは消さず分類に使い続ける");
    }

    #[tokio::test]
    async fn test_rescan_llm_failure_keeps_previous_context() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());
        rescan_project(&db, &MockGenerator, "p1", false).await.unwrap();

        std::fs::write(dir.path().join("b.txt"), "y").unwrap();
        let outcome = rescan_project(&db, &FailGenerator, "p1", false).await.unwrap();
        assert_eq!(outcome.status, "ok");
        assert!(!outcome.regenerated, "LLM失敗時は再生成失敗として扱う");

        let md = std::fs::read_to_string(dir.path().join("PIGEON-CONTEXT.md")).unwrap();
        assert!(md.contains("〇〇ホール"), "前回のautoセクションを維持（劣化しない）");
    }

    #[tokio::test]
    async fn test_rescan_cloud_mode_excludes_unallowed_files_from_input() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct SpyGenerator {
            saw_secret: std::sync::Arc<AtomicBool>,
        }
        #[async_trait]
        impl TextGenerator for SpyGenerator {
            async fn generate_text(&self, _s: &str, user: &str) -> Result<String, AppError> {
                if user.contains("秘密") || user.contains("secret.txt") {
                    self.saw_secret.store(true, Ordering::SeqCst);
                }
                Ok("- 要約".to_string())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("public.txt"), "公開資料").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "秘密").unwrap();
        let db = setup(dir.path().to_str().unwrap());
        {
            let conn = db.lock().unwrap();
            let d = crate::db::directories::get_directory_by_project(&conn, "p1")
                .unwrap()
                .unwrap();
            crate::db::cloud_rules::set_rule(&conn, &d.id, "file", "public.txt", true).unwrap();
        }

        let saw_secret = std::sync::Arc::new(AtomicBool::new(false));
        let spy = SpyGenerator { saw_secret: saw_secret.clone() };
        rescan_project(&db, &spy, "p1", true).await.unwrap();

        assert!(
            !saw_secret.load(Ordering::SeqCst),
            "cloud=true では未許可ファイルは名前も内容もLLMに渡さない（スペック§5不変条件1）"
        );
    }
}
