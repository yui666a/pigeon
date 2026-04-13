use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::AppError;

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
        let keyprovider = iota_stronghold::KeyProvider::try_from(
            zeroize::Zeroizing::new(password.to_vec())
        ).map_err(|e| AppError::Stronghold(format!("Key derivation failed: {}", e)))?;

        // Load existing snapshot if it exists
        if path.exists() {
            stronghold
                .load_snapshot(&keyprovider, &snapshot_path)
                .map_err(|e| AppError::Stronghold(format!("Failed to load snapshot: {}", e)))?;
        }

        let client_path = b"pigeon".to_vec();

        // Create or load the client
        let _client = stronghold
            .create_client(&client_path)
            .or_else(|_| stronghold.load_client(&client_path))
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

    pub fn insert(&self, key: &str, value: &[u8]) -> Result<(), AppError> {
        let inner = self.inner.lock().map_err(|e| AppError::Stronghold(e.to_string()))?;
        let client = inner.stronghold
            .get_client(&inner.client_path)
            .map_err(|e| AppError::Stronghold(format!("Failed to get client: {}", e)))?;
        let store = client.store();
        store
            .insert(key.as_bytes().to_vec(), value.to_vec(), None)
            .map_err(|e| AppError::Stronghold(format!("Failed to insert: {}", e)))?;
        inner.stronghold
            .commit_with_keyprovider(&inner.snapshot_path, &inner.keyprovider)
            .map_err(|e| AppError::Stronghold(format!("Failed to save: {}", e)))?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, AppError> {
        let inner = self.inner.lock().map_err(|e| AppError::Stronghold(e.to_string()))?;
        let client = inner.stronghold
            .get_client(&inner.client_path)
            .map_err(|e| AppError::Stronghold(format!("Failed to get client: {}", e)))?;
        let store = client.store();
        store
            .get(key.as_bytes())
            .map_err(|e| AppError::Stronghold(format!("Failed to get: {}", e)))
    }

    pub fn delete(&self, key: &str) -> Result<(), AppError> {
        let inner = self.inner.lock().map_err(|e| AppError::Stronghold(e.to_string()))?;
        let client = inner.stronghold
            .get_client(&inner.client_path)
            .map_err(|e| AppError::Stronghold(format!("Failed to get client: {}", e)))?;
        let store = client.store();
        let _ = store.delete(key.as_bytes());
        inner.stronghold
            .commit_with_keyprovider(&inner.snapshot_path, &inner.keyprovider)
            .map_err(|e| AppError::Stronghold(format!("Failed to save: {}", e)))?;
        Ok(())
    }
}
