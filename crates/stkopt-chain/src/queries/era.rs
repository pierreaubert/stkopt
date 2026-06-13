//! Era-related chain queries.

use crate::ChainClient;
use crate::error::ChainError;
use stkopt_core::EraInfo;
use subxt::dynamic::{At, Value};

impl ChainClient {
    /// Get the active era information.
    pub async fn get_active_era(&self) -> Result<Option<EraInfo>, ChainError> {
        let storage_query = subxt::dynamic::storage::<Vec<Value>, Value>("Staking", "ActiveEra");

        tracing::debug!("Fetching ActiveEra storage...");

        let block = self.client().at_current_block().await?;
        let result = block.storage().try_fetch(&storage_query, vec![]).await?;

        let Some(value) = result else {
            tracing::debug!("ActiveEra storage returned None");
            return Ok(None);
        };

        tracing::debug!("ActiveEra storage returned value");

        let decoded: Value = value.decode()?;

        // ActiveEra is Option<ActiveEraInfo> where ActiveEraInfo = { index: u32, start: Option<u64> }
        let index = decoded
            .at("index")
            .ok_or_else(|| ChainError::InvalidData("Missing era index".into()))?
            .as_u128()
            .ok_or_else(|| ChainError::InvalidData("Invalid era index".into()))?
            as u32;

        let start_timestamp_ms = decoded
            .at("start")
            .and_then(|v: &Value| v.as_u128())
            .unwrap_or(0) as u64;

        let era_duration_ms = self.get_era_duration_ms().await?;
        // NOTE: SystemTime is used here because we need an absolute wall-clock
        // timestamp to compare with the on-chain era start time. This is an
        // approximation; local clock skew relative to the chain may affect accuracy.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| {
                ChainError::InvalidData(format!("System time is before Unix epoch: {}", e))
            })?
            .as_millis() as u64;

        let elapsed = now.saturating_sub(start_timestamp_ms);
        let pct_complete = if era_duration_ms > 0 {
            (elapsed as f64 / era_duration_ms as f64).min(1.0)
        } else {
            0.0
        };

        let estimated_end_ms = start_timestamp_ms + era_duration_ms;

        Ok(Some(EraInfo {
            index,
            start_timestamp_ms,
            duration_ms: era_duration_ms,
            pct_complete,
            estimated_end_ms,
        }))
    }

    /// Get era duration in milliseconds.
    ///
    /// Since the Polkadot 2.0 migration, Asset Hub doesn't have Babe constants.
    /// We first try the Staking `MaxEraDuration` constant. If that is not
    /// available we fall back to the legacy Babe-based computation, and only
    /// use a hard-coded 24-hour default as a last resort.
    ///
    /// The on-chain value is returned as-is; it is not clamped to a default.
    pub async fn get_era_duration_ms(&self) -> Result<u64, ChainError> {
        // First try to get MaxEraDuration from Staking constants (Asset Hub has this).
        if let Ok(max_era) = self.get_constant_u64("Staking", "MaxEraDuration").await {
            return Ok(max_era);
        }

        // Try legacy relay chain approach (Babe-based).
        if let (Ok(sessions_per_era), Ok(epoch_duration), Ok(expected_block_time)) = (
            self.get_constant_u32("Staking", "SessionsPerEra").await,
            self.get_constant_u64("Babe", "EpochDuration").await,
            self.get_constant_u64("Babe", "ExpectedBlockTime").await,
        ) {
            return Ok(sessions_per_era as u64 * epoch_duration * expected_block_time);
        }

        // Default fallback: 24 hours (standard Polkadot era).
        tracing::warn!("Could not determine era duration from chain, using 24 hour default");
        Ok(24 * 60 * 60 * 1000)
    }

    /// Get bonding duration in eras.
    pub async fn get_bonding_duration(&self) -> Result<u32, ChainError> {
        self.get_constant_u32("Staking", "BondingDuration").await
    }

    /// Get history depth (number of eras for which staking data is kept).
    pub async fn get_history_depth(&self) -> Result<u32, ChainError> {
        self.get_constant_u32("Staking", "HistoryDepth").await
    }

    /// Helper to get a u32 constant from runtime.
    async fn get_constant_u32(&self, pallet: &str, name: &str) -> Result<u32, ChainError> {
        let constant = (pallet, name);
        let block = self.client().at_current_block().await?;
        let decoded = block.constants().entry(&constant)?;
        decoded.as_u128().map(|v| v as u32).ok_or_else(|| {
            ChainError::InvalidData(format!("Invalid constant {}::{}", pallet, name))
        })
    }

    /// Helper to get a u64 constant from runtime.
    async fn get_constant_u64(&self, pallet: &str, name: &str) -> Result<u64, ChainError> {
        let constant = (pallet, name);
        let block = self.client().at_current_block().await?;
        let decoded = block.constants().entry(&constant)?;
        decoded.as_u128().map(|v| v as u64).ok_or_else(|| {
            ChainError::InvalidData(format!("Invalid constant {}::{}", pallet, name))
        })
    }
}
