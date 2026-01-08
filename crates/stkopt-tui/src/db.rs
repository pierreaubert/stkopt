//! SQLite database for caching staking history, validator data, and chain metadata.

use crate::action::{DisplayValidator, StakingHistoryPoint};
use rusqlite::{Connection, Result, params};
use std::collections::HashMap;
use std::path::Path;
use stkopt_core::Network;

/// Cached chain metadata.
#[derive(Debug, Clone)]
pub struct CachedChainMetadata {
    pub genesis_hash: String,
    pub spec_version: u32,
    pub tx_version: u32,
    pub ss58_prefix: u16,
    pub token_symbol: String,
    pub token_decimals: u8,
    pub era_duration_ms: u64,
    pub current_era: u32,
}

/// Cached account status.
#[derive(Debug, Clone)]
pub struct CachedAccountStatus {
    pub free_balance: u128,
    pub reserved_balance: u128,
    pub frozen_balance: u128,
    pub staked_amount: u128,
    pub nominations_json: Option<String>,
    pub pool_id: Option<u32>,
    pub pool_points: Option<u128>,
}

/// Database wrapper for staking history storage.
pub struct HistoryDb {
    conn: Connection,
}

impl HistoryDb {
    /// Open or create the database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Open an in-memory database (for testing).
    #[allow(dead_code)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Initialize the database schema.
    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS staking_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                network TEXT NOT NULL,
                address TEXT NOT NULL,
                era INTEGER NOT NULL,
                date TEXT NOT NULL,
                reward INTEGER NOT NULL,
                bonded INTEGER NOT NULL,
                apy REAL NOT NULL,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(network, address, era)
            );

            CREATE INDEX IF NOT EXISTS idx_staking_history_lookup
                ON staking_history(network, address, era);

            CREATE INDEX IF NOT EXISTS idx_staking_history_era
                ON staking_history(network, address, era DESC);

            CREATE TABLE IF NOT EXISTS validator_identities (
                network TEXT NOT NULL,
                address TEXT NOT NULL,
                display_name TEXT NOT NULL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, address)
            );

            CREATE INDEX IF NOT EXISTS idx_validator_identities_network
                ON validator_identities(network);

            -- Cached validator data (active validators with APY, stake, etc.)
            CREATE TABLE IF NOT EXISTS cached_validators (
                network TEXT NOT NULL,
                address TEXT NOT NULL,
                commission REAL NOT NULL,
                blocked INTEGER NOT NULL DEFAULT 0,
                total_stake INTEGER NOT NULL,
                own_stake INTEGER NOT NULL,
                nominator_count INTEGER NOT NULL,
                points INTEGER NOT NULL DEFAULT 0,
                apy REAL NOT NULL,
                era INTEGER NOT NULL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, address)
            );

            CREATE INDEX IF NOT EXISTS idx_cached_validators_network_apy
                ON cached_validators(network, apy DESC);

            -- Cached account status
            CREATE TABLE IF NOT EXISTS cached_account_status (
                network TEXT NOT NULL,
                address TEXT NOT NULL,
                free_balance INTEGER NOT NULL DEFAULT 0,
                reserved_balance INTEGER NOT NULL DEFAULT 0,
                frozen_balance INTEGER NOT NULL DEFAULT 0,
                staked_amount INTEGER NOT NULL DEFAULT 0,
                nominations_json TEXT,
                pool_id INTEGER,
                pool_points INTEGER,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, address)
            );

            -- Chain metadata cache (spec version, genesis hash, etc.)
            CREATE TABLE IF NOT EXISTS chain_metadata (
                network TEXT PRIMARY KEY,
                genesis_hash TEXT NOT NULL,
                spec_version INTEGER NOT NULL,
                tx_version INTEGER NOT NULL,
                ss58_prefix INTEGER NOT NULL,
                token_symbol TEXT NOT NULL,
                token_decimals INTEGER NOT NULL,
                era_duration_ms INTEGER NOT NULL,
                current_era INTEGER NOT NULL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )?;
        Ok(())
    }

    /// Store a staking history point.
    pub fn insert_history(
        &self,
        network: Network,
        address: &str,
        point: &StakingHistoryPoint,
    ) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO staking_history
                (network, address, era, date, reward, bonded, apy)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                network.to_string(),
                address,
                point.era,
                point.date,
                point.reward as i64,
                point.bonded as i64,
                point.apy,
            ],
        )?;
        Ok(())
    }

    /// Store multiple history points in a transaction.
    pub fn insert_history_batch(
        &mut self,
        network: Network,
        address: &str,
        points: &[StakingHistoryPoint],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                r#"
                INSERT OR REPLACE INTO staking_history
                    (network, address, era, date, reward, bonded, apy)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
            )?;

            let network_str = network.to_string();
            for point in points {
                stmt.execute(params![
                    &network_str,
                    address,
                    point.era,
                    &point.date,
                    point.reward as i64,
                    point.bonded as i64,
                    point.apy,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Get history for an address, ordered by era descending.
    pub fn get_history(
        &self,
        network: Network,
        address: &str,
        limit: Option<u32>,
    ) -> Result<Vec<StakingHistoryPoint>> {
        let mut stmt = if let Some(limit) = limit {
            self.conn.prepare(&format!(
                r#"
                SELECT era, date, reward, bonded, apy
                FROM staking_history
                WHERE network = ?1 AND address = ?2
                ORDER BY era DESC
                LIMIT {}
                "#,
                limit
            ))?
        } else {
            self.conn.prepare(
                r#"
                SELECT era, date, reward, bonded, apy
                FROM staking_history
                WHERE network = ?1 AND address = ?2
                ORDER BY era DESC
                "#,
            )?
        };

        let rows = stmt.query_map(params![network.to_string(), address], |row| {
            Ok(StakingHistoryPoint {
                era: row.get(0)?,
                date: row.get(1)?,
                reward: row.get::<_, i64>(2)? as u128,
                bonded: row.get::<_, i64>(3)? as u128,
                apy: row.get(4)?,
            })
        })?;

        let mut points = Vec::new();
        for row in rows {
            points.push(row?);
        }
        // Reverse to get ascending order (oldest first)
        points.reverse();
        Ok(points)
    }

    /// Get the latest era stored for an address.
    pub fn get_latest_era(&self, network: Network, address: &str) -> Result<Option<u32>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT MAX(era) FROM staking_history
            WHERE network = ?1 AND address = ?2
            "#,
        )?;

        let result: Option<u32> = stmt
            .query_row(params![network.to_string(), address], |row| row.get(0))
            .ok();

        Ok(result)
    }

    /// Get eras that are missing in the cache for a given range.
    pub fn get_missing_eras(
        &self,
        network: Network,
        address: &str,
        from_era: u32,
        to_era: u32,
    ) -> Result<Vec<u32>> {
        let stored: std::collections::HashSet<u32> = {
            let mut stmt = self.conn.prepare(
                r#"
                SELECT era FROM staking_history
                WHERE network = ?1 AND address = ?2 AND era >= ?3 AND era <= ?4
                "#,
            )?;

            let rows = stmt.query_map(
                params![network.to_string(), address, from_era, to_era],
                |row| row.get(0),
            )?;

            let mut set = std::collections::HashSet::new();
            for row in rows {
                set.insert(row?);
            }
            set
        };

        let missing: Vec<u32> = (from_era..=to_era)
            .filter(|era| !stored.contains(era))
            .collect();

        Ok(missing)
    }

    /// Count total history entries for an address.
    pub fn count_history(&self, network: Network, address: &str) -> Result<u32> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT COUNT(*) FROM staking_history
            WHERE network = ?1 AND address = ?2
            "#,
        )?;

        let count: u32 = stmt.query_row(params![network.to_string(), address], |row| row.get(0))?;
        Ok(count)
    }

    /// Delete all history entries for an address (all networks).
    /// Used when removing an account from the address book.
    pub fn delete_address_history(&self, address: &str) -> Result<u32> {
        let deleted = self.conn.execute(
            "DELETE FROM staking_history WHERE address = ?1",
            params![address],
        )?;
        Ok(deleted as u32)
    }

    /// Delete old history entries beyond a certain count.
    #[allow(dead_code)]
    pub fn prune_history(&self, network: Network, address: &str, keep_count: u32) -> Result<u32> {
        let deleted = self.conn.execute(
            r#"
            DELETE FROM staking_history
            WHERE network = ?1 AND address = ?2
            AND era NOT IN (
                SELECT era FROM staking_history
                WHERE network = ?1 AND address = ?2
                ORDER BY era DESC
                LIMIT ?3
            )
            "#,
            params![network.to_string(), address, keep_count],
        )?;
        Ok(deleted as u32)
    }

    // ==================== Validator Identity Cache ====================

    /// Get all cached validator identities for a network.
    pub fn get_validator_identities(&self, network: Network) -> Result<HashMap<String, String>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT address, display_name
            FROM validator_identities
            WHERE network = ?1
            "#,
        )?;

        let rows = stmt.query_map(params![network.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut identities = HashMap::new();
        for row in rows {
            let (address, name) = row?;
            identities.insert(address, name);
        }
        Ok(identities)
    }

    /// Store or update a validator identity.
    pub fn set_validator_identity(
        &self,
        network: Network,
        address: &str,
        display_name: &str,
    ) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO validator_identities
                (network, address, display_name, updated_at)
            VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
            "#,
            params![network.to_string(), address, display_name],
        )?;
        Ok(())
    }

    /// Store multiple validator identities in a transaction.
    pub fn set_validator_identities_batch(
        &mut self,
        network: Network,
        identities: &HashMap<String, String>,
    ) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;
        {
            let mut stmt = tx.prepare(
                r#"
                INSERT OR REPLACE INTO validator_identities
                    (network, address, display_name, updated_at)
                VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
                "#,
            )?;

            let network_str = network.to_string();
            for (address, name) in identities {
                stmt.execute(params![&network_str, address, name])?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    /// Get the count of cached identities for a network.
    pub fn count_validator_identities(&self, network: Network) -> Result<u32> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT COUNT(*) FROM validator_identities
            WHERE network = ?1
            "#,
        )?;

        let count: u32 = stmt.query_row(params![network.to_string()], |row| row.get(0))?;
        Ok(count)
    }

    // ==================== Cached Validators ====================

    /// Get cached validators for a network, ordered by APY descending.
    pub fn get_cached_validators(&self, network: Network) -> Result<Vec<DisplayValidator>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT v.address, v.commission, v.blocked, v.total_stake, v.own_stake,
                   v.nominator_count, v.points, v.apy, v.era,
                   COALESCE(i.display_name, '') as name
            FROM cached_validators v
            LEFT JOIN validator_identities i ON v.network = i.network AND v.address = i.address
            WHERE v.network = ?1
            ORDER BY v.apy DESC
            "#,
        )?;

        let rows = stmt.query_map(params![network.to_string()], |row| {
            Ok(DisplayValidator {
                address: row.get(0)?,
                name: row.get(9)?,
                commission: row.get(1)?,
                blocked: row.get::<_, i32>(2)? != 0,
                total_stake: row.get::<_, i64>(3)? as u128,
                own_stake: row.get::<_, i64>(4)? as u128,
                nominator_count: row.get(5)?,
                points: row.get(6)?,
                apy: row.get(7)?,
            })
        })?;

        let mut validators = Vec::new();
        for row in rows {
            validators.push(row?);
        }
        Ok(validators)
    }

    /// Store validators in the cache.
    pub fn set_cached_validators(
        &mut self,
        network: Network,
        era: u32,
        validators: &[DisplayValidator],
    ) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;
        {
            // Clear old validators for this network
            tx.execute(
                "DELETE FROM cached_validators WHERE network = ?1",
                params![network.to_string()],
            )?;

            let mut stmt = tx.prepare(
                r#"
                INSERT INTO cached_validators
                    (network, address, commission, blocked, total_stake, own_stake,
                     nominator_count, points, apy, era)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
            )?;

            let network_str = network.to_string();
            for v in validators {
                stmt.execute(params![
                    &network_str,
                    &v.address,
                    v.commission,
                    if v.blocked { 1 } else { 0 },
                    v.total_stake as i64,
                    v.own_stake as i64,
                    v.nominator_count,
                    v.points,
                    v.apy,
                    era,
                ])?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    /// Get count of cached validators.
    pub fn count_cached_validators(&self, network: Network) -> Result<u32> {
        let mut stmt = self
            .conn
            .prepare("SELECT COUNT(*) FROM cached_validators WHERE network = ?1")?;
        let count: u32 = stmt.query_row(params![network.to_string()], |row| row.get(0))?;
        Ok(count)
    }

    // ==================== Chain Metadata ====================

    /// Get cached chain metadata.
    pub fn get_chain_metadata(&self, network: Network) -> Result<Option<CachedChainMetadata>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT genesis_hash, spec_version, tx_version, ss58_prefix,
                   token_symbol, token_decimals, era_duration_ms, current_era
            FROM chain_metadata
            WHERE network = ?1
            "#,
        )?;

        let result = stmt.query_row(params![network.to_string()], |row| {
            Ok(CachedChainMetadata {
                genesis_hash: row.get(0)?,
                spec_version: row.get(1)?,
                tx_version: row.get(2)?,
                ss58_prefix: row.get(3)?,
                token_symbol: row.get(4)?,
                token_decimals: row.get(5)?,
                era_duration_ms: row.get(6)?,
                current_era: row.get(7)?,
            })
        });

        match result {
            Ok(meta) => Ok(Some(meta)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Store chain metadata in cache.
    pub fn set_chain_metadata(&self, network: Network, meta: &CachedChainMetadata) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO chain_metadata
                (network, genesis_hash, spec_version, tx_version, ss58_prefix,
                 token_symbol, token_decimals, era_duration_ms, current_era, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, CURRENT_TIMESTAMP)
            "#,
            params![
                network.to_string(),
                &meta.genesis_hash,
                meta.spec_version,
                meta.tx_version,
                meta.ss58_prefix,
                &meta.token_symbol,
                meta.token_decimals,
                meta.era_duration_ms,
                meta.current_era,
            ],
        )?;
        Ok(())
    }

    // ==================== Cached Account Status ====================

    /// Get cached account status.
    pub fn get_cached_account_status(
        &self,
        network: Network,
        address: &str,
    ) -> Result<Option<CachedAccountStatus>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT free_balance, reserved_balance, frozen_balance, staked_amount,
                   nominations_json, pool_id, pool_points
            FROM cached_account_status
            WHERE network = ?1 AND address = ?2
            "#,
        )?;

        let result = stmt.query_row(params![network.to_string(), address], |row| {
            Ok(CachedAccountStatus {
                free_balance: row.get::<_, i64>(0)? as u128,
                reserved_balance: row.get::<_, i64>(1)? as u128,
                frozen_balance: row.get::<_, i64>(2)? as u128,
                staked_amount: row.get::<_, i64>(3)? as u128,
                nominations_json: row.get(4)?,
                pool_id: row.get(5)?,
                pool_points: row.get::<_, Option<i64>>(6)?.map(|p| p as u128),
            })
        });

        match result {
            Ok(status) => Ok(Some(status)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Store account status in cache.
    pub fn set_cached_account_status(
        &self,
        network: Network,
        address: &str,
        status: &CachedAccountStatus,
    ) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO cached_account_status
                (network, address, free_balance, reserved_balance, frozen_balance,
                 staked_amount, nominations_json, pool_id, pool_points, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, CURRENT_TIMESTAMP)
            "#,
            params![
                network.to_string(),
                address,
                status.free_balance as i64,
                status.reserved_balance as i64,
                status.frozen_balance as i64,
                status.staked_amount as i64,
                &status.nominations_json,
                status.pool_id,
                status.pool_points.map(|p| p as i64),
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_point(era: u32) -> StakingHistoryPoint {
        StakingHistoryPoint {
            era,
            date: format!("2025010{}", era % 10),
            reward: 1_000_000_000_000,
            bonded: 100_000_000_000_000,
            apy: 0.15,
        }
    }

    fn make_test_validator(address: &str, apy: f64) -> DisplayValidator {
        DisplayValidator {
            address: address.to_string(),
            name: Some(format!("Validator {}", address)),
            commission: 0.05,
            blocked: false,
            total_stake: 1_000_000_000_000_000,
            own_stake: 100_000_000_000_000,
            nominator_count: 100,
            points: 1000,
            apy,
        }
    }

    #[test]
    fn test_insert_and_get_history() {
        let db = HistoryDb::open_memory().unwrap();

        let point = StakingHistoryPoint {
            era: 1500,
            date: "20250101".to_string(),
            reward: 1_000_000_000_000,
            bonded: 100_000_000_000_000,
            apy: 0.15,
        };

        db.insert_history(Network::Polkadot, "15oF4uV", &point)
            .unwrap();

        let history = db.get_history(Network::Polkadot, "15oF4uV", None).unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].era, 1500);
        assert_eq!(history[0].reward, 1_000_000_000_000);
    }

    #[test]
    fn test_get_missing_eras() {
        let mut db = HistoryDb::open_memory().unwrap();

        let points = vec![
            StakingHistoryPoint {
                era: 1500,
                date: "20250101".to_string(),
                reward: 1_000_000_000_000,
                bonded: 100_000_000_000_000,
                apy: 0.15,
            },
            StakingHistoryPoint {
                era: 1502,
                date: "20250103".to_string(),
                reward: 1_000_000_000_000,
                bonded: 100_000_000_000_000,
                apy: 0.15,
            },
        ];

        db.insert_history_batch(Network::Polkadot, "15oF4uV", &points)
            .unwrap();

        let missing = db
            .get_missing_eras(Network::Polkadot, "15oF4uV", 1499, 1503)
            .unwrap();

        assert_eq!(missing, vec![1499, 1501, 1503]);
    }

    #[test]
    fn test_get_history_with_limit() {
        let mut db = HistoryDb::open_memory().unwrap();
        let points: Vec<_> = (1..=10).map(|i| make_test_point(1500 + i)).collect();
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let history = db
            .get_history(Network::Polkadot, "addr1", Some(3))
            .unwrap();
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_get_history_empty() {
        let db = HistoryDb::open_memory().unwrap();
        let history = db
            .get_history(Network::Polkadot, "nonexistent", None)
            .unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn test_get_latest_era() {
        let mut db = HistoryDb::open_memory().unwrap();
        let points = vec![make_test_point(1500), make_test_point(1505), make_test_point(1503)];
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let latest = db.get_latest_era(Network::Polkadot, "addr1").unwrap();
        assert_eq!(latest, Some(1505));
    }

    #[test]
    fn test_get_latest_era_empty() {
        let db = HistoryDb::open_memory().unwrap();
        let latest = db.get_latest_era(Network::Polkadot, "nonexistent").unwrap();
        assert!(latest.is_none());
    }

    #[test]
    fn test_count_history() {
        let mut db = HistoryDb::open_memory().unwrap();
        let points: Vec<_> = (0..5).map(|i| make_test_point(1500 + i)).collect();
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let count = db.count_history(Network::Polkadot, "addr1").unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn test_count_history_empty() {
        let db = HistoryDb::open_memory().unwrap();
        let count = db.count_history(Network::Polkadot, "nonexistent").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_delete_address_history() {
        let mut db = HistoryDb::open_memory().unwrap();
        let points = vec![make_test_point(1500), make_test_point(1501)];
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();
        db.insert_history_batch(Network::Kusama, "addr1", &points)
            .unwrap();

        let deleted = db.delete_address_history("addr1").unwrap();
        assert_eq!(deleted, 4); // Both networks

        let count = db.count_history(Network::Polkadot, "addr1").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_prune_history() {
        let mut db = HistoryDb::open_memory().unwrap();
        let points: Vec<_> = (0..10).map(|i| make_test_point(1500 + i)).collect();
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let deleted = db.prune_history(Network::Polkadot, "addr1", 3).unwrap();
        assert_eq!(deleted, 7);

        let count = db.count_history(Network::Polkadot, "addr1").unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_validator_identities() {
        let mut db = HistoryDb::open_memory().unwrap();

        // Single insert
        db.set_validator_identity(Network::Polkadot, "val1", "Validator One")
            .unwrap();

        let identities = db.get_validator_identities(Network::Polkadot).unwrap();
        assert_eq!(identities.len(), 1);
        assert_eq!(identities.get("val1"), Some(&"Validator One".to_string()));

        // Batch insert
        let mut batch = HashMap::new();
        batch.insert("val2".to_string(), "Validator Two".to_string());
        batch.insert("val3".to_string(), "Validator Three".to_string());
        let count = db
            .set_validator_identities_batch(Network::Polkadot, &batch)
            .unwrap();
        assert_eq!(count, 2);

        let identities = db.get_validator_identities(Network::Polkadot).unwrap();
        assert_eq!(identities.len(), 3);
    }

    #[test]
    fn test_count_validator_identities() {
        let mut db = HistoryDb::open_memory().unwrap();
        let mut batch = HashMap::new();
        batch.insert("val1".to_string(), "V1".to_string());
        batch.insert("val2".to_string(), "V2".to_string());
        db.set_validator_identities_batch(Network::Polkadot, &batch)
            .unwrap();

        let count = db.count_validator_identities(Network::Polkadot).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_cached_validators() {
        let mut db = HistoryDb::open_memory().unwrap();
        let validators = vec![
            make_test_validator("val1", 0.15),
            make_test_validator("val2", 0.12),
            make_test_validator("val3", 0.18),
        ];

        let count = db
            .set_cached_validators(Network::Polkadot, 1500, &validators)
            .unwrap();
        assert_eq!(count, 3);

        let cached = db.get_cached_validators(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 3);
        // Should be ordered by APY descending
        assert_eq!(cached[0].address, "val3");
        assert_eq!(cached[1].address, "val1");
        assert_eq!(cached[2].address, "val2");
    }

    #[test]
    fn test_count_cached_validators() {
        let mut db = HistoryDb::open_memory().unwrap();
        let validators = vec![
            make_test_validator("val1", 0.15),
            make_test_validator("val2", 0.12),
        ];
        db.set_cached_validators(Network::Polkadot, 1500, &validators)
            .unwrap();

        let count = db.count_cached_validators(Network::Polkadot).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_cached_validators_replaces_old() {
        let mut db = HistoryDb::open_memory().unwrap();
        let validators1 = vec![make_test_validator("val1", 0.15)];
        db.set_cached_validators(Network::Polkadot, 1500, &validators1)
            .unwrap();

        let validators2 = vec![
            make_test_validator("val2", 0.12),
            make_test_validator("val3", 0.18),
        ];
        db.set_cached_validators(Network::Polkadot, 1501, &validators2)
            .unwrap();

        let cached = db.get_cached_validators(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 2);
        assert!(!cached.iter().any(|v| v.address == "val1"));
    }

    #[test]
    fn test_chain_metadata() {
        let db = HistoryDb::open_memory().unwrap();

        let meta = CachedChainMetadata {
            genesis_hash: "0x91b171bb158e2d3848fa23a9f1c25182".to_string(),
            spec_version: 1002000,
            tx_version: 26,
            ss58_prefix: 0,
            token_symbol: "DOT".to_string(),
            token_decimals: 10,
            era_duration_ms: 86400000,
            current_era: 1500,
        };

        db.set_chain_metadata(Network::Polkadot, &meta).unwrap();

        let loaded = db.get_chain_metadata(Network::Polkadot).unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.genesis_hash, meta.genesis_hash);
        assert_eq!(loaded.spec_version, 1002000);
        assert_eq!(loaded.token_symbol, "DOT");
    }

    #[test]
    fn test_chain_metadata_not_found() {
        let db = HistoryDb::open_memory().unwrap();
        let meta = db.get_chain_metadata(Network::Polkadot).unwrap();
        assert!(meta.is_none());
    }

    #[test]
    fn test_cached_account_status() {
        let db = HistoryDb::open_memory().unwrap();

        let status = CachedAccountStatus {
            free_balance: 100_000_000_000_000,
            reserved_balance: 10_000_000_000_000,
            frozen_balance: 5_000_000_000_000,
            staked_amount: 50_000_000_000_000,
            nominations_json: Some("[\"val1\",\"val2\"]".to_string()),
            pool_id: Some(42),
            pool_points: Some(1_000_000),
        };

        db.set_cached_account_status(Network::Polkadot, "addr1", &status)
            .unwrap();

        let loaded = db
            .get_cached_account_status(Network::Polkadot, "addr1")
            .unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.free_balance, status.free_balance);
        assert_eq!(loaded.pool_id, Some(42));
        assert_eq!(loaded.nominations_json, status.nominations_json);
    }

    #[test]
    fn test_cached_account_status_not_found() {
        let db = HistoryDb::open_memory().unwrap();
        let status = db
            .get_cached_account_status(Network::Polkadot, "nonexistent")
            .unwrap();
        assert!(status.is_none());
    }

    #[test]
    fn test_cached_account_status_without_optional_fields() {
        let db = HistoryDb::open_memory().unwrap();

        let status = CachedAccountStatus {
            free_balance: 100_000_000_000_000,
            reserved_balance: 0,
            frozen_balance: 0,
            staked_amount: 0,
            nominations_json: None,
            pool_id: None,
            pool_points: None,
        };

        db.set_cached_account_status(Network::Polkadot, "addr1", &status)
            .unwrap();

        let loaded = db
            .get_cached_account_status(Network::Polkadot, "addr1")
            .unwrap()
            .unwrap();
        assert!(loaded.nominations_json.is_none());
        assert!(loaded.pool_id.is_none());
        assert!(loaded.pool_points.is_none());
    }

    #[test]
    fn test_insert_or_replace_history() {
        let db = HistoryDb::open_memory().unwrap();

        let point1 = StakingHistoryPoint {
            era: 1500,
            date: "20250101".to_string(),
            reward: 1_000_000_000_000,
            bonded: 100_000_000_000_000,
            apy: 0.15,
        };
        db.insert_history(Network::Polkadot, "addr1", &point1)
            .unwrap();

        // Insert again with different values - should replace
        let point2 = StakingHistoryPoint {
            era: 1500,
            date: "20250101".to_string(),
            reward: 2_000_000_000_000, // Different reward
            bonded: 100_000_000_000_000,
            apy: 0.16, // Different APY
        };
        db.insert_history(Network::Polkadot, "addr1", &point2)
            .unwrap();

        let history = db.get_history(Network::Polkadot, "addr1", None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].reward, 2_000_000_000_000);
        assert_eq!(history[0].apy, 0.16);
    }

    #[test]
    fn test_cached_chain_metadata_clone() {
        let meta = CachedChainMetadata {
            genesis_hash: "0x91b171bb158e2d3848fa23a9f1c25182".to_string(),
            spec_version: 1002000,
            tx_version: 26,
            ss58_prefix: 0,
            token_symbol: "DOT".to_string(),
            token_decimals: 10,
            era_duration_ms: 86400000,
            current_era: 1500,
        };
        let meta_clone = meta.clone();
        assert_eq!(meta.genesis_hash, meta_clone.genesis_hash);
        assert_eq!(meta.spec_version, meta_clone.spec_version);
    }

    #[test]
    fn test_cached_account_status_clone() {
        let status = CachedAccountStatus {
            free_balance: 100_000_000_000_000,
            reserved_balance: 10_000_000_000_000,
            frozen_balance: 5_000_000_000_000,
            staked_amount: 50_000_000_000_000,
            nominations_json: Some("[\"val1\"]".to_string()),
            pool_id: Some(42),
            pool_points: Some(1_000_000),
        };
        let status_clone = status.clone();
        assert_eq!(status.free_balance, status_clone.free_balance);
        assert_eq!(status.pool_id, status_clone.pool_id);
    }

    #[test]
    fn test_validator_identity_update() {
        let db = HistoryDb::open_memory().unwrap();

        db.set_validator_identity(Network::Polkadot, "val1", "Name 1")
            .unwrap();
        db.set_validator_identity(Network::Polkadot, "val1", "Name 2")
            .unwrap();

        let identities = db.get_validator_identities(Network::Polkadot).unwrap();
        assert_eq!(identities.len(), 1);
        assert_eq!(identities.get("val1"), Some(&"Name 2".to_string()));
    }

    #[test]
    fn test_cached_validators_with_identity_join() {
        let mut db = HistoryDb::open_memory().unwrap();

        // Set up identity
        db.set_validator_identity(Network::Polkadot, "val1", "My Validator")
            .unwrap();

        // Set up validator
        let validators = vec![DisplayValidator {
            address: "val1".to_string(),
            name: None, // Empty - should be filled by join
            commission: 0.05,
            blocked: false,
            total_stake: 1_000_000,
            own_stake: 100_000,
            nominator_count: 10,
            points: 100,
            apy: 0.15,
        }];
        db.set_cached_validators(Network::Polkadot, 1500, &validators)
            .unwrap();

        let cached = db.get_cached_validators(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].name, Some("My Validator".to_string()));
    }

    #[test]
    fn test_blocked_validator_storage() {
        let mut db = HistoryDb::open_memory().unwrap();

        let validators = vec![DisplayValidator {
            address: "val1".to_string(),
            name: None,
            commission: 0.05,
            blocked: true, // Blocked validator
            total_stake: 1_000_000,
            own_stake: 100_000,
            nominator_count: 10,
            points: 100,
            apy: 0.15,
        }];
        db.set_cached_validators(Network::Polkadot, 1500, &validators)
            .unwrap();

        let cached = db.get_cached_validators(Network::Polkadot).unwrap();
        assert!(cached[0].blocked);
    }

    #[test]
    fn test_network_isolation() {
        let db = HistoryDb::open_memory().unwrap();

        // Insert for Polkadot
        db.insert_history(Network::Polkadot, "addr1", &make_test_point(1500))
            .unwrap();
        // Insert for Kusama
        db.insert_history(Network::Kusama, "addr1", &make_test_point(6000))
            .unwrap();

        let polkadot_history = db.get_history(Network::Polkadot, "addr1", None).unwrap();
        let kusama_history = db.get_history(Network::Kusama, "addr1", None).unwrap();

        assert_eq!(polkadot_history.len(), 1);
        assert_eq!(polkadot_history[0].era, 1500);
        assert_eq!(kusama_history.len(), 1);
        assert_eq!(kusama_history[0].era, 6000);
    }
}
