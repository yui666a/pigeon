use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rand::RngCore;
use sha2::Digest;
use zeroize::Zeroizing;

use crate::error::AppError;

/// SecureStore マスター鍵の長さ（Stronghold の KeyProvider が要求する 32byte）。
pub const MASTER_KEY_LEN: usize = 32;

/// マスター鍵の保管先の抽象（本番: OS キーチェーン / テスト: インメモリ）。
///
/// 鍵はデバイス固有の CSPRNG 乱数であり、ソースコードに鍵素材を置かない
/// （ADR 0003）。バックエンドが違えば鍵も違うため、スナップショットは
/// デバイス間で相互に復号できない。
pub trait MasterKeyBackend {
    fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>, AppError>;
    fn store(&self, key: &[u8]) -> Result<(), AppError>;
    /// エラーメッセージ用の保管先説明。
    fn describe(&self) -> String;
}

/// OS キーチェーン保管（macOS Keychain / Windows Credential Manager /
/// Linux secret-service）。Linux は zbus ベースの async-secret-service 経由
/// （pure Rust。デーモン不在の環境は FallbackKeyBackend でファイル保管へ落とす）。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub struct KeychainBackend {
    service: String,
    account: String,
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl KeychainBackend {
    pub fn new(service: &str, account: &str) -> Self {
        Self {
            service: service.to_string(),
            account: account.to_string(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, AppError> {
        keyring::Entry::new(&self.service, &self.account)
            .map_err(|e| AppError::Stronghold(format!("keychain entry unavailable: {e}")))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl MasterKeyBackend for KeychainBackend {
    fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>, AppError> {
        match self.entry()?.get_secret() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Stronghold(format!(
                "failed to read master key from keychain: {e}"
            ))),
        }
    }

    fn store(&self, key: &[u8]) -> Result<(), AppError> {
        self.entry()?
            .set_secret(key)
            .map_err(|e| AppError::Stronghold(format!("failed to store master key: {e}")))
    }

    fn describe(&self) -> String {
        format!("OS keychain ({}/{})", self.service, self.account)
    }
}

/// ファイル保管（キーチェーン非対応環境の暫定。所有者のみ読書き可 0600）。
///
/// Linux の secret-service 連携は将来課題（ADR 0003）。固定鍵と違い
/// デバイス固有の乱数である点は同じで、ソース公開でも鍵は漏れない。
pub struct FileKeyBackend {
    path: PathBuf,
}

impl FileKeyBackend {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl MasterKeyBackend for FileKeyBackend {
    fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>, AppError> {
        if !self.path.exists() {
            return Ok(None);
        }
        std::fs::read(&self.path)
            .map(|bytes| Some(Zeroizing::new(bytes)))
            .map_err(|e| AppError::Stronghold(format!("failed to read master key file: {e}")))
    }

    fn store(&self, key: &[u8]) -> Result<(), AppError> {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&self.path)
            .map_err(|e| AppError::Stronghold(format!("failed to create master key file: {e}")))?;
        file.write_all(key)
            .map_err(|e| AppError::Stronghold(format!("failed to write master key file: {e}")))
    }

    fn describe(&self) -> String {
        format!("key file ({})", self.path.display())
    }
}

/// 主バックエンド（キーチェーン）が使えない環境で予備（ファイル）へ落とす
/// 合成バックエンド。Linux の secret-service はヘッドレス環境や CI で
/// デーモンが存在しないため、実行時フォールバックが必須（ADR 0003）。
///
/// - load: 主 → 予備の順。主が空で予備に鍵がある場合は主へ複製する
///   （旧 FileKeyBackend からの移行。ファイルは可用性のため残す:
///   デーモンが一時的に不在の起動でも同じ鍵で開けるようにする）
/// - store: 主に保存できたらファイルは作らない。主が失敗したときのみ予備へ
pub struct FallbackKeyBackend<P, F> {
    primary: P,
    fallback: F,
}

impl<P: MasterKeyBackend, F: MasterKeyBackend> FallbackKeyBackend<P, F> {
    pub fn new(primary: P, fallback: F) -> Self {
        Self { primary, fallback }
    }
}

impl<P: MasterKeyBackend, F: MasterKeyBackend> MasterKeyBackend for FallbackKeyBackend<P, F> {
    fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>, AppError> {
        match self.primary.load() {
            Ok(Some(key)) => Ok(Some(key)),
            Ok(None) => match self.fallback.load()? {
                Some(key) => {
                    // 予備にだけ鍵がある = 旧ファイル保管からの移行。主へ複製する。
                    // 複製失敗でも鍵自体は返す（起動を止めない）
                    if let Err(e) = self.primary.store(&key) {
                        eprintln!(
                            "[warn] master key: failed to migrate key into {}: {e}",
                            self.primary.describe()
                        );
                    } else {
                        eprintln!(
                            "[info] master key: migrated from {} to {}",
                            self.fallback.describe(),
                            self.primary.describe()
                        );
                    }
                    Ok(Some(key))
                }
                None => Ok(None),
            },
            Err(e) => {
                eprintln!(
                    "[warn] master key: {} unavailable ({e}); falling back to {}",
                    self.primary.describe(),
                    self.fallback.describe()
                );
                self.fallback.load()
            }
        }
    }

    fn store(&self, key: &[u8]) -> Result<(), AppError> {
        match self.primary.store(key) {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!(
                    "[warn] master key: failed to store into {} ({e}); falling back to {}",
                    self.primary.describe(),
                    self.fallback.describe()
                );
                self.fallback.store(key)
            }
        }
    }

    fn describe(&self) -> String {
        format!(
            "{} (fallback: {})",
            self.primary.describe(),
            self.fallback.describe()
        )
    }
}

/// 実行環境に応じた既定のマスター鍵バックエンドを返す。
pub fn default_master_key_backend(data_dir: &Path) -> Box<dyn MasterKeyBackend> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let _ = data_dir;
        Box::new(KeychainBackend::new(
            "com.haiso666.pigeon",
            "secure-store-master-key",
        ))
    }
    #[cfg(target_os = "linux")]
    {
        // secret-service（GNOME Keyring 等）を優先し、デーモン不在なら
        // 従来の master.key ファイル（0600）へフォールバックする
        Box::new(FallbackKeyBackend::new(
            KeychainBackend::new("com.haiso666.pigeon", "secure-store-master-key"),
            FileKeyBackend::new(data_dir.join("master.key")),
        ))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Box::new(FileKeyBackend::new(data_dir.join("master.key")))
    }
}

/// マスター鍵を取得する。未生成なら CSPRNG で 32byte を生成して保管する。
pub fn resolve_master_key(
    backend: &(impl MasterKeyBackend + ?Sized),
) -> Result<Zeroizing<Vec<u8>>, AppError> {
    if let Some(key) = backend.load()? {
        if key.len() != MASTER_KEY_LEN {
            return Err(AppError::Stronghold(format!(
                "master key in {} has unexpected length {} (expected {MASTER_KEY_LEN})",
                backend.describe(),
                key.len()
            )));
        }
        return Ok(key);
    }
    let mut key = Zeroizing::new(vec![0u8; MASTER_KEY_LEN]);
    rand::rngs::OsRng.fill_bytes(&mut key);
    backend.store(&key)?;
    Ok(key)
}

/// 旧実装（〜2026-07）の固定鍵。**新規の暗号化には決して使わない。**
/// 既存スナップショットを新しいランダム鍵へ再暗号化する移行のためだけに残す。
fn legacy_fixed_key() -> Zeroizing<Vec<u8>> {
    Zeroizing::new(
        sha2::Sha256::digest(b"com.haiso666.pigeon-secure-store-key")
            .as_slice()
            .to_vec(),
    )
}

/// `open_with_migration` の結果（起動ログとテスト検証用）。
#[derive(Debug)]
pub enum MasterKeyMigration {
    /// スナップショットが無く、新規作成した。
    FreshStore,
    /// 現行鍵でそのまま開けた。
    AlreadyCurrent,
    /// 旧固定鍵のスナップショットを現行鍵で再暗号化した。
    MigratedFromLegacy,
    /// どの鍵でも開けず、スナップショットを退避して新規作成した（要再認証）。
    UnreadableBackedUp { backup: PathBuf },
}

/// 秘密情報を失ったときの回復コストによる書き込みの分類（ADR 0006 決定 4）。
///
/// Stronghold のコミットは scrypt（work factor 19、`iota-crypto` の
/// `RECOMMENDED_MINIMUM_ENCRYPT_WORK_FACTOR`）でスナップショット鍵を導出し直すため、
/// 保管件数や値の大きさに関係なく 1 回あたり秒オーダーの固定コストがかかる。
/// この KDF は「性能のために弱めてはならない」ものであり（ADR 0006 却下案・
/// `stronghold_engine` のソースコメントの双方が明記）、削れるのは**コミットの回数**
/// だけである。
///
/// そこで「失ったときに何が起きるか」で書き込みを二分し、回復不能なものだけを
/// 即座に永続化する。速度のために耐久性を一律に捨てるのではなく、捨てても
/// ユーザーに影響が出ないものだけを遅延させる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    /// 失うとユーザーが再認証・再入力を強いられる秘密（リフレッシュトークンを含む
    /// OAuth 一式・パスワード・API キー・SA JSON）。書き込み時に必ずコミットする。
    Critical,
    /// 失っても他の秘密から再取得でき、ユーザー操作を必要としない値。
    /// コミットを遅延させてよい。
    Deferrable,
}

impl Durability {
    /// キー名から耐久性クラスを決める。
    ///
    /// **未知のキーは `Critical` に倒す。** 新しい秘密種別が追加されたときに
    /// 分類を書き忘れても、「黙って失われる」side ではなく「遅いが安全」side に
    /// 落ちるようにするための既定値である。
    pub fn of_key(key: &str) -> Self {
        if key.starts_with(ACCESS_TOKEN_CACHE_PREFIX) {
            Durability::Deferrable
        } else {
            Durability::Critical
        }
    }
}

/// 再取得可能なアクセストークンのキャッシュに使うキー接頭辞。
/// この接頭辞を持つ値だけが遅延コミットの対象になる。
pub const ACCESS_TOKEN_CACHE_PREFIX: &str = "access_token_cache_";

/// 未コミットの変更があるかを追跡し、「いまコミットすべきか」を判断する。
///
/// `Critical` な書き込みは、そこまでに溜まった `Deferrable` な変更も巻き込んで
/// 1 回のコミットで永続化する。遅延分のために追加のコミットは発生しない。
#[derive(Debug, Default)]
struct PendingCommit {
    dirty: std::sync::atomic::AtomicBool,
}

impl PendingCommit {
    fn new() -> Self {
        Self::default()
    }

    /// 書き込みを記録し、いまコミットすべきなら true を返す。
    /// true を返した時点で未コミット状態は解消済みとして扱う。
    fn record(&self, durability: Durability) -> bool {
        use std::sync::atomic::Ordering;
        match durability {
            // 溜まっていた Deferrable もこのコミットで一緒に永続化される
            Durability::Critical => {
                self.dirty.store(false, Ordering::SeqCst);
                true
            }
            Durability::Deferrable => {
                self.dirty.store(true, Ordering::SeqCst);
                false
            }
        }
    }

    /// 未コミットの変更があれば true を返し、同時に解消済みにする。
    /// 変更が無いときにコミットしないことで、無駄な scrypt を避ける。
    fn take_if_uncommitted(&self) -> bool {
        self.dirty.swap(false, std::sync::atomic::Ordering::SeqCst)
    }

    /// コミットに失敗したときに未コミット状態へ戻す。
    /// 失敗したまま「コミット済み」にすると、遅延中だった変更が
    /// 次の flush でも拾われずに失われる。
    fn restore_after_failed_commit(&self) {
        self.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// 未コミットの変更が残っているか（コミット判断の検証用）。
    #[cfg(test)]
    fn has_uncommitted(&self) -> bool {
        self.dirty.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Simple secure key-value store backed by the filesystem.
/// Tokens and passwords are stored as JSON in an encrypted file using iota_stronghold.
/// For now, we use a simpler approach: an in-memory HashMap persisted to an encrypted JSON file
/// via the Stronghold store API.
///
/// The StrongholdCollection managed by tauri-plugin-stronghold is designed for JS-to-Rust comms.
/// We use our own wrapper for Rust-side operations.
pub struct StrongholdStore {
    inner: Mutex<SecureStoreInner>,
    /// 未コミットの遅延書き込みの有無。`inner` とは独立に触れるよう
    /// Mutex の外に置く（`has_uncommitted` のためにロックを取らない）。
    pending: PendingCommit,
}

/// 秘密情報の保管先。本番は Stronghold、テストは InMemory。
///
/// enum ディスパッチにより呼び出し側の `&SecureStore` を変えずに、
/// テストで実 Stronghold（スナップショット I/O が 1 回 55 秒）を回避する。
pub enum SecureStore {
    /// Box で包むのは、Stronghold 側が InMemory より大幅に大きく、
    /// enum 全体のサイズが常に大きい方へ引きずられるため（clippy::large_enum_variant）。
    Stronghold(Box<StrongholdStore>),
    InMemory(Mutex<std::collections::HashMap<String, Vec<u8>>>),
}

struct SecureStoreInner {
    stronghold: iota_stronghold::Stronghold,
    snapshot_path: iota_stronghold::SnapshotPath,
    keyprovider: iota_stronghold::KeyProvider,
    client_path: Vec<u8>,
}

impl StrongholdStore {
    fn new(path: PathBuf, password: &[u8]) -> Result<Self, AppError> {
        let snapshot_path = iota_stronghold::SnapshotPath::from_path(&path);
        let stronghold = iota_stronghold::Stronghold::default();
        let keyprovider =
            iota_stronghold::KeyProvider::try_from(zeroize::Zeroizing::new(password.to_vec()))
                .map_err(|e| AppError::Stronghold(format!("Key derivation failed: {}", e)))?;

        // Load existing snapshot if it exists
        if path.exists() {
            stronghold
                .load_snapshot(&keyprovider, &snapshot_path)
                .map_err(|e| AppError::Stronghold(format!("Failed to load snapshot: {}", e)))?;
        }

        let client_path = b"pigeon".to_vec();

        // 既存スナップショットのクライアントを復元し、無ければ新規作成する。
        // create_client を先に呼ぶと空クライアントが既存データを覆い隠し、
        // 次の commit でスナップショットを空内容で上書きしてしまう（データ消失）
        let _client = stronghold
            .load_client(&client_path)
            .or_else(|_| stronghold.create_client(&client_path))
            .map_err(|e| AppError::Stronghold(format!("Failed to create/load client: {}", e)))?;

        Ok(Self {
            inner: Mutex::new(SecureStoreInner {
                stronghold,
                snapshot_path,
                keyprovider,
                client_path,
            }),
            pending: PendingCommit::new(),
        })
    }

    /// スナップショットを現行鍵で開く。開けない場合は旧固定鍵からの移行を試み、
    /// それも不能なら退避して新規作成する（秘密は失われるが起動は継続できる）。
    fn open_with_migration(
        path: PathBuf,
        key: &[u8],
    ) -> Result<(Self, MasterKeyMigration), AppError> {
        if !path.exists() {
            return Ok((Self::new(path, key)?, MasterKeyMigration::FreshStore));
        }
        match Self::new(path.clone(), key) {
            Ok(store) => Ok((store, MasterKeyMigration::AlreadyCurrent)),
            Err(_) => match Self::new(path.clone(), &legacy_fixed_key()) {
                Ok(store) => {
                    store.reencrypt(key)?;
                    Ok((store, MasterKeyMigration::MigratedFromLegacy))
                }
                Err(_) => {
                    let file_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "pigeon.stronghold".to_string());
                    let backup = path.with_file_name(format!("{file_name}.unreadable.bak"));
                    std::fs::rename(&path, &backup).map_err(|e| {
                        AppError::Stronghold(format!("failed to back up unreadable snapshot: {e}"))
                    })?;
                    Ok((
                        Self::new(path, key)?,
                        MasterKeyMigration::UnreadableBackedUp { backup },
                    ))
                }
            },
        }
    }

    /// スナップショットを新しい鍵で再暗号化して保存し直す（鍵移行用）。
    fn reencrypt(&self, new_password: &[u8]) -> Result<(), AppError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| AppError::Stronghold(e.to_string()))?;
        inner.keyprovider =
            iota_stronghold::KeyProvider::try_from(Zeroizing::new(new_password.to_vec()))
                .map_err(|e| AppError::Stronghold(format!("Key derivation failed: {}", e)))?;
        inner
            .stronghold
            .commit_with_keyprovider(&inner.snapshot_path, &inner.keyprovider)
            .map_err(|e| AppError::Stronghold(format!("Failed to re-encrypt snapshot: {}", e)))
    }

    fn insert(&self, key: &str, value: &[u8]) -> Result<(), AppError> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AppError::Stronghold(e.to_string()))?;
        let client = inner
            .stronghold
            .get_client(&inner.client_path)
            .map_err(|e| AppError::Stronghold(format!("Failed to get client: {}", e)))?;
        let store = client.store();
        store
            .insert(key.as_bytes().to_vec(), value.to_vec(), None)
            .map_err(|e| AppError::Stronghold(format!("Failed to insert: {}", e)))?;
        // ここまででメモリ上の状態は更新済み。get() は常に最新値を読む。
        // スナップショットへの書き出しは耐久性クラスに応じて判断する
        if self.pending.record(Durability::of_key(key)) {
            self.commit_or_restore(&inner)?;
        }
        Ok(())
    }

    /// コミットし、失敗したら未コミット状態へ戻す（次の機会に再試行させる）。
    fn commit_or_restore(&self, inner: &SecureStoreInner) -> Result<(), AppError> {
        Self::commit(inner).inspect_err(|_| self.pending.restore_after_failed_commit())
    }

    /// メモリ上の状態をスナップショットへ書き出す（scrypt が走る重い処理）。
    fn commit(inner: &SecureStoreInner) -> Result<(), AppError> {
        inner
            .stronghold
            .commit_with_keyprovider(&inner.snapshot_path, &inner.keyprovider)
            .map_err(|e| AppError::Stronghold(format!("Failed to save: {}", e)))
    }

    /// 未コミットの遅延書き込みがあればスナップショットへ書き出す。
    /// 変更が無ければ何もしない（無駄な scrypt を走らせない）。
    fn flush(&self) -> Result<(), AppError> {
        if !self.pending.take_if_uncommitted() {
            return Ok(());
        }
        let inner = self
            .inner
            .lock()
            .map_err(|e| AppError::Stronghold(e.to_string()))?;
        self.commit_or_restore(&inner)
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, AppError> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AppError::Stronghold(e.to_string()))?;
        let client = inner
            .stronghold
            .get_client(&inner.client_path)
            .map_err(|e| AppError::Stronghold(format!("Failed to get client: {}", e)))?;
        let store = client.store();
        store
            .get(key.as_bytes())
            .map_err(|e| AppError::Stronghold(format!("Failed to get: {}", e)))
    }

    fn delete(&self, key: &str) -> Result<(), AppError> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| AppError::Stronghold(e.to_string()))?;
        let client = inner
            .stronghold
            .get_client(&inner.client_path)
            .map_err(|e| AppError::Stronghold(format!("Failed to get client: {}", e)))?;
        let store = client.store();
        let _ = store.delete(key.as_bytes());
        // 削除は「秘密を消す」操作なので、Critical なキーでは即座に永続化する。
        // 遅延させると削除したはずの秘密がスナップショットに残り続ける
        if self.pending.record(Durability::of_key(key)) {
            self.commit_or_restore(&inner)?;
        }
        Ok(())
    }
}

impl Drop for StrongholdStore {
    /// 正常終了時に遅延分を取りこぼさないための最後の砦。
    /// 異常終了（プロセスの強制終了・電源断）では走らないため、
    /// これは耐久性の保証ではなく best-effort である。
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            eprintln!("[warn] secure store: failed to flush pending writes on drop: {e}");
        }
    }
}

impl SecureStore {
    /// スナップショットを現行鍵で開く（本番: Stronghold バリアント）。
    pub fn new(path: PathBuf, password: &[u8]) -> Result<Self, AppError> {
        Ok(SecureStore::Stronghold(Box::new(StrongholdStore::new(
            path, password,
        )?)))
    }

    /// スナップショットを現行鍵で開く。開けない場合は旧固定鍵からの移行を試み、
    /// それも不能なら退避して新規作成する（本番: Stronghold バリアント）。
    pub fn open_with_migration(
        path: PathBuf,
        key: &[u8],
    ) -> Result<(Self, MasterKeyMigration), AppError> {
        let (store, migration) = StrongholdStore::open_with_migration(path, key)?;
        Ok((SecureStore::Stronghold(Box::new(store)), migration))
    }

    /// テスト/フォールバック用のインメモリ実装。スナップショット I/O を行わない。
    pub fn in_memory() -> Self {
        SecureStore::InMemory(Mutex::new(std::collections::HashMap::new()))
    }

    pub fn insert(&self, key: &str, value: &[u8]) -> Result<(), AppError> {
        match self {
            SecureStore::Stronghold(s) => s.insert(key, value),
            SecureStore::InMemory(m) => {
                let mut map = m.lock().map_err(|e| AppError::Stronghold(e.to_string()))?;
                map.insert(key.to_string(), value.to_vec());
                Ok(())
            }
        }
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, AppError> {
        match self {
            SecureStore::Stronghold(s) => s.get(key),
            SecureStore::InMemory(m) => {
                let map = m.lock().map_err(|e| AppError::Stronghold(e.to_string()))?;
                Ok(map.get(key).cloned())
            }
        }
    }

    pub fn delete(&self, key: &str) -> Result<(), AppError> {
        match self {
            SecureStore::Stronghold(s) => s.delete(key),
            SecureStore::InMemory(m) => {
                let mut map = m.lock().map_err(|e| AppError::Stronghold(e.to_string()))?;
                map.remove(key);
                Ok(())
            }
        }
    }

    /// 遅延中の書き込みをスナップショットへ確定させる。
    ///
    /// `Durability::Deferrable` な書き込み（再取得可能なアクセストークン等）は
    /// 書き込み時点ではコミットされない。アプリ終了時など、次にプロセスが
    /// 死んでも困らない状態にしておきたい地点で明示的に呼ぶ。
    /// 未コミットの変更が無ければ何もしないため、繰り返し呼んでも安全。
    pub fn flush(&self) -> Result<(), AppError> {
        match self {
            SecureStore::Stronghold(s) => s.flush(),
            // InMemory は永続化しないので何もすることがない
            SecureStore::InMemory(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_in_memory_insert_get_roundtrip() {
        let store = SecureStore::in_memory();
        store.insert("k", b"v").unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some(b"v".as_ref()));
    }

    // --- 書き込みの耐久性クラス分け（ADR 0006 決定 4） ---
    //
    // 実 Stronghold のコミットは scrypt(work factor 19) が固定で走るため
    // 1 回あたり秒オーダーであり、書き込みのたびに実行するとその間 get() が
    // 同じ Mutex で待たされる。コミット回数そのものを減らすのが本質的な対策で、
    // ここではその「いつコミットするか」の判断ロジックを実 I/O 抜きで検証する。

    #[test]
    fn test_durability_of_key_classifies_credentials_as_critical() {
        // 再取得不能な秘密（リフレッシュトークンを含む OAuth 一式・パスワード・
        // API キー・SA JSON）は、失うとユーザーが再認証を強いられる。
        // これらは即座にスナップショットへ書き出す
        assert_eq!(Durability::of_key("oauth_acc1"), Durability::Critical);
        assert_eq!(Durability::of_key("password_acc1"), Durability::Critical);
        assert_eq!(Durability::of_key("claude_api_key"), Durability::Critical);
        assert_eq!(Durability::of_key("vertex_sa_json"), Durability::Critical);
    }

    #[test]
    fn test_durability_defaults_to_critical_for_unknown_keys() {
        // 新しい秘密種別を追加したときに、分類漏れが「失ってよい」側へ
        // 倒れてはならない。未知のキーは安全側（即コミット）に倒す
        assert_eq!(
            Durability::of_key("some_future_secret"),
            Durability::Critical,
            "未知のキーは安全側（即コミット）に倒す"
        );
    }

    #[test]
    fn test_durability_classifies_access_token_cache_as_deferrable() {
        // アクセストークンはリフレッシュトークンから再取得できる。
        // 失っても再認証は不要（次回同期で再発行される）ため遅延コミットしてよい
        assert_eq!(
            Durability::of_key("access_token_cache_acc1"),
            Durability::Deferrable
        );
    }

    #[test]
    fn test_in_memory_get_missing_returns_none() {
        let store = SecureStore::in_memory();
        assert_eq!(store.get("nope").unwrap(), None);
    }

    #[test]
    fn test_in_memory_overwrite_and_delete() {
        let store = SecureStore::in_memory();
        store.insert("k", b"v1").unwrap();
        store.insert("k", b"v2").unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some(b"v2".as_ref()));
        store.delete("k").unwrap();
        assert_eq!(store.get("k").unwrap(), None);
    }

    // --- コミット回数の制御（PendingCommit） ---
    //
    // 実 Stronghold を使わずに「何回コミットが発火したか」を数えるため、
    // コミット先を差し替えられる PendingCommit のロジックだけを検証する。

    #[test]
    fn test_critical_write_commits_immediately() {
        let pending = PendingCommit::new();
        assert!(
            pending.record(Durability::Critical),
            "再取得不能な秘密は即コミットする"
        );
        assert!(
            !pending.has_uncommitted(),
            "即コミット後は未コミットの変更が残らない"
        );
    }

    #[test]
    fn test_deferrable_write_does_not_commit_immediately() {
        let pending = PendingCommit::new();
        assert!(
            !pending.record(Durability::Deferrable),
            "再取得可能な値は即コミットしない"
        );
        assert!(
            pending.has_uncommitted(),
            "未コミットの変更として記録される"
        );
    }

    #[test]
    fn test_critical_write_flushes_pending_deferrable_writes() {
        // 遅延中の変更があるところに Critical が来たら、両方まとめて 1 回で書く。
        // 遅延分のために追加のコミットを発生させない（コミット回数を増やさない）
        let pending = PendingCommit::new();
        pending.record(Durability::Deferrable);
        assert!(pending.has_uncommitted());

        assert!(pending.record(Durability::Critical));
        assert!(
            !pending.has_uncommitted(),
            "Critical のコミットが遅延分もまとめて永続化する"
        );
    }

    #[test]
    fn test_flush_commits_only_when_there_are_pending_writes() {
        let pending = PendingCommit::new();
        assert!(
            !pending.take_if_uncommitted(),
            "未コミットの変更が無ければコミットしない（無駄な scrypt を避ける）"
        );

        pending.record(Durability::Deferrable);
        assert!(
            pending.take_if_uncommitted(),
            "未コミットの変更があればコミットする"
        );
        assert!(
            !pending.take_if_uncommitted(),
            "同じ変更を二度コミットしない"
        );
    }

    #[test]
    fn test_failed_commit_keeps_pending_state_for_retry() {
        // コミットが失敗したら未コミット状態を戻す。
        // 戻さないと、遅延中だった変更が「書けていないのに書けたことになる」
        // 状態で忘れ去られ、次の flush でも拾われずに失われる
        let pending = PendingCommit::new();
        pending.record(Durability::Deferrable);
        assert!(pending.record(Durability::Critical));

        pending.restore_after_failed_commit();
        assert!(
            pending.has_uncommitted(),
            "コミット失敗後は未コミット扱いに戻し、次の機会に再試行する"
        );
    }

    #[test]
    fn test_many_deferrable_writes_collapse_into_one_commit() {
        // 遅延書き込みが何回来てもコミットは 1 回に畳まれる
        let pending = PendingCommit::new();
        for _ in 0..100 {
            assert!(!pending.record(Durability::Deferrable));
        }
        assert!(pending.take_if_uncommitted());
        assert!(!pending.take_if_uncommitted());
    }

    /// テスト用のインメモリ鍵バックエンド（キーチェーンの代役）。
    struct InMemoryBackend(Mutex<Option<Vec<u8>>>);

    impl InMemoryBackend {
        fn empty() -> Self {
            Self(Mutex::new(None))
        }
    }

    impl MasterKeyBackend for InMemoryBackend {
        fn load(&self) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, AppError> {
            Ok(self.0.lock().unwrap().clone().map(zeroize::Zeroizing::new))
        }

        fn store(&self, key: &[u8]) -> Result<(), AppError> {
            *self.0.lock().unwrap() = Some(key.to_vec());
            Ok(())
        }

        fn describe(&self) -> String {
            "in-memory (test)".to_string()
        }
    }

    /// 常に失敗するバックエンド（secret-service デーモン不在の模擬）。
    struct FailingBackend;

    impl MasterKeyBackend for FailingBackend {
        fn load(&self) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, AppError> {
            Err(AppError::Stronghold("daemon unavailable".into()))
        }

        fn store(&self, _key: &[u8]) -> Result<(), AppError> {
            Err(AppError::Stronghold("daemon unavailable".into()))
        }

        fn describe(&self) -> String {
            "failing (test)".to_string()
        }
    }

    // --- FallbackKeyBackend（Linux: secret-service 優先 + ファイル退避） ---

    #[test]
    fn test_fallback_prefers_primary_and_does_not_touch_fallback() {
        let backend = FallbackKeyBackend::new(InMemoryBackend::empty(), InMemoryBackend::empty());
        let key = resolve_master_key(&backend).unwrap();

        assert_eq!(*backend.primary.load().unwrap().unwrap(), *key);
        assert!(
            backend.fallback.load().unwrap().is_none(),
            "主が健在なら予備（ファイル）に鍵を作らない"
        );
    }

    #[test]
    fn test_fallback_migrates_existing_fallback_key_into_primary() {
        // 旧 FileKeyBackend 運用からの移行: 予備にだけ鍵がある状態
        let backend = FallbackKeyBackend::new(InMemoryBackend::empty(), InMemoryBackend::empty());
        backend.fallback.store(&[9u8; MASTER_KEY_LEN]).unwrap();

        let key = resolve_master_key(&backend).unwrap();
        assert_eq!(*key, [9u8; MASTER_KEY_LEN]);
        assert_eq!(
            *backend.primary.load().unwrap().unwrap(),
            [9u8; MASTER_KEY_LEN],
            "予備の鍵が主へ複製される"
        );
        assert!(
            backend.fallback.load().unwrap().is_some(),
            "予備の鍵は可用性のため残す（デーモン不在時の起動用）"
        );
    }

    #[test]
    fn test_fallback_uses_fallback_when_primary_unavailable() {
        // デーモン不在: load/store とも予備で完結し、鍵は安定して同じものを返す
        let backend = FallbackKeyBackend::new(FailingBackend, InMemoryBackend::empty());
        let key = resolve_master_key(&backend).unwrap();
        let key2 = resolve_master_key(&backend).unwrap();
        assert_eq!(*key, *key2);
        assert_eq!(*backend.fallback.load().unwrap().unwrap(), *key);
    }

    #[test]
    fn test_fallback_store_errors_only_if_both_fail() {
        let backend = FallbackKeyBackend::new(FailingBackend, FailingBackend);
        assert!(resolve_master_key(&backend).is_err());
    }

    // --- resolve_master_key ---

    #[test]
    fn test_resolve_master_key_generates_and_persists_32_bytes() {
        let backend = InMemoryBackend::empty();
        let key = resolve_master_key(&backend).unwrap();
        assert_eq!(key.len(), MASTER_KEY_LEN);
        // 2回目は保存済みの同じ鍵を返す（毎回生成し直さない）
        let key2 = resolve_master_key(&backend).unwrap();
        assert_eq!(*key, *key2);
    }

    #[test]
    fn test_resolve_master_key_distinct_per_backend() {
        // デバイス（=バックエンド）ごとに鍵は異なる（固定鍵の廃止）
        let key_a = resolve_master_key(&InMemoryBackend::empty()).unwrap();
        let key_b = resolve_master_key(&InMemoryBackend::empty()).unwrap();
        assert_ne!(*key_a, *key_b);
    }

    #[test]
    fn test_resolve_master_key_rejects_wrong_length() {
        let backend = InMemoryBackend::empty();
        backend.store(b"short-key").unwrap();
        assert!(resolve_master_key(&backend).is_err());
    }

    // --- FileKeyBackend（Linux 等キーチェーン非対応環境の暫定保管先） ---

    #[test]
    fn test_file_backend_roundtrip() {
        let dir = TempDir::new().unwrap();
        let backend = FileKeyBackend::new(dir.path().join("master.key"));
        assert!(backend.load().unwrap().is_none());
        let key = resolve_master_key(&backend).unwrap();
        assert_eq!(*resolve_master_key(&backend).unwrap(), *key);
    }

    #[cfg(unix)]
    #[test]
    fn test_file_backend_sets_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("master.key");
        let backend = FileKeyBackend::new(path.clone());
        resolve_master_key(&backend).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "鍵ファイルは所有者のみ読書き可");
    }

    // --- SecureStore: スナップショットの再オープン ---

    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_secure_store_reopen_reads_persisted_value() {
        // 再オープンで既存データが読めること。create_client を先に呼ぶ旧実装は
        // 空クライアントが既存データを覆い隠し、次の commit で消失していた
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pigeon.stronghold");
        let key = random_key();
        {
            let store = SecureStore::new(path.clone(), &key).unwrap();
            store.insert("k", b"v").unwrap();
        }
        let store = SecureStore::new(path, &key).unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some(b"v".as_ref()));
    }

    // --- open_with_migration ---

    fn random_key() -> Vec<u8> {
        resolve_master_key(&InMemoryBackend::empty())
            .unwrap()
            .to_vec()
    }

    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_open_with_migration_fresh_store() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pigeon.stronghold");
        let key = random_key();
        let (store, outcome) = SecureStore::open_with_migration(path, &key).unwrap();
        assert!(matches!(outcome, MasterKeyMigration::FreshStore));
        store.insert("k", b"v").unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some(b"v".as_ref()));
    }

    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_open_with_migration_reopens_with_current_key() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pigeon.stronghold");
        let key = random_key();
        {
            let (store, _) = SecureStore::open_with_migration(path.clone(), &key).unwrap();
            store.insert("k", b"v").unwrap();
        }
        let (store, outcome) = SecureStore::open_with_migration(path, &key).unwrap();
        assert!(matches!(outcome, MasterKeyMigration::AlreadyCurrent));
        assert_eq!(store.get("k").unwrap().as_deref(), Some(b"v".as_ref()));
    }

    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_open_with_migration_migrates_legacy_snapshot() {
        // 旧固定鍵で作られた既存スナップショットは、新鍵で再暗号化して
        // 中身を保持したまま開ける（ユーザーの再認証を避ける）
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pigeon.stronghold");
        {
            let legacy = SecureStore::new(path.clone(), &legacy_fixed_key()).unwrap();
            legacy.insert("oauth_acc1", b"token").unwrap();
        }
        let key = random_key();
        let (store, outcome) = SecureStore::open_with_migration(path.clone(), &key).unwrap();
        assert!(matches!(outcome, MasterKeyMigration::MigratedFromLegacy));
        assert_eq!(
            store.get("oauth_acc1").unwrap().as_deref(),
            Some(b"token".as_ref()),
            "移行後も既存の秘密を読める"
        );
        drop(store);

        // 再暗号化済みなので、次回は新鍵でそのまま開ける
        let (store, outcome) = SecureStore::open_with_migration(path.clone(), &key).unwrap();
        assert!(matches!(outcome, MasterKeyMigration::AlreadyCurrent));
        assert_eq!(
            store.get("oauth_acc1").unwrap().as_deref(),
            Some(b"token".as_ref())
        );
        drop(store);

        // 旧固定鍵ではもう開けない（再暗号化の完了確認）
        assert!(SecureStore::new(path, &legacy_fixed_key()).is_err());
    }

    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_open_with_migration_unreadable_snapshot_backed_up() {
        // 別デバイスの鍵で作られた（=どの手持ち鍵でも開けない）スナップショットは
        // 上書き破壊せず退避し、新規ストアで起動する（要再認証）
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pigeon.stronghold");
        let key_a = random_key();
        {
            let (store, _) = SecureStore::open_with_migration(path.clone(), &key_a).unwrap();
            store.insert("k", b"v").unwrap();
        }
        let key_b = random_key();
        let (store, outcome) = SecureStore::open_with_migration(path.clone(), &key_b).unwrap();
        match outcome {
            MasterKeyMigration::UnreadableBackedUp { backup } => {
                assert!(backup.exists(), "元スナップショットは退避されて残る");
            }
            other => panic!("expected UnreadableBackedUp, got {other:?}"),
        }
        assert!(store.get("k").unwrap().is_none(), "新規ストアは空");
    }

    // --- 遅延コミットの永続化（実 Stronghold でしか検証できない） ---
    //
    // PendingCommit のロジックは上のユニットテストで実 I/O 抜きに検証済みだが、
    // 「遅延させた値が本当に再オープン後も読めるか」「Critical な書き込みが
    // 遅延分を巻き込んで永続化するか」はスナップショットへの往復が要る。

    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_deferrable_write_is_readable_before_flush() {
        // 遅延中でもメモリ上の状態は更新済みなので get() は最新値を返す
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pigeon.stronghold");
        let key = random_key();
        let store = SecureStore::new(path, &key).unwrap();
        let cache_key = format!("{ACCESS_TOKEN_CACHE_PREFIX}acc1");
        store.insert(&cache_key, b"at").unwrap();
        assert_eq!(
            store.get(&cache_key).unwrap().as_deref(),
            Some(b"at".as_ref()),
            "コミット前でも読み出せる"
        );
    }

    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_flush_persists_deferred_write() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pigeon.stronghold");
        let key = random_key();
        let cache_key = format!("{ACCESS_TOKEN_CACHE_PREFIX}acc1");
        {
            let store = SecureStore::new(path.clone(), &key).unwrap();
            store.insert(&cache_key, b"at").unwrap();
            store.flush().unwrap();
        }
        let store = SecureStore::new(path, &key).unwrap();
        assert_eq!(
            store.get(&cache_key).unwrap().as_deref(),
            Some(b"at".as_ref()),
            "flush 後は再オープンしても残る"
        );
    }

    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_critical_write_persists_pending_deferrable_write_too() {
        // Critical の書き込みは、それまでに溜まった Deferrable も
        // 同じ 1 回のコミットで永続化する
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pigeon.stronghold");
        let key = random_key();
        let cache_key = format!("{ACCESS_TOKEN_CACHE_PREFIX}acc1");
        {
            let store = SecureStore::new(path.clone(), &key).unwrap();
            store.insert(&cache_key, b"at").unwrap();
            // Critical（再取得不能な秘密）の書き込み
            store.insert("oauth_acc1", b"token").unwrap();
        }
        let store = SecureStore::new(path, &key).unwrap();
        assert_eq!(
            store.get("oauth_acc1").unwrap().as_deref(),
            Some(b"token".as_ref())
        );
        assert_eq!(
            store.get(&cache_key).unwrap().as_deref(),
            Some(b"at".as_ref()),
            "遅延分も Critical のコミットに巻き込まれて永続化される"
        );
    }

    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_critical_write_persists_without_explicit_flush() {
        // 再取得不能な秘密は flush を呼ばなくても失われない
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pigeon.stronghold");
        let key = random_key();
        {
            let store = SecureStore::new(path.clone(), &key).unwrap();
            store.insert("password_acc1", b"pw").unwrap();
        }
        let store = SecureStore::new(path, &key).unwrap();
        assert_eq!(
            store.get("password_acc1").unwrap().as_deref(),
            Some(b"pw".as_ref())
        );
    }

    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_delete_of_critical_key_is_persisted_immediately() {
        // 削除した秘密がスナップショットに残り続けないこと
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pigeon.stronghold");
        let key = random_key();
        {
            let store = SecureStore::new(path.clone(), &key).unwrap();
            store.insert("oauth_acc1", b"token").unwrap();
            store.delete("oauth_acc1").unwrap();
        }
        let store = SecureStore::new(path, &key).unwrap();
        assert_eq!(
            store.get("oauth_acc1").unwrap(),
            None,
            "削除は即座に永続化される"
        );
    }

    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_stronghold_snapshots_not_cross_decryptable() {
        // 受け入れ基準: 別鍵（=別デバイス）で生成したスナップショットは相互に復号できない
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pigeon.stronghold");
        let key_a = random_key();
        let key_b = random_key();
        {
            SecureStore::new(path.clone(), &key_a)
                .unwrap()
                .insert("k", b"v")
                .unwrap();
        }
        assert!(SecureStore::new(path, &key_b).is_err());
    }
}
