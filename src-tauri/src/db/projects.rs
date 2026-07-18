use crate::db::assignments;
use crate::error::AppError;
use crate::models::classifier::ProjectSummary;
use crate::models::project::{CreateProjectRequest, Project, UpdateProjectRequest};
use rusqlite::{params, Connection};
use uuid::Uuid;

/// projects テーブルの1行を Project へ変換する共通マッパー。
/// カラム順は SELECT 句の
/// `id, account_id, name, description, color, is_archived, parent_id, created_at, updated_at`
/// に一致させること。
fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        account_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        color: row.get(4)?,
        is_archived: row.get(5)?,
        parent_id: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub fn insert_project_with_id(
    conn: &Connection,
    id: &str,
    account_id: &str,
    name: &str,
    description: Option<&str>,
    color: Option<&str>,
    parent_id: Option<&str>,
) -> Result<Project, AppError> {
    if let Some(pid) = parent_id {
        let parent = get_project(conn, pid)?;
        if parent.account_id != account_id {
            return Err(AppError::Validation(
                "親案件は同じアカウントに属している必要があります".into(),
            ));
        }
        if parent.is_archived {
            return Err(AppError::Validation(
                "アーカイブ済みの案件の下には作成できません".into(),
            ));
        }
    }
    conn.execute(
        "INSERT INTO projects (id, account_id, name, description, color, parent_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, account_id, name, description, color, parent_id],
    )?;
    get_project(conn, id)
}

pub fn insert_project(conn: &Connection, req: &CreateProjectRequest) -> Result<Project, AppError> {
    let id = Uuid::new_v4().to_string();
    insert_project_with_id(
        conn,
        &id,
        &req.account_id,
        &req.name,
        req.description.as_deref(),
        req.color.as_deref(),
        req.parent_id.as_deref(),
    )
}

pub fn get_project(conn: &Connection, id: &str) -> Result<Project, AppError> {
    conn.query_row(
        "SELECT id, account_id, name, description, color, is_archived, parent_id, created_at, updated_at
         FROM projects WHERE id = ?1",
        params![id],
        row_to_project,
    )
    .map_err(|_| AppError::ProjectNotFound(id.to_string()))
}

pub fn list_projects(conn: &Connection, account_id: &str) -> Result<Vec<Project>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, name, description, color, is_archived, parent_id, created_at, updated_at
         FROM projects
         WHERE account_id = ?1 AND is_archived = FALSE
         ORDER BY created_at",
    )?;
    let projects = stmt
        .query_map(params![account_id], row_to_project)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(projects)
}

/// 自分+全子孫の ID を深い順（depth DESC）で返す。
/// 深い順は delete_project の葉先行削除の前提（FK CASCADE の再帰上限を踏まない）。
/// 存在しない id は空 Vec（呼び出し側で ProjectNotFound にする）。
pub fn subtree_ids(conn: &Connection, id: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE subtree(id, depth) AS (
             SELECT id, 0 FROM projects WHERE id = ?1
             UNION ALL
             SELECT p.id, s.depth + 1 FROM projects p JOIN subtree s ON p.parent_id = s.id
         )
         SELECT id FROM subtree ORDER BY depth DESC, id",
    )?;
    let ids = stmt
        .query_map(params![id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

/// ルート→自ノード順の祖先パス（自分を含む）。
pub fn ancestor_path(conn: &Connection, id: &str) -> Result<Vec<Project>, AppError> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE anc(id, account_id, name, description, color, is_archived, parent_id, created_at, updated_at, depth) AS (
             SELECT id, account_id, name, description, color, is_archived, parent_id, created_at, updated_at, 0
             FROM projects WHERE id = ?1
             UNION ALL
             SELECT p.id, p.account_id, p.name, p.description, p.color, p.is_archived, p.parent_id, p.created_at, p.updated_at, a.depth + 1
             FROM projects p JOIN anc a ON p.id = a.parent_id
         )
         SELECT id, account_id, name, description, color, is_archived, parent_id, created_at, updated_at
         FROM anc ORDER BY depth DESC",
    )?;
    let projects = stmt
        .query_map(params![id], row_to_project)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(projects)
}

/// 「ツアー > 会場 > 音響」形式のパス文字列（パンくず・LLM・訂正ログスナップショット用）。
pub fn project_path_string(conn: &Connection, id: &str) -> Result<String, AppError> {
    let path = ancestor_path(conn, id)?;
    if path.is_empty() {
        return Err(AppError::ProjectNotFound(id.to_string()));
    }
    Ok(path
        .iter()
        .map(|p| p.name.as_str())
        .collect::<Vec<_>>()
        .join(" > "))
}

/// 親の付け替え。DB トリガーが最終防衛線だが、ユーザー向けの日本語エラーは
/// ここで返す（存在・同一アカウント・循環・アーカイブ済み親）。
pub fn set_parent(conn: &Connection, id: &str, new_parent: Option<&str>) -> Result<(), AppError> {
    let project = get_project(conn, id)?;
    if let Some(parent_id) = new_parent {
        let parent = get_project(conn, parent_id)?;
        if parent.account_id != project.account_id {
            return Err(AppError::Validation(
                "親案件は同じアカウントに属している必要があります".into(),
            ));
        }
        if parent.is_archived {
            return Err(AppError::Validation(
                "アーカイブ済みの案件の下には移動できません".into(),
            ));
        }
        if subtree_ids(conn, id)?.contains(&parent_id.to_string()) {
            return Err(AppError::Validation(
                "自分自身または配下の案件を親にはできません".into(),
            ));
        }
    }
    conn.execute(
        "UPDATE projects SET parent_id = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
        params![new_parent, id],
    )?;
    Ok(())
}

pub fn update_project(
    conn: &Connection,
    id: &str,
    req: &UpdateProjectRequest,
) -> Result<Project, AppError> {
    // Fetch existing project to apply partial updates
    let current = get_project(conn, id)?;

    let new_name = req.name.as_deref().unwrap_or(&current.name);
    let new_description = req.description.as_ref().or(current.description.as_ref());
    let new_color = req.color.as_ref().or(current.color.as_ref());

    conn.execute(
        "UPDATE projects
         SET name = ?1, description = ?2, color = ?3, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?4",
        params![new_name, new_description, new_color, id],
    )?;
    get_project(conn, id)
}

/// サブツリー一括アーカイブ（親だけ消えて子が宙に浮く状態を作らない）。
pub fn archive_project(conn: &Connection, id: &str) -> Result<(), AppError> {
    let ids = subtree_ids(conn, id)?;
    if ids.is_empty() {
        return Err(AppError::ProjectNotFound(id.to_string()));
    }
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "UPDATE projects SET is_archived = TRUE, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        )?;
        for pid in &ids {
            stmt.execute(params![pid])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Build ProjectSummary list for LLM classification context.
///
/// `for_cloud=true` のときは allow_cloud_context が付いた案件のみ context を注入する
/// （スペック§5不変条件2）。Ollama（ローカル）は false で全案件注入。
pub fn build_project_summaries(
    conn: &Connection,
    account_id: &str,
    for_cloud: bool,
) -> Result<Vec<ProjectSummary>, AppError> {
    let projs = list_projects(conn, account_id)?;
    let mut summaries = Vec::with_capacity(projs.len());
    for p in projs {
        let recent_subjects = assignments::get_recent_subjects(conn, &p.id, 10)?;
        let top_senders = assignments::get_top_senders(conn, &p.id, 5)?;
        let context = crate::db::project_contexts::get_context(conn, &p.id)?
            .filter(|c| !for_cloud || c.allow_cloud_context)
            .and_then(|c| c.cached_context)
            .map(|c| c.chars().take(800).collect::<String>());
        summaries.push(ProjectSummary {
            id: p.id,
            name: p.name,
            description: p.description,
            recent_subjects,
            top_senders,
            context,
        });
    }
    Ok(summaries)
}

/// サブツリーを葉先行で明示削除する。FK CASCADE（防御層）に深い再帰をさせない。
pub fn delete_project(conn: &Connection, id: &str) -> Result<(), AppError> {
    let ids = subtree_ids(conn, id)?;
    if ids.is_empty() {
        return Err(AppError::ProjectNotFound(id.to_string()));
    }
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare("DELETE FROM projects WHERE id = ?1")?;
        for pid in &ids {
            stmt.execute(params![pid])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// 加算的継承の有効コンテキスト。ルート→自ノード順で、各エントリは定義元
/// ノードに紐づく（クラウド送信可否も定義元ノードのルールで評価される——
/// ルール同士の合成はしない。設計書 §7）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct EffectiveContextEntry {
    pub project_id: String,
    pub project_name: String,
    pub is_self: bool,
    pub directory_path: Option<String>,
    pub context: Option<String>,
}

pub fn build_effective_context(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<EffectiveContextEntry>, AppError> {
    let path = ancestor_path(conn, project_id)?;
    if path.is_empty() {
        return Err(AppError::ProjectNotFound(project_id.to_string()));
    }
    let mut entries = Vec::with_capacity(path.len());
    for node in &path {
        let context = crate::db::project_contexts::get_context(conn, &node.id)?
            .and_then(|c| c.cached_context);
        let directory_path =
            crate::db::directories::get_directory_by_project(conn, &node.id)?.map(|d| d.path);
        entries.push(EffectiveContextEntry {
            project_id: node.id.clone(),
            project_name: node.name.clone(),
            is_self: node.id == project_id,
            directory_path,
            context,
        });
    }
    Ok(entries)
}

/// サブツリー配下の所属メール数（削除確認ダイアログ用）。
pub fn count_subtree_mails(conn: &Connection, id: &str) -> Result<u32, AppError> {
    let count: u32 = conn.query_row(
        "WITH RECURSIVE subtree(id) AS (
             SELECT id FROM projects WHERE id = ?1
             UNION ALL
             SELECT p.id FROM projects p JOIN subtree s ON p.parent_id = s.id
         )
         SELECT COUNT(*) FROM mail_project_assignments WHERE project_id IN (SELECT id FROM subtree)",
        params![id],
        |r| r.get(0),
    )?;
    Ok(count)
}

/// source を target へ統合する。
/// 検証: self / 異アカウント / target が source の子孫 / どちらかがアーカイブ済み → 拒否。
/// 処理順（1トランザクション）: (1) source 直属メールを target へ reassign（パス
/// スナップショット付き訂正ログ）→ (2) source の子を target の子へ reparent →
/// (3) source を削除（子は移動済みなので単発 DELETE で CASCADE 再帰なし）。
pub fn merge_projects(
    conn: &Connection,
    source_id: &str,
    target_id: &str,
) -> Result<u32, AppError> {
    if source_id == target_id {
        return Err(AppError::Validation(
            "同じ案件同士はマージできません".into(),
        ));
    }
    let source = get_project(conn, source_id)?;
    let target = get_project(conn, target_id)?;
    if source.account_id != target.account_id {
        return Err(AppError::Validation(
            "異なるアカウントの案件はマージできません".into(),
        ));
    }
    if source.is_archived || target.is_archived {
        return Err(AppError::Validation(
            "アーカイブ済みの案件はマージできません".into(),
        ));
    }
    if subtree_ids(conn, source_id)?.contains(&target_id.to_string()) {
        return Err(AppError::Validation(
            "統合先が統合元の配下にあります".into(),
        ));
    }

    let tx = conn.unchecked_transaction()?;

    // Get all mail IDs currently assigned to the source project
    let mail_ids: Vec<String> = {
        let mut stmt =
            tx.prepare("SELECT mail_id FROM mail_project_assignments WHERE project_id = ?1")?;
        let ids = stmt
            .query_map(params![source_id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids
    };

    let count = mail_ids.len() as u32;

    // Reassign each mail to the target project and log the correction
    for mail_id in &mail_ids {
        assignments::reassign_with_correction(&tx, mail_id, source_id, target_id)?;
    }

    // Reparent source's children to target before deleting source
    tx.execute(
        "UPDATE projects SET parent_id = ?1 WHERE parent_id = ?2",
        params![target_id, source_id],
    )?;

    // Delete the source project (no cascade issues since assignments and children were moved)
    tx.execute("DELETE FROM projects WHERE id = ?1", params![source_id])?;

    tx.commit()?;

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    // 共有ヘルパの setup_db は FK 有効化・マイグレーション適用済みで、
    // テストアカウント acc1 を作成済みの接続を返す
    use crate::test_helpers::setup_db;

    fn sample_create_req(account_id: &str) -> CreateProjectRequest {
        CreateProjectRequest {
            account_id: account_id.to_string(),
            name: "Test Project".into(),
            description: Some("A test project".into()),
            color: Some("#FF5733".into()),
            parent_id: None,
        }
    }

    fn insert_child(conn: &Connection, id: &str, name: &str, parent: Option<&str>) -> Project {
        insert_project_with_id(conn, id, "acc1", name, None, None, parent).unwrap()
    }

    #[test]
    fn test_insert_and_get_project() {
        let conn = setup_db();

        let req = sample_create_req("acc1");
        let project = insert_project(&conn, &req).unwrap();

        assert!(!project.id.is_empty());
        assert_eq!(project.account_id, "acc1");
        assert_eq!(project.name, "Test Project");
        assert_eq!(project.description, Some("A test project".into()));
        assert_eq!(project.color, Some("#FF5733".into()));
        assert!(!project.is_archived);
        assert!(!project.created_at.is_empty());
        assert!(!project.updated_at.is_empty());

        let fetched = get_project(&conn, &project.id).unwrap();
        assert_eq!(fetched.id, project.id);
        assert_eq!(fetched.name, project.name);
    }

    #[test]
    fn test_list_projects_excludes_archived() {
        let conn = setup_db();

        let req = sample_create_req("acc1");
        let p1 = insert_project(&conn, &req).unwrap();
        let p2 = insert_project(
            &conn,
            &CreateProjectRequest {
                account_id: "acc1".into(),
                name: "Project 2".into(),
                description: None,
                color: None,
                parent_id: None,
            },
        )
        .unwrap();

        // Archive p2
        archive_project(&conn, &p2.id).unwrap();

        let projects = list_projects(&conn, "acc1").unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, p1.id);
    }

    #[test]
    fn test_update_project() {
        let conn = setup_db();

        let project = insert_project(&conn, &sample_create_req("acc1")).unwrap();

        let update_req = UpdateProjectRequest {
            name: Some("Updated Name".into()),
            description: None,
            color: Some("#00FF00".into()),
        };
        let updated = update_project(&conn, &project.id, &update_req).unwrap();

        assert_eq!(updated.name, "Updated Name");
        assert_eq!(updated.description, Some("A test project".into())); // unchanged
        assert_eq!(updated.color, Some("#00FF00".into()));
    }

    #[test]
    fn test_delete_project() {
        let conn = setup_db();

        let project = insert_project(&conn, &sample_create_req("acc1")).unwrap();
        delete_project(&conn, &project.id).unwrap();

        let result = get_project(&conn, &project.id);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::ProjectNotFound(_)));
    }

    #[test]
    fn test_get_nonexistent_project() {
        let conn = setup_db();

        let result = get_project(&conn, "nonexistent-id");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::ProjectNotFound(_)));
    }

    #[test]
    fn test_build_project_summaries_includes_cached_context() {
        let conn = setup_db();
        let p = insert_project(&conn, &sample_create_req("acc1")).unwrap();
        crate::db::project_contexts::upsert_generated(&conn, &p.id, "会場: 〇〇ホール", "h", "i")
            .unwrap();

        let summaries = build_project_summaries(&conn, "acc1", false).unwrap();
        assert_eq!(summaries[0].context.as_deref(), Some("会場: 〇〇ホール"));
    }

    #[test]
    fn test_build_project_summaries_cloud_excludes_unallowed_context() {
        let conn = setup_db();
        let p = insert_project(&conn, &sample_create_req("acc1")).unwrap();
        crate::db::project_contexts::upsert_generated(&conn, &p.id, "秘密のコンテキスト", "h", "i")
            .unwrap();
        // allow_cloud_context はデフォルト false のまま

        let summaries = build_project_summaries(&conn, "acc1", true).unwrap();
        assert!(
            summaries[0].context.is_none(),
            "スペック§5不変条件2: 未許可案件のコンテキストはクラウドに注入しない"
        );

        // 許可すると注入される
        crate::db::project_contexts::set_allow_cloud_context(&conn, &p.id, true).unwrap();
        let summaries = build_project_summaries(&conn, "acc1", true).unwrap();
        assert!(summaries[0].context.is_some());
    }

    #[test]
    fn test_build_project_summaries_includes_top_senders() {
        let conn = setup_db();
        let p = insert_project(&conn, &sample_create_req("acc1")).unwrap();

        let m1 = crate::test_helpers::make_mail("m1", "<m1@ex>", "Mail 1", "2026-04-13T10:00:00");
        crate::db::mails::insert_mail(&conn, &m1).unwrap();
        assignments::assign_mail(&conn, "m1", &p.id, "ai", Some(0.9)).unwrap();

        let summaries = build_project_summaries(&conn, "acc1", false).unwrap();
        assert!(
            !summaries[0].top_senders.is_empty(),
            "割り当て済みメールの送信者がtop_sendersに含まれるはず"
        );
        assert_eq!(summaries[0].top_senders[0], "sender@example.com");
    }

    #[test]
    fn test_merge_projects_moves_mails() {
        let conn = setup_db();

        let source = insert_project(
            &conn,
            &CreateProjectRequest {
                account_id: "acc1".into(),
                name: "Source".into(),
                description: None,
                color: None,
                parent_id: None,
            },
        )
        .unwrap();
        let target = insert_project(
            &conn,
            &CreateProjectRequest {
                account_id: "acc1".into(),
                name: "Target".into(),
                description: None,
                color: None,
                parent_id: None,
            },
        )
        .unwrap();

        // Insert mails and assign to source
        let m1 = crate::test_helpers::make_mail("m1", "<m1@ex>", "Mail 1", "2026-04-13T10:00:00");
        let m2 = crate::test_helpers::make_mail("m2", "<m2@ex>", "Mail 2", "2026-04-13T11:00:00");
        crate::db::mails::insert_mail(&conn, &m1).unwrap();
        crate::db::mails::insert_mail(&conn, &m2).unwrap();
        assignments::assign_mail(&conn, "m1", &source.id, "ai", Some(0.9)).unwrap();
        assignments::assign_mail(&conn, "m2", &source.id, "ai", Some(0.8)).unwrap();

        let moved = merge_projects(&conn, &source.id, &target.id).unwrap();
        assert_eq!(moved, 2);

        // Mails should now be in target
        let target_mails = assignments::get_mails_by_project(&conn, &target.id).unwrap();
        assert_eq!(target_mails.len(), 2);

        // Source project should be deleted
        assert!(matches!(
            get_project(&conn, &source.id),
            Err(AppError::ProjectNotFound(_))
        ));

        // Corrections should be logged
        let corrections = assignments::get_recent_corrections(&conn, "acc1", 20).unwrap();
        assert_eq!(corrections.len(), 2);
    }

    #[test]
    fn test_merge_projects_source_empty() {
        let conn = setup_db();

        let source = insert_project(
            &conn,
            &CreateProjectRequest {
                account_id: "acc1".into(),
                name: "Empty Source".into(),
                description: None,
                color: None,
                parent_id: None,
            },
        )
        .unwrap();
        let target = insert_project(
            &conn,
            &CreateProjectRequest {
                account_id: "acc1".into(),
                name: "Target".into(),
                description: None,
                color: None,
                parent_id: None,
            },
        )
        .unwrap();

        let moved = merge_projects(&conn, &source.id, &target.id).unwrap();
        assert_eq!(moved, 0);

        // Source should still be deleted
        assert!(matches!(
            get_project(&conn, &source.id),
            Err(AppError::ProjectNotFound(_))
        ));
        // Target should still exist
        assert!(get_project(&conn, &target.id).is_ok());
    }

    #[test]
    fn test_merge_projects_source_not_found() {
        let conn = setup_db();

        let target = insert_project(
            &conn,
            &CreateProjectRequest {
                account_id: "acc1".into(),
                name: "Target".into(),
                description: None,
                color: None,
                parent_id: None,
            },
        )
        .unwrap();

        let result = merge_projects(&conn, "nonexistent", &target.id);
        assert!(matches!(result, Err(AppError::ProjectNotFound(_))));
    }

    #[test]
    fn test_merge_projects_target_not_found() {
        let conn = setup_db();

        let source = insert_project(
            &conn,
            &CreateProjectRequest {
                account_id: "acc1".into(),
                name: "Source".into(),
                description: None,
                color: None,
                parent_id: None,
            },
        )
        .unwrap();

        let result = merge_projects(&conn, &source.id, "nonexistent");
        assert!(matches!(result, Err(AppError::ProjectNotFound(_))));
    }

    #[test]
    fn test_merge_preserves_existing_target_mails() {
        let conn = setup_db();

        let source = insert_project(
            &conn,
            &CreateProjectRequest {
                account_id: "acc1".into(),
                name: "Source".into(),
                description: None,
                color: None,
                parent_id: None,
            },
        )
        .unwrap();
        let target = insert_project(
            &conn,
            &CreateProjectRequest {
                account_id: "acc1".into(),
                name: "Target".into(),
                description: None,
                color: None,
                parent_id: None,
            },
        )
        .unwrap();

        // Existing mail in target
        let m1 = crate::test_helpers::make_mail("m1", "<m1@ex>", "Existing", "2026-04-13T09:00:00");
        crate::db::mails::insert_mail(&conn, &m1).unwrap();
        assignments::assign_mail(&conn, "m1", &target.id, "user", Some(1.0)).unwrap();

        // Mail in source
        let m2 = crate::test_helpers::make_mail("m2", "<m2@ex>", "Moved", "2026-04-13T10:00:00");
        crate::db::mails::insert_mail(&conn, &m2).unwrap();
        assignments::assign_mail(&conn, "m2", &source.id, "ai", Some(0.9)).unwrap();

        let moved = merge_projects(&conn, &source.id, &target.id).unwrap();
        assert_eq!(moved, 1);

        let target_mails = assignments::get_mails_by_project(&conn, &target.id).unwrap();
        assert_eq!(target_mails.len(), 2);
    }

    #[test]
    fn test_merge_rejects_self_and_descendant_and_archived() {
        let conn = setup_db();
        insert_child(&conn, "root", "ツアー", None);
        insert_child(&conn, "mid", "埼玉", Some("root"));
        insert_child(&conn, "arch", "旧公演", None);
        archive_project(&conn, "arch").unwrap();

        assert!(merge_projects(&conn, "root", "root").is_err(), "self");
        assert!(
            merge_projects(&conn, "root", "mid").is_err(),
            "descendant target"
        );
        assert!(
            merge_projects(&conn, "mid", "arch").is_err(),
            "archived target"
        );
        assert!(
            merge_projects(&conn, "arch", "mid").is_err(),
            "archived source"
        );
    }

    #[test]
    fn test_merge_reparents_children_to_target() {
        let conn = setup_db();
        insert_child(&conn, "src", "旧ツアー", None);
        insert_child(&conn, "child", "埼玉", Some("src"));
        insert_child(&conn, "dst", "新ツアー", None);

        merge_projects(&conn, "src", "dst").unwrap();

        let parent: Option<String> = conn
            .query_row(
                "SELECT parent_id FROM projects WHERE id = 'child'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(parent.as_deref(), Some("dst"));
        assert!(get_project(&conn, "src").is_err());
    }

    #[test]
    fn test_merge_records_path_snapshots() {
        let conn = setup_db();
        insert_child(&conn, "src", "照明", None);
        insert_child(&conn, "dst", "音響", None);
        let m = crate::test_helpers::make_mail("m1", "<m1@ex>", "S", "2026-07-18T10:00:00");
        crate::db::mails::insert_mail(&conn, &m).unwrap();
        assignments::assign_mail(&conn, "m1", "src", "ai", Some(0.9)).unwrap();

        merge_projects(&conn, "src", "dst").unwrap();

        let c = &assignments::get_recent_corrections(&conn, "acc1", 1).unwrap()[0];
        assert_eq!(c.from_path.as_deref(), Some("照明"));
        assert_eq!(c.to_path, "音響");
    }

    #[test]
    fn test_subtree_ids_returns_deepest_first() {
        let conn = setup_db();
        insert_child(&conn, "root", "ツアー", None);
        insert_child(&conn, "mid", "埼玉", Some("root"));
        insert_child(&conn, "leaf", "音響", Some("mid"));
        insert_child(&conn, "other", "別件", None);

        let ids = subtree_ids(&conn, "root").unwrap();
        assert_eq!(ids.len(), 3);
        assert_eq!(ids.last().unwrap(), "root", "自分が最後（最浅）");
        assert!(ids.iter().position(|i| i == "leaf") < ids.iter().position(|i| i == "mid"));
        assert!(subtree_ids(&conn, "nonexistent").unwrap().is_empty());
    }

    #[test]
    fn test_ancestor_path_and_path_string() {
        let conn = setup_db();
        insert_child(&conn, "root", "ツアー", None);
        insert_child(&conn, "mid", "埼玉", Some("root"));
        insert_child(&conn, "leaf", "音響", Some("mid"));

        let path = ancestor_path(&conn, "leaf").unwrap();
        let names: Vec<&str> = path.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["ツアー", "埼玉", "音響"]);
        assert_eq!(
            project_path_string(&conn, "leaf").unwrap(),
            "ツアー > 埼玉 > 音響"
        );
    }

    #[test]
    fn test_set_parent_validations() {
        let conn = setup_db();
        insert_child(&conn, "root", "ツアー", None);
        insert_child(&conn, "mid", "埼玉", Some("root"));
        insert_child(&conn, "arch", "旧公演", None);
        archive_project(&conn, "arch").unwrap();

        // 正常系: ルート化と付け替え
        set_parent(&conn, "mid", None).unwrap();
        set_parent(&conn, "mid", Some("root")).unwrap();
        // 自分自身・子孫は拒否（アプリ層エラー）
        assert!(set_parent(&conn, "root", Some("root")).is_err());
        assert!(set_parent(&conn, "root", Some("mid")).is_err());
        // アーカイブ済み親は拒否
        assert!(set_parent(&conn, "mid", Some("arch")).is_err());
    }

    #[test]
    fn test_create_project_under_archived_parent_is_rejected() {
        let conn = setup_db();
        insert_child(&conn, "arch", "旧公演", None);
        archive_project(&conn, "arch").unwrap();
        let result = insert_project_with_id(&conn, "c1", "acc1", "子", None, None, Some("arch"));
        assert!(result.is_err());
    }

    #[test]
    fn test_insert_project_with_id_rejects_cross_account_parent() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('acc2', 'Other', 'other@example.com', 'imap.example.com', 'smtp.example.com', 'plain', 'other')",
            [],
        )
        .unwrap();
        // acc2 の親案件
        insert_project_with_id(
            &conn,
            "parent2",
            "acc2",
            "他アカウント案件",
            None,
            None,
            None,
        )
        .unwrap();

        let result = insert_project_with_id(&conn, "c1", "acc1", "子", None, None, Some("parent2"));
        assert!(
            matches!(result, Err(AppError::Validation(_))),
            "異アカウント親は Validation エラーで拒否されるはず: {result:?}"
        );
    }

    #[test]
    fn test_set_parent_rejects_cross_account_parent() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('acc2', 'Other', 'other@example.com', 'imap.example.com', 'smtp.example.com', 'plain', 'other')",
            [],
        )
        .unwrap();
        insert_child(&conn, "c1", "acc1案件", None);
        insert_project_with_id(
            &conn,
            "parent2",
            "acc2",
            "他アカウント案件",
            None,
            None,
            None,
        )
        .unwrap();

        let result = set_parent(&conn, "c1", Some("parent2"));
        assert!(
            matches!(result, Err(AppError::Validation(_))),
            "異アカウント親は Validation エラーで拒否されるはず: {result:?}"
        );
    }

    #[test]
    fn test_delete_project_removes_subtree_and_unassigns_mails() {
        let conn = setup_db();
        insert_child(&conn, "root", "ツアー", None);
        insert_child(&conn, "mid", "埼玉", Some("root"));
        let m = crate::test_helpers::make_mail("m1", "<m1@ex>", "S", "2026-07-18T10:00:00");
        crate::db::mails::insert_mail(&conn, &m).unwrap();
        assignments::assign_mail(&conn, "m1", "mid", "user", None).unwrap();

        delete_project(&conn, "root").unwrap();

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
        let assigned: i64 = conn
            .query_row("SELECT COUNT(*) FROM mail_project_assignments", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(assigned, 0, "メールは未分類に戻る");
    }

    #[test]
    fn test_delete_deep_subtree_leaf_first_avoids_cascade_limit() {
        // FK CASCADE の再帰上限（既定1000）を踏まないことの回帰テスト
        let conn = setup_db();
        conn.execute(
            "WITH RECURSIVE nums(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM nums WHERE n < 1100)
             INSERT INTO projects (id, account_id, name, parent_id)
             SELECT 'd' || n, 'acc1', 'N' || n, CASE WHEN n = 1 THEN NULL ELSE 'd' || (n - 1) END
             FROM nums",
            [],
        )
        .unwrap();
        delete_project(&conn, "d1").unwrap();
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_archive_project_archives_subtree() {
        let conn = setup_db();
        insert_child(&conn, "root", "ツアー", None);
        insert_child(&conn, "mid", "埼玉", Some("root"));
        archive_project(&conn, "root").unwrap();
        let active: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE is_archived = FALSE",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(active, 0, "親だけ消えて子が宙に浮く状態を作らない");
    }

    #[test]
    fn test_build_effective_context_accumulates_ancestors() {
        let conn = setup_db();
        insert_child(&conn, "root", "ツアー", None);
        insert_child(&conn, "leaf", "音響", Some("root"));
        crate::db::project_contexts::upsert_generated(
            &conn,
            "root",
            "ツアー全体の共有情報",
            "h",
            "i",
        )
        .unwrap();
        crate::db::project_contexts::upsert_generated(&conn, "leaf", "音響の機材リスト", "h", "i")
            .unwrap();

        let entries = build_effective_context(&conn, "leaf").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].project_name, "ツアー");
        assert!(!entries[0].is_self);
        assert_eq!(entries[0].context.as_deref(), Some("ツアー全体の共有情報"));
        assert_eq!(entries[1].project_name, "音響");
        assert!(entries[1].is_self);
    }

    #[test]
    fn test_build_effective_context_includes_directory_path_and_missing_context() {
        let mut conn = setup_db();
        insert_child(&conn, "root", "ツアー", None);
        insert_child(&conn, "leaf", "音響", Some("root"));
        crate::db::directories::link_directory(&mut conn, "root", "/tmp/tour").unwrap();

        let entries = build_effective_context(&conn, "leaf").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].directory_path.as_deref(), Some("/tmp/tour"));
        assert!(entries[0].context.is_none());
        assert!(entries[1].directory_path.is_none());
    }

    #[test]
    fn test_build_effective_context_nonexistent_project() {
        let conn = setup_db();
        let result = build_effective_context(&conn, "nonexistent");
        assert!(matches!(result, Err(AppError::ProjectNotFound(_))));
    }

    #[test]
    fn test_count_subtree_mails() {
        let conn = setup_db();
        insert_child(&conn, "root", "ツアー", None);
        insert_child(&conn, "mid", "埼玉", Some("root"));
        for (mid, pid) in [("m1", "root"), ("m2", "mid")] {
            let m = crate::test_helpers::make_mail(
                mid,
                &format!("<{mid}@ex>"),
                "S",
                "2026-07-18T10:00:00",
            );
            crate::db::mails::insert_mail(&conn, &m).unwrap();
            assignments::assign_mail(&conn, mid, pid, "user", None).unwrap();
        }
        assert_eq!(count_subtree_mails(&conn, "root").unwrap(), 2);
        assert_eq!(count_subtree_mails(&conn, "mid").unwrap(), 1);
    }
}
