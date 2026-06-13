//! Async orchestration helpers for validator and pool enrichment.
//!
//! These helpers combine chain queries with the display enrichment functions in
//! [`crate::display`] so that multiple front-ends share the same enrichment
//! pipeline.

use crate::{
    ChainClient, ChainError, PeopleChainClient, PoolInfo as ChainPoolInfo, PoolMetadata,
    PoolNominations, ValidatorApyData, ValidatorInfo,
};
use std::collections::HashMap;
use stkopt_core::display::DisplayPool;

use crate::display::{
    DEFAULT_VALIDATOR_APY_LOOKBACK_ERAS, DisplayValidatorEnrichment, enrich_display_pools,
    enrich_display_validators, missing_validator_identity_addresses,
    pool_ids_for_nomination_queries, pool_metadata_map, validator_identity_display_map,
};

/// Result of enriching validators, including the data needed by front-ends to
/// report progress and update identity caches.
#[derive(Debug, Clone)]
pub struct ValidatorEnrichmentOutcome {
    pub enrichment: DisplayValidatorEnrichment,
    pub fresh_identities: HashMap<String, String>,
    pub updated_identity_map: HashMap<String, String>,
    pub apy_data: Option<ValidatorApyData>,
}

/// Result of enriching pools, including intermediate lookups so callers can
/// send partial UI updates or log progress.
#[derive(Debug, Clone)]
pub struct PoolEnrichmentOutcome {
    pub pools: Vec<DisplayPool>,
    pub metadata_map: HashMap<u32, String>,
    pub nominations_map: HashMap<u32, PoolNominations>,
}

/// Async data source for validator enrichment.
#[allow(async_fn_in_trait)]
pub trait ValidatorEnrichmentSource {
    /// Fetch the most recent complete validator APY data.
    async fn fetch_validator_apy_data(
        &self,
        latest_completed_era: u32,
        max_lookback: u32,
    ) -> Result<Option<ValidatorApyData>, ChainError>;
}

impl ValidatorEnrichmentSource for ChainClient {
    async fn fetch_validator_apy_data(
        &self,
        latest_completed_era: u32,
        max_lookback: u32,
    ) -> Result<Option<ValidatorApyData>, ChainError> {
        self.get_recent_validator_apy_data(latest_completed_era, max_lookback)
            .await
    }
}

/// Async data source for pool enrichment.
#[allow(async_fn_in_trait)]
pub trait PoolEnrichmentSource {
    /// Fetch pool metadata (names) for all pools.
    async fn fetch_pool_metadata(&self) -> Result<Vec<PoolMetadata>, ChainError>;

    /// Fetch nominations for the given pool IDs.
    async fn fetch_pool_nominations(
        &self,
        pool_ids: &[u32],
    ) -> Result<Vec<PoolNominations>, ChainError>;
}

impl PoolEnrichmentSource for ChainClient {
    async fn fetch_pool_metadata(&self) -> Result<Vec<PoolMetadata>, ChainError> {
        self.get_pool_metadata().await
    }

    async fn fetch_pool_nominations(
        &self,
        pool_ids: &[u32],
    ) -> Result<Vec<PoolNominations>, ChainError> {
        self.get_pool_nominations_batch(pool_ids).await
    }
}

/// Fetch identities, APY data, and enrich validators.
///
/// Missing identities are fetched from the People chain when a client is
/// provided. APY and identity fetch errors are logged and treated as missing
/// data so that enrichment can still produce a best-effort result.
pub async fn fetch_and_enrich_validators<S: ValidatorEnrichmentSource>(
    client: &S,
    validators: &[ValidatorInfo],
    people: Option<&PeopleChainClient>,
    mut identity_map: HashMap<String, String>,
    fallback_apy_era: u32,
    era_duration_ms: u64,
) -> Result<ValidatorEnrichmentOutcome, ChainError> {
    let apy_data = match client
        .fetch_validator_apy_data(fallback_apy_era, DEFAULT_VALIDATOR_APY_LOOKBACK_ERAS)
        .await
    {
        Ok(data) => data,
        Err(e) => {
            tracing::warn!("Failed to fetch validator APY data: {}", e);
            None
        }
    };

    let fresh_identities = match people {
        Some(people_client) => {
            fetch_missing_identities(validators, people_client, &identity_map).await
        }
        None => HashMap::new(),
    };

    identity_map.extend(fresh_identities.iter().map(|(k, v)| (k.clone(), v.clone())));

    let enrichment = enrich_display_validators(
        validators,
        &identity_map,
        apy_data.as_ref(),
        fallback_apy_era,
        era_duration_ms,
    );

    Ok(ValidatorEnrichmentOutcome {
        enrichment,
        fresh_identities,
        updated_identity_map: identity_map,
        apy_data,
    })
}

async fn fetch_missing_identities(
    validators: &[ValidatorInfo],
    people: &PeopleChainClient,
    identity_map: &HashMap<String, String>,
) -> HashMap<String, String> {
    let missing = missing_validator_identity_addresses(validators, identity_map);
    if missing.is_empty() {
        return HashMap::new();
    }
    match people.get_identities(&missing).await {
        Ok(identities) => validator_identity_display_map(identities),
        Err(e) => {
            tracing::warn!("Failed to fetch identities from People chain: {}", e);
            HashMap::new()
        }
    }
}

/// Fetch metadata, nominations, and enrich pools.
///
/// If `prefetched_metadata` is provided, metadata is fetched from the chain
/// and the prefetched value is used instead. Fetch errors are logged and
/// treated as empty data so enrichment can still return best-effort pools.
pub async fn fetch_and_enrich_pools<S: PoolEnrichmentSource>(
    client: &S,
    pools: &[ChainPoolInfo],
    validator_apy_map: &HashMap<String, f64>,
    max_pools_to_query: usize,
    prefetched_metadata: Option<&HashMap<u32, String>>,
) -> Result<PoolEnrichmentOutcome, ChainError> {
    let metadata_map = match prefetched_metadata {
        Some(map) => map.clone(),
        None => match client.fetch_pool_metadata().await {
            Ok(metadata) => pool_metadata_map(&metadata),
            Err(e) => {
                tracing::warn!("Failed to fetch pool metadata: {}", e);
                HashMap::new()
            }
        },
    };

    let pool_ids = pool_ids_for_nomination_queries(pools, max_pools_to_query);
    let nominations_map: HashMap<u32, PoolNominations> =
        match client.fetch_pool_nominations(&pool_ids).await {
            Ok(nominations) => nominations.into_iter().map(|n| (n.pool_id, n)).collect(),
            Err(e) => {
                tracing::warn!("Failed to fetch pool nominations: {}", e);
                HashMap::new()
            }
        };

    let display_pools = enrich_display_pools(
        pools,
        &metadata_map,
        &nominations_map,
        validator_apy_map,
        max_pools_to_query,
    );

    Ok(PoolEnrichmentOutcome {
        pools: display_pools,
        metadata_map,
        nominations_map,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PoolRoles, PoolState, ValidatorExposure, ValidatorPoints};
    use stkopt_core::ValidatorPreferences;
    use subxt::utils::AccountId32;

    struct MockValidatorSource {
        apy_data: Option<ValidatorApyData>,
    }

    impl ValidatorEnrichmentSource for MockValidatorSource {
        async fn fetch_validator_apy_data(
            &self,
            _latest_completed_era: u32,
            _max_lookback: u32,
        ) -> Result<Option<ValidatorApyData>, ChainError> {
            Ok(self.apy_data.clone())
        }
    }

    fn validator(byte: u8, commission: f64) -> ValidatorInfo {
        ValidatorInfo {
            address: AccountId32::from([byte; 32]),
            preferences: ValidatorPreferences {
                commission,
                blocked: false,
            },
        }
    }

    #[tokio::test]
    async fn fetch_and_enrich_validators_uses_identity_map_and_apy_data() {
        let v = validator(1, 0.0);
        let exposure = ValidatorExposure {
            address: v.address.clone(),
            own: 1_000,
            total: 10_000,
            nominator_count: 1,
        };
        let points = ValidatorPoints {
            address: v.address.clone(),
            points: 100,
        };
        // Small era reward so the resulting APY stays within the realistic range.
        let apy_data = ValidatorApyData {
            era: 5,
            era_reward: 100,
            total_points: 1_000,
            points: vec![points],
            exposures: vec![exposure],
        };
        let source = MockValidatorSource {
            apy_data: Some(apy_data),
        };
        let mut identity_map = HashMap::new();
        identity_map.insert(v.address.to_string(), "Alice".to_string());

        let outcome = fetch_and_enrich_validators(
            &source,
            &[v.clone()],
            None,
            identity_map,
            5,
            24 * 60 * 60 * 1000,
        )
        .await
        .unwrap();

        assert_eq!(outcome.enrichment.validators.len(), 1);
        assert_eq!(
            outcome.enrichment.validators[0].name,
            Some("Alice".to_string())
        );
        assert!(outcome.enrichment.validators[0].apy.is_some());
        assert_eq!(outcome.updated_identity_map.len(), 1);
        assert!(outcome.fresh_identities.is_empty());
    }

    #[tokio::test]
    async fn fetch_and_enrich_validators_falls_back_when_apy_source_fails() {
        struct FailingSource;

        impl ValidatorEnrichmentSource for FailingSource {
            async fn fetch_validator_apy_data(
                &self,
                _era: u32,
                _lookback: u32,
            ) -> Result<Option<ValidatorApyData>, ChainError> {
                Err(ChainError::Storage("network down".to_string()))
            }
        }

        let v = validator(2, 0.0);
        let outcome = fetch_and_enrich_validators(
            &FailingSource,
            &[v.clone()],
            None,
            HashMap::new(),
            3,
            24 * 60 * 60 * 1000,
        )
        .await
        .unwrap();

        assert_eq!(outcome.enrichment.validators.len(), 1);
        assert_eq!(
            outcome.enrichment.validators[0].address,
            v.address.to_string()
        );
        assert!(outcome.enrichment.validators[0].apy.is_none());
    }

    struct MockPoolSource {
        metadata: Vec<PoolMetadata>,
        nominations: Vec<PoolNominations>,
    }

    impl PoolEnrichmentSource for MockPoolSource {
        async fn fetch_pool_metadata(&self) -> Result<Vec<PoolMetadata>, ChainError> {
            Ok(self.metadata.clone())
        }

        async fn fetch_pool_nominations(
            &self,
            _pool_ids: &[u32],
        ) -> Result<Vec<PoolNominations>, ChainError> {
            Ok(self.nominations.clone())
        }
    }

    fn pool(id: u32, state: PoolState, points: u128, members: u32) -> ChainPoolInfo {
        ChainPoolInfo {
            id,
            state,
            points,
            member_count: members,
            roles: PoolRoles {
                depositor: AccountId32::from([0; 32]),
                root: None,
                nominator: None,
                bouncer: None,
            },
            commission: None,
        }
    }

    #[tokio::test]
    async fn fetch_and_enrich_pools_computes_apy_from_source() {
        let target = AccountId32::from([7; 32]);
        let pools = vec![pool(1, PoolState::Open, 1_000, 10)];
        let metadata = vec![PoolMetadata {
            id: 1,
            name: "One".to_string(),
        }];
        let nominations = vec![PoolNominations {
            pool_id: 1,
            stash: AccountId32::from([8; 32]),
            targets: vec![target.clone()],
        }];
        let source = MockPoolSource {
            metadata,
            nominations,
        };
        let validator_apys = HashMap::from([(target.to_string(), 0.12)]);

        let outcome = fetch_and_enrich_pools(&source, &pools, &validator_apys, 10, None)
            .await
            .unwrap();

        assert_eq!(outcome.pools.len(), 1);
        assert_eq!(outcome.pools[0].name, "One");
        assert!((outcome.pools[0].apy.unwrap() - 0.12).abs() < 1e-9);
        assert_eq!(outcome.metadata_map.get(&1), Some(&"One".to_string()));
        assert_eq!(outcome.nominations_map.len(), 1);
    }

    #[tokio::test]
    async fn fetch_and_enrich_pools_uses_prefetched_metadata() {
        let pools = vec![pool(2, PoolState::Open, 500, 5)];
        let prefetched = HashMap::from([(2, "Prefetched".to_string())]);
        let source = MockPoolSource {
            metadata: vec![],
            nominations: vec![],
        };

        let outcome =
            fetch_and_enrich_pools(&source, &pools, &HashMap::new(), 10, Some(&prefetched))
                .await
                .unwrap();

        assert_eq!(outcome.pools[0].name, "Prefetched");
        assert!(source.metadata.is_empty());
    }
}
