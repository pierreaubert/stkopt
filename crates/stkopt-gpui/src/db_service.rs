use crate::app::{HistoryPoint, PoolInfo, ValidatorInfo};
use crate::persistence::get_data_dir;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use stkopt_core::Network;
use stkopt_core::db::{
    AccountStatusService, CachePolicy, CachedAccountStatus, CachedChainMetadata, HistoryService,
    StakingDb, StartupDataCache, StartupDataService,
};

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
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_history(network, &address, limit)
                    .context("Failed to get history")
            })
            .await?
    }

    /// Get staking history for an inclusive era range.
    pub async fn get_history_range(
        &self,
        network: Network,
        address: String,
        from_era: u32,
        to_era: u32,
    ) -> Result<Vec<HistoryPoint>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_history_range(network, &address, from_era, to_era)
                    .context("Failed to get history range")
            })
            .await?
    }

    /// Get eras that are missing or whose cached APY is above the accepted maximum.
    pub async fn get_missing_eras_with_max_apy(
        &self,
        network: Network,
        address: String,
        from_era: u32,
        to_era: u32,
        max_apy: f64,
    ) -> Result<Vec<u32>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_missing_eras_with_max_apy(network, &address, from_era, to_era, max_apy)
                    .context("Failed to get missing eras")
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
        self.handle
            .spawn_blocking(move || {
                let mut db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.insert_history_batch(network, &address, &points)
                    .context("Failed to insert history")
            })
            .await?
    }

    /// Get cached validators.
    pub async fn get_cached_validators(&self, network: Network) -> Result<Vec<ValidatorInfo>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_cached_validators(network)
                    .context("Failed to get cached validators")
            })
            .await?
    }

    /// Get recent cached validators before current-era metadata is known.
    pub async fn get_recent_cached_validators(
        &self,
        network: Network,
    ) -> Result<Vec<ValidatorInfo>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_recent_cached_validators(
                    network,
                    CachePolicy::default().startup_max_age_secs,
                )
                .context("Failed to get recent cached validators")
            })
            .await?
    }

    /// Get cached validators only if fresh for the current era.
    pub async fn get_fresh_cached_validators(
        &self,
        network: Network,
        current_era: u32,
    ) -> Result<Vec<ValidatorInfo>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_fresh_cached_validators(
                    network,
                    current_era,
                    CachePolicy::default().startup_max_age_secs,
                    CachePolicy::default().startup_max_era_lag,
                )
                .context("Failed to get fresh cached validators")
            })
            .await?
    }

    /// Load startup cache data plus refresh decisions under the shared policy.
    pub async fn get_startup_cache(
        &self,
        network: Network,
        current_era: u32,
    ) -> Result<StartupDataCache> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                StartupDataService::load(&db, network, current_era, CachePolicy::default())
                    .context("Failed to load startup cache")
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
        self.handle
            .spawn_blocking(move || {
                let mut db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.set_cached_validators(network, era, &validators)
                    .context("Failed to set cached validators")
            })
            .await?
    }

    /// Set cached validators with completeness metadata.
    pub async fn set_cached_validators_checked(
        &self,
        network: Network,
        era: u32,
        validators: Vec<ValidatorInfo>,
        complete: bool,
    ) -> Result<usize> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let mut db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.set_cached_validators_checked(network, era, &validators, complete)
                    .context("Failed to set cached validators")
            })
            .await?
    }

    /// Get cached validator identities.
    pub async fn get_validator_identities(
        &self,
        network: Network,
    ) -> Result<HashMap<String, String>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_validator_identities(network)
                    .context("Failed to get cached validator identities")
            })
            .await?
    }

    /// Get cached validator identities that are at most `max_age_secs` old.
    pub async fn get_validator_identities_within_age(
        &self,
        network: Network,
        max_age_secs: i64,
    ) -> Result<HashMap<String, String>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_validator_identities_within_age(network, max_age_secs)
                    .context("Failed to get cached validator identities within age")
            })
            .await?
    }

    /// Store cached validator identities.
    pub async fn set_validator_identities_batch(
        &self,
        network: Network,
        identities: HashMap<String, String>,
    ) -> Result<usize> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let mut db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.set_validator_identities_batch(network, &identities)
                    .context("Failed to set cached validator identities")
            })
            .await?
    }

    /// Get cached pools.
    pub async fn get_cached_pools(&self, network: Network) -> Result<Vec<PoolInfo>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_cached_pools(network)
                    .context("Failed to get cached pools")
            })
            .await?
    }

    /// Get recent cached pools before current-era metadata is known.
    pub async fn get_recent_cached_pools(&self, network: Network) -> Result<Vec<PoolInfo>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_recent_cached_pools(network, CachePolicy::default().startup_max_age_secs)
                    .context("Failed to get recent cached pools")
            })
            .await?
    }

    /// Get cached pools only if fresh for the current era.
    pub async fn get_fresh_cached_pools(
        &self,
        network: Network,
        current_era: u32,
    ) -> Result<Vec<PoolInfo>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_fresh_cached_pools(
                    network,
                    current_era,
                    CachePolicy::default().startup_max_age_secs,
                    CachePolicy::default().startup_max_era_lag,
                )
                .context("Failed to get fresh cached pools")
            })
            .await?
    }

    /// Set cached pools.
    pub async fn set_cached_pools(&self, network: Network, pools: Vec<PoolInfo>) -> Result<usize> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let mut db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.set_cached_pools(network, &pools)
                    .context("Failed to set cached pools")
            })
            .await?
    }

    /// Set cached pools with the current era snapshot.
    pub async fn set_cached_pools_at_era(
        &self,
        network: Network,
        era: u32,
        pools: Vec<PoolInfo>,
    ) -> Result<usize> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let mut db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.set_cached_pools_at_era(network, era, &pools)
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
        self.handle
            .spawn_blocking(move || {
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
        self.handle
            .spawn_blocking(move || {
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
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.get_cached_account_status(network, &address)
                    .context("Failed to get cached account status")
            })
            .await?
    }

    /// Get cached account status if it is recent enough for read-through use.
    pub async fn get_recent_cached_account_status(
        &self,
        network: Network,
        address: String,
    ) -> Result<Option<CachedAccountStatus>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                AccountStatusService::load_cached(&db, network, &address, CachePolicy::default())
                    .context("Failed to get recent cached account status")
            })
            .await?
    }

    /// Load cached history for a range under the shared history policy.
    #[allow(clippy::too_many_arguments)]
    pub async fn get_history_cache_range(
        &self,
        network: Network,
        address: String,
        start_era: u32,
        end_era: u32,
        current_era: u32,
        current_era_start_ms: u64,
        era_duration_ms: u64,
    ) -> Result<Vec<HistoryPoint>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                HistoryService::load_cached_range(
                    &db,
                    network,
                    &address,
                    start_era,
                    end_era,
                    current_era,
                    current_era_start_ms,
                    era_duration_ms,
                    CachePolicy::default(),
                )
                .context("Failed to load cached history range")
            })
            .await?
    }

    /// Get missing history eras under the shared history policy.
    pub async fn get_missing_history_eras(
        &self,
        network: Network,
        address: String,
        start_era: u32,
        end_era: u32,
    ) -> Result<Vec<u32>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                HistoryService::missing_eras(
                    &db,
                    network,
                    &address,
                    start_era,
                    end_era,
                    CachePolicy::default(),
                )
                .context("Failed to get missing history eras")
            })
            .await?
    }

    /// Load latest cached history fallback under the shared history policy.
    pub async fn get_latest_history_cache(
        &self,
        network: Network,
        address: String,
        limit: Option<u32>,
    ) -> Result<Vec<HistoryPoint>> {
        let db = self.db.clone();
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                HistoryService::load_latest(&db, network, &address, limit, CachePolicy::default())
                    .context("Failed to load latest cached history")
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
        self.handle
            .spawn_blocking(move || {
                let db = db.lock().map_err(|_| anyhow::anyhow!("Db lock poisoned"))?;
                db.set_cached_account_status(network, &address, &status)
                    .context("Failed to set cached account status")
            })
            .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_point(era: u32, apy: f64) -> HistoryPoint {
        HistoryPoint {
            era,
            date: Some(format!("202501{:02}", era % 100)),
            reward: 10,
            bonded: 1_000,
            apy,
        }
    }

    #[test]
    fn test_history_range_and_missing_filter() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let service = DbService::new_memory(runtime.handle().clone()).unwrap();

        runtime.block_on(async {
            service
                .insert_history_batch(
                    Network::Polkadot,
                    "addr1".to_string(),
                    vec![
                        history_point(1500, 0.12),
                        history_point(1501, 0.95),
                        history_point(1502, 0.10),
                    ],
                )
                .await
                .unwrap();

            let history = service
                .get_history_range(Network::Polkadot, "addr1".to_string(), 1501, 1502)
                .await
                .unwrap();
            assert_eq!(
                history.iter().map(|point| point.era).collect::<Vec<_>>(),
                vec![1501, 1502]
            );

            let missing = service
                .get_missing_eras_with_max_apy(
                    Network::Polkadot,
                    "addr1".to_string(),
                    1500,
                    1503,
                    0.50,
                )
                .await
                .unwrap();
            assert_eq!(missing, vec![1501, 1503]);
        });
    }
}
