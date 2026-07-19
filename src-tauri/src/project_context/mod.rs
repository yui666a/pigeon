pub mod cloud_policy;
pub mod context_file;
pub mod digest;
pub mod extractor;
pub mod scanner;

use crate::classifier::TextGenerator;
use crate::db::{
    cloud_rules, directories, project_contexts, project_files, project_notes, projects,
};
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
        let prev = project_contexts::get_context(&conn, project_id)?.and_then(|c| c.inventory_hash);
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
        let conn = db.lock().map_err(AppError::lock_err)?;
        // 正本は project_notes。外部エディタでの md 編集を DB へ取り込んでからキャッシュを再生成する
        crate::project_notes_sync::import_note_from_disk(&conn, project_id)?;
        crate::project_notes_sync::refresh_cached_context(&conn, project_id)?;
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
            let mut take = extracted.text.len().min(budget);
            while take > 0 && !extracted.text.is_char_boundary(take) {
                take -= 1;
            }
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

    // --- 7. project_notes.ai_md 更新（正本はDB。ユーザー欄=user_mdは不可侵） ---
    let auto_body = format!(
        "## 案件コンテキスト（自動生成 {}）\n\n{}",
        Utc::now().format("%Y-%m-%d"),
        digest_body.trim()
    );

    // --- 8. DB更新（ロック内）: ai_md を履歴退避しつつ差し替え、キャッシュとファイルへ反映 ---
    {
        let mut conn = db.lock().map_err(AppError::lock_err)?;
        // 構成変更に伴う再生成でも、直前に外部エディタで user_md 欄が編集されている可能性が
        // ある（自己修復と同様）。ファイルを書き潰す前に取り込み、ユーザー欄を不可侵に保つ
        crate::project_notes_sync::import_note_from_disk(&conn, project_id)?;

        // ディレクトリ再スキャンによる差し替えも「AIがダイジェストを再生成した」ことと
        // 同義なので、手編集と同じ履歴退避付きの差し替えを使う（generate_project_note_ai と同じ方針）
        project_notes::replace_ai_md_with_history(&mut conn, project_id, &auto_body)?;

        let note = project_notes::get_note(&conn, project_id)?;
        let user_md = note.as_ref().map(|n| n.user_md.as_str()).unwrap_or("");
        let composed = crate::project_notes_sync::compose_markdown(
            user_md,
            Some(auto_body.as_str()),
            &project_name,
        );
        let cached =
            context_file::build_cached_context(&composed, context_file::MAX_CACHED_CONTEXT_CHARS);
        let context_hash = extractor::sha256_hex(composed.as_bytes());

        // ファイルへの書き出しは sync_note_to_disk 経由のみ（書き込み経路を1つに保つ）。
        // DBが正本なので、ミラー書き出しの失敗はDB側の更新（直後のupsert_generatedに
        // よるinventory_hash更新）を止めてはいけない（コマンド呼び出し側と同じ方針）
        if let Err(e) = crate::project_notes_sync::sync_note_to_disk(&conn, project_id) {
            eprintln!("[warn] PIGEON-CONTEXT.md への書き出しに失敗: {}", e);
        }

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
        let mut conn = crate::test_helpers::setup_db();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', '春公演')",
            [],
        )
        .unwrap();
        crate::db::directories::link_directory(&mut conn, "p1", dir_path).unwrap();
        Mutex::new(conn)
    }

    #[tokio::test]
    async fn test_rescan_generates_context_file_and_cache() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("香盤表.md"), "第1幕").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        let outcome = rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        assert_eq!(outcome.status, "ok");
        assert!(outcome.regenerated);
        assert_eq!(outcome.file_count, 1);

        // PIGEON-CONTEXT.md が生成されている
        let md = std::fs::read_to_string(dir.path().join("PIGEON-CONTEXT.md")).unwrap();
        assert!(md.contains("〇〇ホール"));
        assert!(md.contains(context_file::AUTO_MARKER));

        // キャッシュも更新されている
        let conn = db.lock().unwrap();
        let ctx = crate::db::project_contexts::get_context(&conn, "p1")
            .unwrap()
            .unwrap();
        assert!(ctx.cached_context.unwrap().contains("〇〇ホール"));
        assert!(ctx.inventory_hash.is_some());
    }

    #[tokio::test]
    async fn test_rescan_unchanged_skips_regeneration() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        let first = rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        assert!(first.regenerated);
        let second = rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        assert!(!second.regenerated, "構成が同じならLLMを呼ばない");
    }

    #[tokio::test]
    async fn test_rescan_preserves_user_section_on_regeneration() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();

        // ユーザーが自由記入欄を編集
        let md_path = dir.path().join("PIGEON-CONTEXT.md");
        let md = std::fs::read_to_string(&md_path).unwrap();
        let edited = md.replace("# 春公演", "# 春公演\n会場担当: 伊藤さん");
        std::fs::write(&md_path, edited).unwrap();

        // ファイル追加 → 再生成
        std::fs::write(dir.path().join("b.txt"), "y").unwrap();
        rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();

        let md = std::fs::read_to_string(&md_path).unwrap();
        assert!(md.contains("会場担当: 伊藤さん"), "ユーザー欄は不可侵");
    }

    #[tokio::test]
    async fn test_rescan_self_repair_imports_external_edit_into_project_notes() {
        // 構成不変の自己修復パス（§4）。外部エディタでの編集が project_notes（DB正本）へ
        // 取り込まれることを検証する（現行はキャッシュのみ更新していた）。
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();

        let md_path = dir.path().join("PIGEON-CONTEXT.md");
        let md = std::fs::read_to_string(&md_path).unwrap();
        let edited = md.replace("# 春公演", "# 春公演\n会場担当: 伊藤さん");
        std::fs::write(&md_path, edited).unwrap();

        // ファイル構成は変えず再スキャン → §4 自己修復のみ
        let outcome = rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        assert!(!outcome.regenerated);

        let conn = db.lock().unwrap();
        let note = crate::db::project_notes::get_note(&conn, "p1")
            .unwrap()
            .unwrap();
        assert!(
            note.user_md.contains("会場担当: 伊藤さん"),
            "外部編集がDB正本(project_notes)へ取り込まれる"
        );
    }

    #[tokio::test]
    async fn test_rescan_regeneration_writes_ai_md_and_archives_previous_to_history() {
        // 構成変更に伴う再生成（§7/§8）。ダイジェストは project_notes.ai_md に書き込まれ、
        // 旧ダイジェストは履歴へ退避される（generate_project_note_ai と同じ差し替え方針）。
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        let first_ai_md = {
            let conn = db.lock().unwrap();
            crate::db::project_notes::get_note(&conn, "p1")
                .unwrap()
                .unwrap()
                .ai_md
                .unwrap()
        };
        assert!(first_ai_md.contains("〇〇ホール"));

        // 構成変更 → 再生成
        std::fs::write(dir.path().join("b.txt"), "y").unwrap();
        let outcome = rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        assert!(outcome.regenerated);

        let conn = db.lock().unwrap();
        let note = crate::db::project_notes::get_note(&conn, "p1")
            .unwrap()
            .unwrap();
        assert!(
            note.ai_md.unwrap().contains("〇〇ホール"),
            "ダイジェストはproject_notes.ai_mdに書き込まれる"
        );
        assert!(!note.ai_edited, "自動生成はai_editedを立てない");

        let history = crate::db::project_notes::list_ai_history(&conn, "p1").unwrap();
        assert_eq!(history.len(), 1, "旧ダイジェストは履歴へ退避される");
        assert_eq!(history[0].ai_md, first_ai_md);
    }

    #[tokio::test]
    async fn test_rescan_regeneration_still_advances_inventory_hash() {
        // §8 で upsert_generated を維持していること（refresh_cached_context には
        // 置き換えていない）を確認する。inventory_hash が更新されないと§4の
        // 「構成不変」判定が壊れ、毎回再生成してしまう。
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        let first_outcome = rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        assert!(
            !first_outcome.regenerated,
            "inventory_hashが更新されていれば構成不変で再生成しない"
        );

        std::fs::write(dir.path().join("b.txt"), "y").unwrap();
        let outcome = rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        assert!(outcome.regenerated);

        let conn = db.lock().unwrap();
        let ctx = crate::db::project_contexts::get_context(&conn, "p1")
            .unwrap()
            .unwrap();
        assert!(ctx.inventory_hash.is_some());
    }

    #[tokio::test]
    async fn test_rescan_missing_directory_sets_status_and_keeps_cache() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());
        rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();

        // ディレクトリ消失（外付けHDD未接続を模擬）
        drop(dir);
        let outcome = rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        assert_eq!(outcome.status, "missing");

        let conn = db.lock().unwrap();
        let ctx = crate::db::project_contexts::get_context(&conn, "p1")
            .unwrap()
            .unwrap();
        assert!(
            ctx.cached_context.is_some(),
            "キャッシュは消さず分類に使い続ける"
        );
    }

    #[tokio::test]
    async fn test_rescan_llm_failure_keeps_previous_context() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());
        rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();

        std::fs::write(dir.path().join("b.txt"), "y").unwrap();
        let outcome = rescan_project(&db, &FailGenerator, "p1", false)
            .await
            .unwrap();
        assert_eq!(outcome.status, "ok");
        assert!(!outcome.regenerated, "LLM失敗時は再生成失敗として扱う");

        let md = std::fs::read_to_string(dir.path().join("PIGEON-CONTEXT.md")).unwrap();
        assert!(
            md.contains("〇〇ホール"),
            "前回のautoセクションを維持（劣化しない）"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_rescan_survives_context_file_write_failure_and_advances_inventory_hash() {
        use std::os::unix::fs::PermissionsExt;

        // §8: sync_note_to_disk はミラー書き出しに過ぎず、失敗してもDB側の正本更新
        // (upsert_generated による inventory_hash 更新)を止めてはいけない。
        // ここでは PIGEON-CONTEXT.md 自身を読み取り専用にし、「ディレクトリの
        // スキャンは読めるが、そのファイルへの書き込みだけ失敗する」状況を
        // 再現する（macOSでは既存ファイルの上書きはファイル自身のパーミッションで
        // 決まり、ディレクトリを読み取り専用にしても上書きは防げないため）。
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let db = setup(dir.path().to_str().unwrap());

        // 1回目: 通常に生成させ、PIGEON-CONTEXT.md を作らせておく
        rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        let context_path = dir.path().join("PIGEON-CONTEXT.md");
        assert!(context_path.exists());

        // PIGEON-CONTEXT.md を読み取り専用にする
        std::fs::set_permissions(&context_path, std::fs::Permissions::from_mode(0o444)).unwrap();

        // ファイル構成を変えて再生成を誘発する
        std::fs::write(dir.path().join("b.txt"), "y").unwrap();

        let result = rescan_project(&db, &MockGenerator, "p1", false).await;

        // 後始末: 権限を戻さないとテスト自体のtempdir掃除が失敗する
        std::fs::set_permissions(&context_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let outcome = result.unwrap_or_else(|e| {
            panic!(
                "PIGEON-CONTEXT.md書き出し失敗はDB側の正本更新を止めてはならない: {}",
                e
            )
        });
        assert!(outcome.regenerated, "DB側の再生成自体は成功する");

        // inventory_hash が新しいスキャン結果まで進んでいること。
        // 進んでいなければ、次回同一構成での再スキャンが再び「構成変更」と誤判定し、
        // LLMを呼び直して履歴を消費し続けてしまう（バグの本体）。
        let conn = db.lock().unwrap();
        let ctx = crate::db::project_contexts::get_context(&conn, "p1")
            .unwrap()
            .unwrap();
        let new_hash = ctx.inventory_hash.clone();
        drop(conn);

        // 3回目: 構成を変えずに再スキャン → inventory_hashが進んでいれば自己修復のみ
        let third = rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        assert!(
            !third.regenerated,
            "inventory_hashが更新されていれば同一構成の再スキャンで再生成しない"
        );

        let conn = db.lock().unwrap();
        let ctx_after = crate::db::project_contexts::get_context(&conn, "p1")
            .unwrap()
            .unwrap();
        assert_eq!(
            ctx_after.inventory_hash, new_hash,
            "構成不変時はinventory_hashも変わらない"
        );
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
        let spy = SpyGenerator {
            saw_secret: saw_secret.clone(),
        };
        rescan_project(&db, &spy, "p1", true).await.unwrap();

        assert!(
            !saw_secret.load(Ordering::SeqCst),
            "cloud=true では未許可ファイルは名前も内容もLLMに渡さない（スペック§5不変条件1）"
        );
    }

    #[tokio::test]
    async fn test_rescan_does_not_panic_when_budget_splits_multibyte_char() {
        // MAX_EXTRACT_BYTES_PER_PROJECT (100KB = 102400バイト) は3で割り切れないため、
        // 「あ」(3バイト)の繰り返しファイルだけを予算超過まで並べると、最後のファイルで
        // 残り予算がちょうど文字の途中（3の倍数でないバイト位置）に落ちる。
        // 102400 % 9999 = 2410, 2410 % 3 = 1 → 文字境界ではない。
        let dir = tempfile::tempdir().unwrap();
        // 「あ」(3バイト) を 3333 回 = 9999バイト/ファイル。10KB上限(MAX_EXTRACT_BYTES_PER_FILE)未満。
        let chunk: String = "あ".repeat(3333);
        for i in 1..=11 {
            std::fs::write(dir.path().join(format!("{:03}.txt", i)), &chunk).unwrap();
        }
        let db = setup(dir.path().to_str().unwrap());

        // 修正前は budget がマルチバイト文字境界に落ちて String::truncate が panic する。
        let outcome = rescan_project(&db, &MockGenerator, "p1", false)
            .await
            .unwrap();
        assert_eq!(outcome.status, "ok");
    }
}
