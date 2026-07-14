//! IMAP IDLE (RFC 2177) による新着メールのリアルタイム検知。
//!
//! 監視タスクは新着の「検知」だけを行い、Tauri イベント `new-mail-detected` を
//! emit する。実際の取り込みはフロントエンドが受けて既存の sync_account 経路で
//! 行う（多重実行ガード・UI更新も既存フローに乗る）。
//! 設計: docs/archive/specs/2026-07-12-imap-idle-design.md

use std::future::Future;
use std::time::Duration;

use async_imap::extensions::idle::IdleResponse;
use async_imap::imap_proto::{MailboxDatum, Response};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::mail_commands::resolve_imap_credentials;
use crate::db::accounts;
use crate::error::AppError;
use crate::mail_sync::imap_client;
use crate::state::{DbState, IdleWatchers, SecureStoreState};

/// 再接続バックオフの初期値
pub(crate) const INITIAL_BACKOFF: Duration = Duration::from_secs(30);
/// 再接続バックオフの上限（10分）
pub(crate) const MAX_BACKOFF: Duration = Duration::from_secs(600);
/// IDLE の張り直し間隔。RFC 2177 はサーバーの無通信切断（30分）対策として
/// 29 分以内の再発行を推奨しており、余裕を持って 25 分にする
pub(crate) const IDLE_REFRESH_INTERVAL: Duration = Duration::from_secs(25 * 60);
/// IDLE 非対応サーバー向けポーリングフォールバックの間隔（15分）
pub(crate) const POLL_FALLBACK_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// new-mail-detected イベントの payload
#[derive(Clone, serde::Serialize)]
pub struct NewMailEvent {
    pub account_id: String,
}

/// 次の再接続バックオフ（2倍、MAX_BACKOFF でキャップ）
pub(crate) fn next_backoff(current: Duration) -> Duration {
    (current * 2).min(MAX_BACKOFF)
}

/// IDLE 中の untagged response が「新着メール」を意味するか。
/// `* n EXISTS`（メッセージ数の増加）と `* n RECENT` のみを新着とみなす。
/// EXPUNGE（削除）や FETCH（フラグ変更）では同期を起動しない
pub(crate) fn is_new_mail_response(resp: &Response<'_>) -> bool {
    matches!(
        resp,
        Response::MailboxData(MailboxDatum::Exists(_))
            | Response::MailboxData(MailboxDatum::Recent(_))
    )
}

/// 1回の監視セッションの終わり方。外側の再接続ループ（watch_loop）が解釈する
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionOutcome {
    /// 接続確立後に切断・エラー。バックオフをリセットして再接続する
    Disconnected,
    /// 接続・認証に失敗。exponential backoff で再接続する
    ConnectFailed,
    /// 監視を終了する（再認証が必要・アカウント削除等）
    Stop,
}

/// 再接続ループ。セッション実行（run_session）と待機（wait）を注入可能にし、
/// 正常→切断→backoff→復帰の状態遷移を単体テストできるようにしている
pub(crate) async fn watch_loop<S, SFut, W, WFut>(mut run_session: S, mut wait: W)
where
    S: FnMut() -> SFut,
    SFut: Future<Output = SessionOutcome>,
    W: FnMut(Duration) -> WFut,
    WFut: Future<Output = ()>,
{
    let mut backoff = INITIAL_BACKOFF;
    loop {
        match run_session().await {
            SessionOutcome::Stop => return,
            SessionOutcome::Disconnected => {
                // 一度は正常に監視できていたので、バックオフをリセットして再接続
                backoff = INITIAL_BACKOFF;
                wait(backoff).await;
            }
            SessionOutcome::ConnectFailed => {
                wait(backoff).await;
                backoff = next_backoff(backoff);
            }
        }
    }
}

/// アカウントの INBOX を IDLE 監視する（切断時は自動再接続）。
/// タスクの開始・停止は start_watching / stop_watching 経由で行う
pub async fn watch_inbox(app: AppHandle, account_id: String) {
    watch_loop(
        || run_watch_session(&app, &account_id),
        |d| tokio::time::sleep(d),
    )
    .await;
    eprintln!("[info] idle: watch stopped for account {}", account_id);
}

/// アカウントの INBOX 監視タスクを開始し、IdleWatchers に登録する。
/// 既に監視中なら中断して置き換える（OAuth 再認証後の再開もこの置き換えで実現）
pub fn start_watching(app: &AppHandle, account_id: &str) {
    let task = watch_inbox(app.clone(), account_id.to_string());
    let handle = tauri::async_runtime::spawn(task);
    app.state::<IdleWatchers>().insert(account_id, handle);
}

/// アカウントの INBOX 監視タスクを停止する（アカウント削除時）
pub fn stop_watching(app: &AppHandle, account_id: &str) {
    app.state::<IdleWatchers>().stop(account_id);
}

/// 新着検知イベントを emit する（検知のみ。取り込みはフロントエンドが起動する）
fn emit_new_mail(app: &AppHandle, account_id: &str) {
    let event = NewMailEvent {
        account_id: account_id.to_string(),
    };
    if let Err(e) = app.emit("new-mail-detected", event) {
        eprintln!(
            "[warn] idle: failed to emit new-mail-detected for {}: {}",
            account_id, e
        );
    }
}

/// 1回の監視セッション: 接続 → CAPABILITY 確認 → SELECT INBOX → IDLE ループ。
/// IDLE 非対応サーバーでは 15 分間隔のポーリング（検知イベントのみ）に
/// フォールバックする。終了理由を SessionOutcome で返す
async fn run_watch_session(app: &AppHandle, account_id: &str) -> SessionOutcome {
    let account = {
        let db = app.state::<DbState>();
        let conn = match db.0.lock() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[warn] idle: DB lock poisoned ({}), stopping watch", e);
                return SessionOutcome::Stop;
            }
        };
        match accounts::get_account(&conn, account_id) {
            Ok(a) => a,
            Err(e) => {
                // アカウントが取得できない（削除済み等）なら監視を続ける意味がない
                eprintln!(
                    "[info] idle: account {} unavailable ({}), stopping watch",
                    account_id, e
                );
                return SessionOutcome::Stop;
            }
        }
    };

    let secure_store = app.state::<SecureStoreState>();
    let (auth_type, username, credential) =
        match resolve_imap_credentials(&account, &secure_store.0).await {
            Ok(creds) => creds,
            Err(AppError::ReauthRequired(_)) => {
                // 再認証が必要。OAuth 完了時に start_watching で再スタートされる
                eprintln!(
                    "[info] idle: reauth required for {}, stopping watch until re-auth",
                    account_id
                );
                return SessionOutcome::Stop;
            }
            Err(e) => {
                eprintln!(
                    "[warn] idle: credential resolution failed for {}: {}",
                    account_id, e
                );
                return SessionOutcome::ConnectFailed;
            }
        };

    let mut session = match imap_client::connect(
        &account.imap_host,
        account.imap_port,
        &auth_type,
        &username,
        &credential,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[warn] idle: connect failed for {}: {}", account_id, e);
            return SessionOutcome::ConnectFailed;
        }
    };

    let supports_idle = match session.capabilities().await {
        Ok(caps) => caps.has_str("IDLE"),
        Err(e) => {
            eprintln!("[warn] idle: CAPABILITY failed for {}: {}", account_id, e);
            return SessionOutcome::Disconnected;
        }
    };

    if !supports_idle {
        // IDLE 非対応: 接続は保持せず、15分間隔の検知イベントのみにフォールバック。
        // 取り込みは IDLE と同じく sync_account 側が行う
        eprintln!(
            "[info] idle: server for {} lacks IDLE capability, polling every {}s",
            account_id,
            POLL_FALLBACK_INTERVAL.as_secs()
        );
        if let Err(e) = session.logout().await {
            eprintln!("[warn] idle: logout failed for {}: {}", account_id, e);
        }
        loop {
            tokio::time::sleep(POLL_FALLBACK_INTERVAL).await;
            emit_new_mail(app, account_id);
        }
    }

    if let Err(e) = session.select("INBOX").await {
        eprintln!("[warn] idle: SELECT INBOX failed for {}: {}", account_id, e);
        return SessionOutcome::Disconnected;
    }

    // IDLE ループ: IDLE_REFRESH_INTERVAL ごとに張り直し、新着応答でイベントを emit
    loop {
        let mut handle = session.idle();
        if let Err(e) = handle.init().await {
            eprintln!("[warn] idle: IDLE init failed for {}: {}", account_id, e);
            return SessionOutcome::Disconnected;
        }
        let outcome = {
            let (wait_fut, _interrupt) = handle.wait_with_timeout(IDLE_REFRESH_INTERVAL);
            wait_fut.await
        };
        // DONE で IDLE を終える（タイムアウト時の張り直しにも必要）
        session = match handle.done().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[warn] idle: IDLE done failed for {}: {}", account_id, e);
                return SessionOutcome::Disconnected;
            }
        };
        match outcome {
            Ok(IdleResponse::NewData(data)) => {
                if is_new_mail_response(data.parsed()) {
                    emit_new_mail(app, account_id);
                }
            }
            // タイムアウト（25分無応答）はそのまま IDLE を張り直す
            Ok(IdleResponse::Timeout) | Ok(IdleResponse::ManualInterrupt) => {}
            Err(e) => {
                eprintln!("[warn] idle: IDLE wait failed for {}: {}", account_id, e);
                return SessionOutcome::Disconnected;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn test_next_backoff_doubles() {
        assert_eq!(
            next_backoff(Duration::from_secs(30)),
            Duration::from_secs(60)
        );
        assert_eq!(
            next_backoff(Duration::from_secs(120)),
            Duration::from_secs(240)
        );
    }

    #[test]
    fn test_next_backoff_caps_at_ten_minutes() {
        // 480s * 2 = 960s だが 600s でキャップ
        assert_eq!(
            next_backoff(Duration::from_secs(480)),
            Duration::from_secs(600)
        );
        // 上限到達後は上限のまま
        assert_eq!(
            next_backoff(Duration::from_secs(600)),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn test_is_new_mail_response_detects_exists_and_recent() {
        assert!(is_new_mail_response(&Response::MailboxData(
            MailboxDatum::Exists(23)
        )));
        assert!(is_new_mail_response(&Response::MailboxData(
            MailboxDatum::Recent(1)
        )));
    }

    #[test]
    fn test_is_new_mail_response_ignores_other_responses() {
        // 削除通知やフラグ変更（FETCH）では同期を起動しない
        assert!(!is_new_mail_response(&Response::Expunge(3)));
        assert!(!is_new_mail_response(&Response::Fetch(3, vec![])));
        assert!(!is_new_mail_response(&Response::MailboxData(
            MailboxDatum::Flags(vec![])
        )));
    }

    /// スクリプトどおりの SessionOutcome を返す run_session と、
    /// sleep 時間を記録する wait を注入して watch_loop を走らせる
    async fn run_scripted_loop(script: Vec<SessionOutcome>) -> Vec<Duration> {
        let outcomes = RefCell::new(script.into_iter());
        let sleeps: RefCell<Vec<Duration>> = RefCell::new(Vec::new());
        watch_loop(
            || {
                let next = outcomes.borrow_mut().next().unwrap_or(SessionOutcome::Stop);
                async move { next }
            },
            |d| {
                sleeps.borrow_mut().push(d);
                async {}
            },
        )
        .await;
        sleeps.into_inner()
    }

    #[tokio::test]
    async fn test_watch_loop_stops_immediately_on_stop() {
        let sleeps = run_scripted_loop(vec![SessionOutcome::Stop]).await;
        assert!(sleeps.is_empty(), "Stop では再接続せず即終了する");
    }

    #[tokio::test]
    async fn test_watch_loop_backs_off_exponentially_on_connect_failure() {
        let sleeps = run_scripted_loop(vec![
            SessionOutcome::ConnectFailed,
            SessionOutcome::ConnectFailed,
            SessionOutcome::ConnectFailed,
            SessionOutcome::Stop,
        ])
        .await;
        assert_eq!(
            sleeps,
            vec![
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(120),
            ]
        );
    }

    #[tokio::test]
    async fn test_watch_loop_resets_backoff_after_healthy_session() {
        // 失敗が続いた後に接続成功→切断（Disconnected）するとバックオフが初期値に戻る
        let sleeps = run_scripted_loop(vec![
            SessionOutcome::ConnectFailed,
            SessionOutcome::ConnectFailed,
            SessionOutcome::Disconnected,
            SessionOutcome::ConnectFailed,
            SessionOutcome::Stop,
        ])
        .await;
        assert_eq!(
            sleeps,
            vec![
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(30), // Disconnected でリセット
                Duration::from_secs(30), // リセット後の最初の失敗も初期値から
            ]
        );
    }

    #[tokio::test]
    async fn test_watch_loop_backoff_is_capped() {
        let mut script = vec![SessionOutcome::ConnectFailed; 8];
        script.push(SessionOutcome::Stop);
        let sleeps = run_scripted_loop(script).await;
        assert_eq!(
            sleeps,
            vec![
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(120),
                Duration::from_secs(240),
                Duration::from_secs(480),
                Duration::from_secs(600), // 10分でキャップ
                Duration::from_secs(600),
                Duration::from_secs(600),
            ]
        );
    }

    #[test]
    fn test_idle_refresh_interval_is_within_rfc2177_limit() {
        // RFC 2177: 29分以内の再発行を推奨（サーバーの30分無通信切断対策）
        assert!(IDLE_REFRESH_INTERVAL < Duration::from_secs(29 * 60));
        assert_eq!(IDLE_REFRESH_INTERVAL, Duration::from_secs(25 * 60));
    }
}
