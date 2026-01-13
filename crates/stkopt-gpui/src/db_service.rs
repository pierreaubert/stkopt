use crate::app::{HistoryPoint, PoolInfo, ValidatorInfo};
use crate::persistence::get_data_dir;
use stkopt_core::db::{CachedAccountStatus, CachedChainMetadata, StakingDb};
use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};
use stkopt_core::Network;

/// Service for asynchronous database access.
#[derive(Clone)]
pub struct DbService {
    db: Arc<Mutex<StakingDb>>,
    handle: tokio::runtime::Handle,
}

impl DbService {
    /// Initialize the database service.
    pub fn new(handle: tokio::runtime::Handle) -> Result<Self> {
        let data_dir = get_data_dir().map_err(|e| anyhow::anyhow!(e))?;
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir).context("Failed to create data directory")?;
        }
        let db_path = data_dir.join("history.db");
        let db = StakingDb::open(&db_path).context("Failed to open database")?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            handle,
        })
    }

    /// Open an in-memory database for testing.
    pub fn new_memory(handle: tokio::runtime::Handle) -> Result<Self> {
        let db = StakingDb::open_memory().context("Failed to open in-memory database")?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            handle,
        })
    }

    /// Get staking history for an address.
    pub async fn get_history(
        &self,
        network: Network,
        address: String,
        limit: Option<u32>,
    ) -> Result<Vec<HistoryPoint>> {
        let db = self.db.clone();
        self.handle.spawn_blocking(move || {
            let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
            db.get_history(network, &address, limit)
                .context("Failed to get history")
        })
        .await?
    }

    /// Insert staking history points.
    pub async fn insert_history_batch(
        &self,
        network: Network,
        address: String,
        points: Vec<HistoryPoint>,
    ) -> Result<()> {
        let db = self.db.clone();
        self.handle.spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
            db.insert_history_batch(network, &address, &points)
                .context("Failed to insert history")
        })
        .await?
    }

    /// Get cached validators.
    pub async fn get_cached_validators(&self, network: Network) -> Result<Vec<ValidatorInfo>> {
        let db = self.db.clone();
        self.handle.spawn_blocking(move || {
            let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
            db.get_cached_validators(network)
                .context("Failed to get cached validators")
        })
        .await?
    }

    /// Set cached validators.
    pub async fn set_cached_validators(
        &self,
        network: Network,
        era: u32,
        validators: Vec<ValidatorInfo>,
    ) -> Result<usize> {
        let db = self.db.clone();
        self.handle.spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
            db.set_cached_validators(network, era, &validators)
                .context("Failed to set cached validators")
        })
        .await?
    }

    /// Get cached pools.
    pub async fn get_cached_pools(&self, network: Network) -> Result<Vec<PoolInfo>> {
        let db = self.db.clone();
        self.handle.spawn_blocking(move || {
            let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
            db.get_cached_pools(network)
                .context("Failed to get cached pools")
        })
        .await?
    }

    /// Set cached pools.
    pub async fn set_cached_pools(
        &self,
        network: Network,
        pools: Vec<PoolInfo>,
    ) -> Result<usize> {
        let db = self.db.clone();
        self.handle.spawn_blocking(move || {
            let mut db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
            db.set_cached_pools(network, &pools)
                .context("Failed to set cached pools")
        })
        .await?
    }

    /// Get chain metadata.
    pub async fn get_chain_metadata(
        &self,
        network: Network,
    ) -> Result<Option<CachedChainMetadata>> {
        let db = self.db.clone();
        self.handle.spawn_blocking(move || {
            let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
            db.get_chain_metadata(network)
                .context("Failed to get chain metadata")
        })
        .await?
    }

    /// Set chain metadata.
    pub async fn set_chain_metadata(
        &self,
        network: Network,
        meta: CachedChainMetadata,
    ) -> Result<()> {
        let db = self.db.clone();
        self.handle.spawn_blocking(move || {
            let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
            db.set_chain_metadata(network, &meta)
                .context("Failed to set chain metadata")
        })
        .await?
    }

    /// Get cached account status.
    pub async fn get_cached_account_status(
        &self,
        network: Network,
        address: String,
    ) -> Result<Option<CachedAccountStatus>> {
        let db = self.db.clone();
        self.handle.spawn_blocking(move || {
            let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
            db.get_cached_account_status(network, &address)
                .context("Failed to get cached account status")
        })
        .await?
    }

    /// Set cached account status.
    pub async fn set_cached_account_status(
        &self,
        network: Network,
        address: String,
        status: CachedAccountStatus,
    ) -> Result<()> {
        let db = self.db.clone();
        self.handle.spawn_blocking(move || {
            let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
            db.set_cached_account_status(network, &address, &status)
                .context("Failed to set cached account status")
        })
        .await?
    }
}
