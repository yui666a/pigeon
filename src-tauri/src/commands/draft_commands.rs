//! 下書き保存 command。
//! 設計: docs/superpowers/specs/2026-07-12-draft-save-design.md

use serde::Deserialize;
use tauri::State;

use crate::db::drafts;
use crate::error::AppError;
use crate::models::draft::Draft;
use crate::state::DbState;
use rusqlite::Connection;

#[derive(Debug, Clone, Deserialize)]
pub struct SaveDraftRequest {
    /// 既存下書きの更新なら Some、新規作成なら None（IDはRust側で採番する）
    #[serde(default)]
    pub id: Option<String>,
    pub account_id: String,
    #[serde(default)]
    pub to_addr: String,
    #[serde(default)]
    pub cc_addr: String,
    #[serde(default)]
    pub bcc_addr: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body_text: String,
    #[serde(default)]
    pub in_reply_to: Option<String>,
}

/// リクエストから Draft を構築する。id は既存下書きの更新ならそれを、
/// 無ければ新規採番する。created_at は呼び出し時点の値を積むが、
/// update_draft のSQLはこの列を更新しないため既存行の created_at は保たれる。
pub(crate) fn build_draft(req: &SaveDraftRequest) -> Draft {
    let now = chrono::Utc::now().to_rfc3339();
    let id = req
        .id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    Draft {
        id,
        account_id: req.account_id.clone(),
        to_addr: req.to_addr.clone(),
        cc_addr: req.cc_addr.clone(),
        bcc_addr: req.bcc_addr.clone(),
        subject: req.subject.clone(),
        body_text: req.body_text.clone(),
        in_reply_to: req.in_reply_to.clone(),
        created_at: now.clone(),
        updated_at: now,
    }
}

/// 下書きを保存する（upsert）。id が既存下書きのものならUPDATE、
/// 無ければ新規採番してINSERTする。Tauri commandから分離してテスト容易にする。
pub(crate) fn upsert_draft(conn: &Connection, req: &SaveDraftRequest) -> Result<Draft, AppError> {
    let already_exists = match &req.id {
        Some(id) => drafts::exists(conn, id)?,
        None => false,
    };

    let draft = build_draft(req);
    if already_exists {
        drafts::update_draft(conn, &draft)?;
    } else {
        drafts::insert_draft(conn, &draft)?;
    }
    Ok(draft)
}

#[tauri::command]
pub async fn save_draft(
    state: State<'_, DbState>,
    req: SaveDraftRequest,
) -> Result<Draft, AppError> {
    state.with_conn(|conn| upsert_draft(conn, &req))
}

#[tauri::command]
pub async fn get_drafts(
    state: State<'_, DbState>,
    account_id: String,
) -> Result<Vec<Draft>, AppError> {
    state.with_conn(|conn| drafts::get_drafts_by_account(conn, &account_id))
}

#[tauri::command]
pub async fn delete_draft(state: State<'_, DbState>, id: String) -> Result<(), AppError> {
    state.with_conn(|conn| drafts::delete_draft(conn, &id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request() -> SaveDraftRequest {
        SaveDraftRequest {
            id: None,
            account_id: "acc1".into(),
            to_addr: "tanaka@example.com".into(),
            cc_addr: "".into(),
            bcc_addr: "".into(),
            subject: "件名".into(),
            body_text: "本文".into(),
            in_reply_to: None,
        }
    }

    #[test]
    fn test_build_draft_new_generates_id() {
        let req = make_request();
        let draft = build_draft(&req);
        assert!(!draft.id.is_empty());
        assert_eq!(draft.account_id, "acc1");
        assert_eq!(draft.created_at, draft.updated_at);
    }

    #[test]
    fn test_build_draft_uses_request_id_when_present() {
        let req = SaveDraftRequest {
            id: Some("existing-id".into()),
            ..make_request()
        };
        let draft = build_draft(&req);
        assert_eq!(draft.id, "existing-id");
    }

    #[test]
    fn test_upsert_draft_update_preserves_created_at_in_db() {
        // build_draft は毎回 created_at に現在時刻を積むが、update_draft のSQLは
        // created_at 列を更新しないため、DBに残る既存行の created_at は保たれる。
        // この契約はDB越しでしか検証できないため upsert_draft を通して確認する。
        let conn = crate::test_helpers::setup_db();
        let first = upsert_draft(&conn, &make_request()).unwrap();
        let original_created_at = first.created_at.clone();

        let update_req = SaveDraftRequest {
            id: Some(first.id.clone()),
            subject: "新".into(),
            ..make_request()
        };
        upsert_draft(&conn, &update_req).unwrap();

        let stored = drafts::get_drafts_by_account(&conn, "acc1")
            .unwrap()
            .into_iter()
            .find(|d| d.id == first.id)
            .unwrap();
        assert_eq!(stored.created_at, original_created_at);
        assert_eq!(stored.subject, "新");
    }

    #[test]
    fn test_upsert_draft_inserts_new_and_get_drafts_returns_it() {
        let conn = crate::test_helpers::setup_db();

        let req = make_request();
        let saved = upsert_draft(&conn, &req).unwrap();
        assert!(!saved.id.is_empty());

        let list = drafts::get_drafts_by_account(&conn, "acc1").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].subject, "件名");
    }

    #[test]
    fn test_upsert_draft_upserts_existing_id() {
        let conn = crate::test_helpers::setup_db();

        let first = upsert_draft(&conn, &make_request()).unwrap();

        let update_req = SaveDraftRequest {
            id: Some(first.id.clone()),
            subject: "更新後".into(),
            ..make_request()
        };
        upsert_draft(&conn, &update_req).unwrap();

        let list = drafts::get_drafts_by_account(&conn, "acc1").unwrap();
        assert_eq!(list.len(), 1, "同じIDでの保存は行を増やさない");
        assert_eq!(list[0].subject, "更新後");
    }

    #[test]
    fn test_upsert_draft_unknown_id_inserts_instead_of_erroring() {
        // フロントが一度も保存していないIDを渡すことはない想定だが、
        // 未知のIDが来ても新規挿入として扱いエラーにしない（防御的）
        let conn = crate::test_helpers::setup_db();
        let req = SaveDraftRequest {
            id: Some("unknown-id".into()),
            ..make_request()
        };
        let saved = upsert_draft(&conn, &req).unwrap();
        assert_eq!(saved.id, "unknown-id");
        assert_eq!(
            drafts::get_drafts_by_account(&conn, "acc1").unwrap().len(),
            1
        );
    }

    #[test]
    fn test_delete_draft_removes_it() {
        let conn = crate::test_helpers::setup_db();

        let saved = upsert_draft(&conn, &make_request()).unwrap();
        drafts::delete_draft(&conn, &saved.id).unwrap();

        assert!(drafts::get_drafts_by_account(&conn, "acc1")
            .unwrap()
            .is_empty());
    }
}
