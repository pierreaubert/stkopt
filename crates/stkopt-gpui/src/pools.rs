//! Pool utilities for sorting, filtering, and caching.

use std::sync::Arc;

use crate::actions::PoolSortColumn;
use crate::app::PoolInfo;

/// Filter pools by search query (matches name or ID).
pub fn filter_pools<'a>(pools: &'a [PoolInfo], query: &str) -> Vec<(usize, &'a PoolInfo)> {
    let query = query.to_lowercase();
    pools
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            if query.is_empty() {
                return true;
            }
            p.name.to_lowercase().contains(&query) || p.id.to_string().contains(&query)
        })
        .collect()
}

/// Sort pools by the specified column.
pub fn sort_pools(pools: &mut [(usize, PoolInfo)], column: PoolSortColumn, ascending: bool) {
    pools.sort_by(|a, b| {
        let cmp = match column {
            PoolSortColumn::Id => a.1.id.cmp(&b.1.id),
            PoolSortColumn::Name => a.1.name.cmp(&b.1.name),
            PoolSortColumn::State => format!("{:?}", a.1.state).cmp(&format!("{:?}", b.1.state)),
            PoolSortColumn::Members => a.1.member_count.cmp(&b.1.member_count),
            PoolSortColumn::TotalBonded => a.1.total_bonded.cmp(&b.1.total_bonded),
            PoolSortColumn::Apy => {
                let a_apy = a.1.apy.unwrap_or(0.0);
                let b_apy = b.1.apy.unwrap_or(0.0);
                a_apy
                    .partial_cmp(&b_apy)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        };
        if ascending { cmp } else { cmp.reverse() }
    });
}

/// Cache for filtered/sorted pools.
#[derive(Debug, Default)]
pub struct PoolFilterCache {
    query: String,
    sort_column: PoolSortColumn,
    sort_asc: bool,
    cached: Arc<Vec<(usize, PoolInfo)>>,
    dirty: bool,
}

impl PoolFilterCache {
    /// Create a new cache with no computed result.
    pub fn new() -> Self {
        Self {
            dirty: true,
            ..Default::default()
        }
    }

    /// Mark the cache as dirty so the next `get` recomputes.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Return the cached filtered/sorted pool list, recomputing if necessary.
    pub fn get(
        &mut self,
        pools: &[PoolInfo],
        query: &str,
        sort_column: PoolSortColumn,
        sort_asc: bool,
    ) -> Arc<Vec<(usize, PoolInfo)>> {
        if self.dirty
            || self.query != query
            || self.sort_column != sort_column
            || self.sort_asc != sort_asc
        {
            self.query = query.to_string();
            self.sort_column = sort_column;
            self.sort_asc = sort_asc;
            let mut filtered: Vec<_> = filter_pools(pools, query)
                .into_iter()
                .map(|(idx, p)| (idx, p.clone()))
                .collect();
            sort_pools(&mut filtered, sort_column, sort_asc);
            self.cached = Arc::new(filtered);
            self.dirty = false;
        }
        self.cached.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::PoolState;

    fn sample_pools() -> Vec<PoolInfo> {
        vec![
            PoolInfo {
                id: 1,
                name: "Alpha Pool".to_string(),
                state: PoolState::Open,
                member_count: 100,
                total_bonded: 2_000_000,
                commission: None,
                apy: Some(0.12),
            },
            PoolInfo {
                id: 2,
                name: "Beta Pool".to_string(),
                state: PoolState::Open,
                member_count: 50,
                total_bonded: 1_000_000,
                commission: None,
                apy: Some(0.15),
            },
            PoolInfo {
                id: 3,
                name: "Gamma Pool".to_string(),
                state: PoolState::Blocked,
                member_count: 200,
                total_bonded: 3_000_000,
                commission: None,
                apy: None,
            },
        ]
    }

    #[test]
    fn test_filter_pools_empty_query() {
        let pools = sample_pools();
        let filtered = filter_pools(&pools, "");
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_filter_pools_by_name() {
        let pools = sample_pools();
        let filtered = filter_pools(&pools, "alpha");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].1.id, 1);
    }

    #[test]
    fn test_filter_pools_by_id() {
        let pools = sample_pools();
        let filtered = filter_pools(&pools, "2");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].1.id, 2);
    }

    #[test]
    fn test_filter_pools_no_match() {
        let pools = sample_pools();
        let filtered = filter_pools(&pools, "xyz");
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_sort_pools_by_id() {
        let mut pools: Vec<_> = sample_pools()
            .into_iter()
            .enumerate()
            .map(|(idx, p)| (idx, p))
            .collect();
        sort_pools(&mut pools, PoolSortColumn::Id, true);
        assert_eq!(pools[0].1.id, 1);
        assert_eq!(pools[1].1.id, 2);
        assert_eq!(pools[2].1.id, 3);
    }

    #[test]
    fn test_sort_pools_by_name() {
        let mut pools: Vec<_> = sample_pools()
            .into_iter()
            .enumerate()
            .map(|(idx, p)| (idx, p))
            .collect();
        sort_pools(&mut pools, PoolSortColumn::Name, true);
        assert_eq!(pools[0].1.name, "Alpha Pool");
        assert_eq!(pools[1].1.name, "Beta Pool");
        assert_eq!(pools[2].1.name, "Gamma Pool");
    }

    #[test]
    fn test_sort_pools_by_apy_desc() {
        let mut pools: Vec<_> = sample_pools()
            .into_iter()
            .enumerate()
            .map(|(idx, p)| (idx, p))
            .collect();
        sort_pools(&mut pools, PoolSortColumn::Apy, false);
        assert_eq!(pools[0].1.id, 2);
        assert_eq!(pools[1].1.id, 1);
        assert_eq!(pools[2].1.id, 3);
    }

    #[test]
    fn test_filter_cache_reuses_result() {
        let pools = sample_pools();
        let mut cache = PoolFilterCache::new();
        let first = cache.get(&pools, "", PoolSortColumn::Id, true);
        let second = cache.get(&pools, "", PoolSortColumn::Id, true);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn test_filter_cache_updates_on_search_change() {
        let pools = sample_pools();
        let mut cache = PoolFilterCache::new();
        let first = cache.get(&pools, "", PoolSortColumn::Id, true);
        assert_eq!(first.len(), 3);
        let second = cache.get(&pools, "alpha", PoolSortColumn::Id, true);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].1.id, 1);
    }

    #[test]
    fn test_filter_cache_updates_on_sort_change() {
        let pools = sample_pools();
        let mut cache = PoolFilterCache::new();
        let first = cache.get(&pools, "", PoolSortColumn::Id, true);
        assert_eq!(first[0].1.id, 1);
        let second = cache.get(&pools, "", PoolSortColumn::Id, false);
        assert_eq!(second[0].1.id, 3);
    }

    #[test]
    fn test_filter_cache_updates_when_source_changes() {
        let pools = sample_pools();
        let mut cache = PoolFilterCache::new();
        let first = cache.get(&pools, "", PoolSortColumn::Id, true);
        assert_eq!(first.len(), 3);

        cache.invalidate();
        let fewer = pools[..2].to_vec();
        let second = cache.get(&fewer, "", PoolSortColumn::Id, true);
        assert_eq!(second.len(), 2);
    }
}
