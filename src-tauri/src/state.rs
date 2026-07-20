use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use tauri::async_runtime::JoinHandle;

use crate::error::AppError;
use crate::secure_store::SecureStore;

pub struct DbState(pub Mutex<Connection>);

impl DbState {
    /// DB 接続のロック取得とロックエラー変換を共通化するヘルパ。
    /// `let conn = state.0.lock().map_err(AppError::lock_err)?` の定型を置き換える。
    /// クロージャの間ロックを保持するため、await を挟む処理には使わないこと。
    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let conn = self.0.lock().map_err(AppError::lock_err)?;
        f(&conn)
    }

    /// `with_conn` の可変版（`Connection::transaction` 等で &mut が必要な場合）。
    pub fn with_conn_mut<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut conn = self.0.lock().map_err(AppError::lock_err)?;
        f(&mut conn)
    }
}

/// SecureStore の遅延初期化ホルダ（ADR 0006 決定 1）。
///
/// Stronghold のオープンはスナップショット暗号化時の scrypt が支配的で、
/// 保管件数に依存せず固定で数十秒かかる。これを `tauri::Builder` 構築前に
/// 実行すると、ウィンドウが存在しない状態で待つことになりユーザーには完全な
/// 無反応に見える。SecureStore は起動時には一切使われず、実際の利用は
/// `sync_account` の資格情報解決などユーザー操作時であるため、最初に秘密情報を
/// 必要とする操作の時点まで初期化を遅延させる。
///
/// 初期化に失敗した場合は panic せず `AppError` を呼び出し元のコマンドへ伝播する
/// （agent.md の unwrap/expect 禁止規約。遅延化後の失敗は既にウィンドウが出ている
/// 時点で起きるため、落とすのは体験上最悪である）。失敗は記憶されないため、
/// 次回アクセス時に再試行される。
///
/// ## ProcessLock との関係（重要）
///
/// GUI と CLI が同じ Stronghold スナップショットを同時に開くと、後から commit
/// した側が先の書き込みを丸ごと上書きしてシークレットが無言で消える
/// （`cli::lock` のコメント参照）。したがって **`init` が実際に呼ばれる時点で
/// `ProcessLock` が保持されていなければならない**。
///
/// GUI ではこれを次のスコープ規則で保証している。`lib.rs::run()` が
/// `ProcessLock` を `_process_lock` としてローカル変数に束縛し、その後
/// `tauri::Builder::…::run()` をブロッキング呼び出しする。`run()` はイベント
/// ループが終了するまで戻らないため、アプリが動いている間ずっと
/// `_process_lock` は `run()` のスタックフレーム上で生存する。この
/// `SecureStoreState` へアクセスしうるのは Tauri コマンドとイベントループ上に
/// spawn されたタスクだけで、いずれもイベントループより長生きできない。
/// よって遅延初期化がいつ走ろうと、必ずロック保持中である。
pub struct SecureStoreState {
    cell: OnceLock<SecureStore>,
    /// 実体を生成するクロージャ。本番は Stronghold のオープン、テストは InMemory。
    /// 初期化が数十秒かかるため、並行アクセスを直列化して二重初期化を防ぐ。
    init: Mutex<Box<dyn Fn() -> Result<SecureStore, AppError> + Send + Sync>>,
}

impl SecureStoreState {
    /// 遅延初期化する SecureStoreState を作る。`init` は最初のアクセス時に呼ばれ、
    /// 一度成功したら二度と呼ばれない（失敗した場合のみ次回アクセスで再試行する）。
    pub fn lazy(init: impl Fn() -> Result<SecureStore, AppError> + Send + Sync + 'static) -> Self {
        Self {
            cell: OnceLock::new(),
            init: Mutex::new(Box::new(init)),
        }
    }

    /// 初期化済みの実体から作る（テスト用途）。
    pub fn ready(store: SecureStore) -> Self {
        let cell = OnceLock::new();
        // 生成直後の空セルへの set なので必ず成功する
        let _ = cell.set(store);
        Self {
            cell,
            init: Mutex::new(Box::new(|| {
                Err(AppError::Stronghold(
                    "secure store already initialized".into(),
                ))
            })),
        }
    }

    /// SecureStore への参照を得る。未初期化なら**この時点で**初期化する
    /// （数十秒かかりうる）。初期化に失敗した場合はエラーを返し、実体は
    /// 未初期化のまま残す（次回アクセスで再試行される）。
    pub fn get(&self) -> Result<&SecureStore, AppError> {
        // 高速パス: 初期化済みならロックを取らずに返す
        if let Some(store) = self.cell.get() {
            return Ok(store);
        }
        // 低速パス: 初期化は数十秒かかるため、並行アクセスを直列化して
        // 同じスナップショットを二重に開かないようにする
        let init = self.init.lock().map_err(AppError::lock_err)?;
        // ロック待ちの間に別スレッドが初期化を終えている可能性がある
        if let Some(store) = self.cell.get() {
            return Ok(store);
        }
        let store = init()?;
        // 上で未初期化を確認しロックも保持しているため set は成功する。
        // 万一競合しても既存の実体を返せばよい（捨てる側の値は使わない）
        let _ = self.cell.set(store);
        self.cell.get().ok_or_else(|| {
            AppError::Stronghold("secure store initialization raced unexpectedly".into())
        })
    }

    /// 既に初期化済みかどうか（起動パスに初期化が漏れていないことの検証用）。
    pub fn is_initialized(&self) -> bool {
        self.cell.get().is_some()
    }
}

/// アカウント単位の同期実行ロック。
/// スレッド一覧は表示のたびに sync_account を呼ぶため、画面遷移や
/// React 開発モードの二重 effect で同一アカウントの同期が並行し得る。
/// 並行すると全員が同期前の max_uid を見て同じメールを多重取り込みするので、
/// ここで直列化する（2本目以降は開始せず即リターンさせる）。
#[derive(Default)]
pub struct SyncLocks(Mutex<HashSet<String>>);

impl SyncLocks {
    pub fn new() -> Self {
        Self::default()
    }

    /// 同期を開始できれば true。同じアカウントの同期が進行中なら false。
    pub fn try_begin(&self, account_id: &str) -> bool {
        match self.0.lock() {
            Ok(mut in_flight) => in_flight.insert(account_id.to_string()),
            // ロックが毒化していたら安全側（開始しない）
            Err(_) => false,
        }
    }

    /// 同期の終了を記録する（成功・失敗を問わず必ず呼ぶ）。
    pub fn finish(&self, account_id: &str) {
        if let Ok(mut in_flight) = self.0.lock() {
            in_flight.remove(account_id);
        }
    }
}

/// 送信添付として許可されたファイルパスの集合。
///
/// 添付は `pick_attachment_files`（ネイティブダイアログ）で選択されたパスのみを
/// 許可し、`send_mail` はこの集合に無いパスを読み取らない。フロントエンドが
/// XSS 等で侵害されても、任意の絶対パス（SSH 鍵等）を添付として外部送出する
/// 経路を塞ぐ（`attachment_commands.rs` が保存先をダイアログ限定にしたのと
/// 同じ防御思想）。プロセス内メモリのためアプリ再起動で消える（下書きは
/// 添付パスを永続化しないため問題にならない）。
#[derive(Default)]
pub struct ApprovedAttachments(Mutex<HashSet<String>>);

impl ApprovedAttachments {
    pub fn new() -> Self {
        Self::default()
    }

    /// ダイアログで選択されたパスを許可リストへ登録する。
    pub fn approve(&self, paths: impl IntoIterator<Item = String>) -> Result<(), AppError> {
        let mut set = self.0.lock().map_err(AppError::lock_err)?;
        set.extend(paths);
        Ok(())
    }

    /// パスが許可済みか。
    pub fn contains(&self, path: &str) -> Result<bool, AppError> {
        let set = self.0.lock().map_err(AppError::lock_err)?;
        Ok(set.contains(path))
    }
}

/// アカウント毎の IMAP IDLE 監視タスク（mail_sync::idle）の管理。
/// 開始（insert）・停止（stop）を account_id 単位で行う。
/// 起動時・アカウント追加・OAuth 完了時に開始し、アカウント削除時に停止する
#[derive(Default)]
pub struct IdleWatchers(Mutex<HashMap<String, JoinHandle<()>>>);

impl IdleWatchers {
    pub fn new() -> Self {
        Self::default()
    }

    /// 監視タスクを登録する。同一アカウントの既存タスクは中断して置き換える
    /// （OAuth 再認証後の監視再開もこの置き換えで実現する）
    pub fn insert(&self, account_id: &str, handle: JoinHandle<()>) {
        match self.0.lock() {
            Ok(mut map) => {
                if let Some(old) = map.insert(account_id.to_string(), handle) {
                    old.abort();
                }
            }
            // ロックが毒化していたら登録できない。渡されたタスクは停止しておく
            Err(_) => handle.abort(),
        }
    }

    /// 監視タスクを中断して登録解除する（アカウント削除時）
    pub fn stop(&self, account_id: &str) {
        if let Ok(mut map) = self.0.lock() {
            if let Some(handle) = map.remove(account_id) {
                handle.abort();
            }
        }
    }

    /// 監視タスクが登録されているか
    pub fn is_watching(&self, account_id: &str) -> bool {
        self.0
            .lock()
            .map(|map| map.contains_key(account_id))
            .unwrap_or(false)
    }
}

/// 埋め込みパス（embedding::worker::run_embedding_pass）の多重起動ガード。
/// 起動時スキャンと同期完了後の両方から spawn され得るため、片方が
/// 走行中はもう片方を即 return させる（キューの奪い合い・二重 DB ロック待ちを防ぐ）。
#[derive(Default, Clone)]
pub struct EmbeddingRunGuard(std::sync::Arc<AtomicBool>);

impl EmbeddingRunGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// 開始できれば true（走行中フラグを立てる）。既に走行中なら false。
    pub fn try_begin(&self) -> bool {
        self.0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// パス終了時に必ず呼ぶ（成功・失敗を問わず）。
    pub fn finish(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_task() -> JoinHandle<()> {
        tauri::async_runtime::spawn(async {
            // 監視タスクの代役: abort されるまで待つだけ
            std::future::pending::<()>().await;
        })
    }

    #[test]
    fn test_idle_watchers_insert_and_stop() {
        let watchers = IdleWatchers::new();
        assert!(!watchers.is_watching("acc1"));

        watchers.insert("acc1", dummy_task());
        assert!(watchers.is_watching("acc1"));
        assert!(!watchers.is_watching("acc2"), "別アカウントには影響しない");

        watchers.stop("acc1");
        assert!(!watchers.is_watching("acc1"));
    }

    #[test]
    fn test_idle_watchers_insert_replaces_existing_task() {
        let watchers = IdleWatchers::new();
        watchers.insert("acc1", dummy_task());
        // 置き換えても二重登録にならない（旧タスクは abort される）
        watchers.insert("acc1", dummy_task());
        assert!(watchers.is_watching("acc1"));
        watchers.stop("acc1");
        assert!(!watchers.is_watching("acc1"));
    }

    #[test]
    fn test_idle_watchers_stop_unknown_account_is_noop() {
        let watchers = IdleWatchers::new();
        watchers.stop("missing"); // panic しない
        assert!(!watchers.is_watching("missing"));
    }

    #[test]
    fn test_second_begin_is_rejected_while_in_flight() {
        let locks = SyncLocks::new();
        assert!(locks.try_begin("acc1"));
        assert!(!locks.try_begin("acc1"), "同一アカウントの多重開始は拒否");
    }

    #[test]
    fn test_different_accounts_run_concurrently() {
        let locks = SyncLocks::new();
        assert!(locks.try_begin("acc1"));
        assert!(locks.try_begin("acc2"), "別アカウントは並行してよい");
    }

    #[test]
    fn test_finish_allows_next_sync() {
        let locks = SyncLocks::new();
        assert!(locks.try_begin("acc1"));
        locks.finish("acc1");
        assert!(locks.try_begin("acc1"), "終了後は再び開始できる");
    }

    #[test]
    fn test_embedding_run_guard_rejects_second_begin_while_running() {
        let guard = EmbeddingRunGuard::new();
        assert!(guard.try_begin());
        assert!(!guard.try_begin(), "走行中の多重起動は拒否");
    }

    #[test]
    fn test_embedding_run_guard_finish_allows_next_run() {
        let guard = EmbeddingRunGuard::new();
        assert!(guard.try_begin());
        guard.finish();
        assert!(guard.try_begin(), "終了後は再び開始できる");
    }

    // --- SecureStoreState の遅延初期化（ADR 0006 決定 1） ---
    //
    // 実 Stronghold はスナップショット I/O が 1 回 55 秒かかるため、
    // ここでは一切使わない。初期化の「回数」と「タイミング」だけを検証する。

    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    /// 初期化回数を数える遅延ストア。init は InMemory を返すだけで I/O しない。
    fn counting_lazy_store() -> (SecureStoreState, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let state = SecureStoreState::lazy(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(SecureStore::in_memory())
        });
        (state, calls)
    }

    #[test]
    fn test_secure_store_not_initialized_before_first_access() {
        // 起動パスの再現: manage 相当の構築を行っただけでは初期化されない。
        // これが数十秒の起動ブロックを外せている根拠になる
        let (state, calls) = counting_lazy_store();
        assert!(!state.is_initialized(), "構築のみでは初期化されない");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "初期化クロージャは一度も呼ばれない"
        );
    }

    #[test]
    fn test_secure_store_initialized_on_first_access() {
        let (state, calls) = counting_lazy_store();
        state.get().expect("初回アクセスで初期化される");
        assert!(state.is_initialized());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "初回アクセスで1回だけ初期化"
        );
    }

    #[test]
    fn test_secure_store_initializes_only_once_across_accesses() {
        // warming と sync_account が続けて呼んでも二重初期化しない
        let (state, calls) = counting_lazy_store();
        state.get().expect("1回目");
        state.get().expect("2回目");
        state.get().expect("3回目");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "初期化は高々1回");
    }

    #[test]
    fn test_secure_store_shares_the_same_instance() {
        // 同じ実体を共有すること（アクセスごとに別ストアを作らない）
        let (state, _) = counting_lazy_store();
        state.get().expect("get").insert("k", b"v").expect("insert");
        assert_eq!(
            state.get().expect("get").get("k").expect("get key"),
            Some(b"v".to_vec()),
            "2回目のアクセスは1回目と同じ実体を返す"
        );
    }

    #[test]
    fn test_secure_store_init_failure_returns_error_instead_of_panicking() {
        // 遅延化後の失敗はウィンドウ表示後に起きる。panic せず
        // 呼び出し元のコマンドへ伝播すること（agent.md の expect 禁止規約）
        let state =
            SecureStoreState::lazy(|| Err(AppError::Stronghold("keychain unavailable".into())));
        let err = match state.get() {
            Err(e) => e,
            Ok(_) => panic!("初期化失敗はエラーとして返る"),
        };
        assert!(
            err.to_string().contains("keychain unavailable"),
            "原因が隠蔽されずに伝播する: {err}"
        );
        assert!(
            !state.is_initialized(),
            "失敗した初期化は記憶されない（サイレントに壊れた状態を残さない）"
        );
    }

    #[test]
    fn test_secure_store_retries_after_failure() {
        // 失敗は記憶しない: キーチェーンの許可ダイアログを一度拒否しても、
        // 次の操作で再試行できる（恒久的に使用不能にしない）
        let attempts = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&attempts);
        let state = SecureStoreState::lazy(move || {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(AppError::Stronghold("transient failure".into()));
            }
            Ok(SecureStore::in_memory())
        });

        assert!(state.get().is_err(), "1回目は失敗する");
        state.get().expect("2回目は再試行して成功する");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_secure_store_concurrent_access_initializes_once() {
        // 起動直後のバックグラウンド warming と、ユーザー操作による
        // sync_account が同時に来ても二重初期化しないこと。
        // 二重に開くとスナップショットの後勝ち上書きでシークレットが消える
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let state = Arc::new(SecureStoreState::lazy(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            // 初期化は本番では数十秒かかる。競合窓を広げて検証する
            std::thread::sleep(std::time::Duration::from_millis(50));
            Ok(SecureStore::in_memory())
        }));

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || state.get().map(|_| ()))
            })
            .collect();
        for handle in handles {
            handle
                .join()
                .expect("スレッドは panic しない")
                .expect("各スレッドが取得に成功する");
        }

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "並行アクセスでも初期化は1回だけ"
        );
    }

    #[test]
    fn test_secure_store_ready_is_initialized_without_init() {
        let state = SecureStoreState::ready(SecureStore::in_memory());
        assert!(state.is_initialized(), "実体を渡した場合は初期化済み");
        state.get().expect("init を呼ばずに取得できる");
    }
}
