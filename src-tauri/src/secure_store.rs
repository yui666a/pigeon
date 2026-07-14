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

/// OS キーチェーン保管（macOS Keychain / Windows Credential Manager）。
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct KeychainBackend {
    service: String,
    account: String,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
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
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

/// Simple secure key-value store backed by the filesystem.
/// Tokens and passwords are stored as JSON in an encrypted file using iota_stronghold.
/// For now, we use a simpler approach: an in-memory HashMap persisted to an encrypted JSON file
/// via the Stronghold store API.
///
/// The StrongholdCollection managed by tauri-plugin-stronghold is designed for JS-to-Rust comms.
/// We use our own wrapper for Rust-side operations.
pub struct SecureStore {
    inner: Mutex<SecureStoreInner>,
}

struct SecureStoreInner {
    stronghold: iota_stronghold::Stronghold,
    snapshot_path: iota_stronghold::SnapshotPath,
    keyprovider: iota_stronghold::KeyProvider,
    client_path: Vec<u8>,
}

impl SecureStore {
    pub fn new(path: PathBuf, password: &[u8]) -> Result<Self, AppError> {
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
        })
    }

    /// スナップショットを現行鍵で開く。開けない場合は旧固定鍵からの移行を試み、
    /// それも不能なら退避して新規作成する（秘密は失われるが起動は継続できる）。
    pub fn open_with_migration(
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

    pub fn insert(&self, key: &str, value: &[u8]) -> Result<(), AppError> {
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
        inner
            .stronghold
            .commit_with_keyprovider(&inner.snapshot_path, &inner.keyprovider)
            .map_err(|e| AppError::Stronghold(format!("Failed to save: {}", e)))?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, AppError> {
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

    pub fn delete(&self, key: &str) -> Result<(), AppError> {
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
        inner
            .stronghold
            .commit_with_keyprovider(&inner.snapshot_path, &inner.keyprovider)
            .map_err(|e| AppError::Stronghold(format!("Failed to save: {}", e)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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

    #[test]
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
