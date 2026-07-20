use crate::bootstrap;
use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::cli::lock::ProcessLock;
use crate::context::Ctx;
use crate::error::AppError;
use crate::secure_store::SecureStore;
use crate::state::{DbState, SyncLocks};
use crate::usecase::{cases, Driver, Registry};
use std::sync::Mutex;

/// CLI / MCP プロセスが持つ実行環境。GUI の Tauri State 群に相当する。
pub struct CliRuntime {
    db: DbState,
    secure_store: SecureStore,
    pending: PendingClassifications,
    batches: ClassifyBatches,
    sync_locks: SyncLocks,
    registry: Registry,
    driver: Driver,
    /// GUI との同時実行を防ぐ排他ロック。CliRuntime が drop されるまで
    /// 保持し続ける必要があるためフィールドとして持つ。構造体のフィールドは
    /// 宣言順に drop されるので、DB と SecureStore を閉じた後に解放される。
    _lock: ProcessLock,
}

impl CliRuntime {
    /// DB と SecureStore を開き、UseCase レジストリを構築する。
    /// GUI が起動中で排他できない場合はエラーを返す。
    pub fn open(driver: Driver) -> Result<Self, AppError> {
        crate::env_config::load_dotenv();

        let data_dir = bootstrap::resolve_data_dir()?;

        // DB / Stronghold を開く前に排他する。Stronghold は自前で排他ロックを
        // 取らず、後から commit した側が先の書き込みを消してしまうため。
        let lock = ProcessLock::acquire(&data_dir)?;

        let conn = bootstrap::open_db(&data_dir)?;
        let (secure_store, migration) = bootstrap::open_secure_store(&data_dir)?;
        bootstrap::report_master_key_migration(&migration);

        let registry = {
            let mut reg = Registry::new();
            cases::register_all(&mut reg);
            reg
        };

        Ok(Self {
            db: DbState(Mutex::new(conn)),
            secure_store,
            pending: PendingClassifications::new(),
            batches: ClassifyBatches::new(),
            sync_locks: SyncLocks::new(),
            registry,
            driver,
            _lock: lock,
        })
    }

    pub fn ctx(&self) -> Ctx<'_> {
        Ctx::new_headless(
            &self.db,
            &self.secure_store,
            &self.pending,
            &self.batches,
            &self.sync_locks,
            self.driver,
        )
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn driver(&self) -> Driver {
        self.driver
    }
}
