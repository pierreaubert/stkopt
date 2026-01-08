//! Era-related chain queries.

use crate::ChainClient;
use crate::error::ChainError;
use stkopt_core::EraInfo;
use subxt::dynamic::{At, DecodedValueThunk, Value};

impl ChainClient {
    /// Get the active era information.
    pub async fn get_active_era(&self) -> Result<Option<EraInfo>, ChainError> {
        let storage_query = subxt::dynamic::storage("Staking", "ActiveEra", ());

        let storage = self.client().storage().at_latest().await?;
        tracing::debug!("Fetching ActiveEra storage...");

        let result: Option<DecodedValueThunk> = storage.fetch(&storage_query).await?;

        let Some(value) = result else {
            tracing::debug!("ActiveEra storage returned None");
            return Ok(None);
        };

        tracing::debug!("ActiveEra storage returned value");

        let decoded = value.to_value()?;

        // ActiveEra is Option<ActiveEraInfo> where ActiveEraInfo = { index: u32, start: Option<u64> }
        let index = decoded
            .at("index")
            .ok_or_else(|| ChainError::InvalidData("Missing era index".into()))?
            .as_u128()
            .ok_or_else(|| ChainError::InvalidData("Invalid era index".into()))?
            as u32;

        let start_timestamp_ms = decoded
            .at("start")
            .and_then(|v: &Value<u32>| v.as_u128())
            .unwrap_or(0) as u64;

        let era_duration_ms = self.get_era_duration_ms().await?;
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
    /// We use network-specific known era durations:
    /// - Polkadot/Kusama: 24 hours (6 sessions × 4 hours)
    /// - Testnets may vary
    ///
    /// Falls back to MaxEraDuration constant if available.
    pub async fn get_era_duration_ms(&self) -> Result<u64, ChainError> {
        // First try to get MaxEraDuration from Staking constants (Asset Hub has this)
        if let Ok(max_era) = self.get_constant_u64("Staking", "MaxEraDuration").await {
            // MaxEraDuration is 36 hours on Asset Hub, but actual eras are 24 hours
            // Use a more conservative estimate based on typical era duration
            // Polkadot: 6 sessions × 2400 blocks × 6s = 86400s = 24 hours
            let polkadot_era_ms: u64 = 24 * 60 * 60 * 1000; // 24 hours
            return Ok(polkadot_era_ms.min(max_era));
        }

        // Try legacy relay chain approach (Babe-based)
        if let (Ok(sessions_per_era), Ok(epoch_duration), Ok(expected_block_time)) = (
            self.get_constant_u32("Staking", "SessionsPerEra").await,
            self.get_constant_u64("Babe", "EpochDuration").await,
            self.get_constant_u64("Babe", "ExpectedBlockTime").await,
        ) {
            return Ok(sessions_per_era as u64 * epoch_duration * expected_block_time);
        }

        // Default fallback: 24 hours (standard Polkadot era)
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
        let constant = subxt::dynamic::constant(pallet, name);
        let value = self.client().constants().at(&constant)?;
        let decoded = value.to_value()?;
        decoded.as_u128().map(|v| v as u32).ok_or_else(|| {
            ChainError::InvalidData(format!("Invalid constant {}::{}", pallet, name))
        })
    }

    /// Helper to get a u64 constant from runtime.
    async fn get_constant_u64(&self, pallet: &str, name: &str) -> Result<u64, ChainError> {
        let constant = subxt::dynamic::constant(pallet, name);
        let value = self.client().constants().at(&constant)?;
        let decoded = value.to_value()?;
        decoded.as_u128().map(|v| v as u64).ok_or_else(|| {
            ChainError::InvalidData(format!("Invalid constant {}::{}", pallet, name))
        })
    }
}
