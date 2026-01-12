//! SQLite database for caching staking history, validator data, and chain metadata.

use rusqlite::{params, Connection, Result};
use std::collections::HashMap;
use std::path::Path;
use stkopt_core::Network;

use crate::app::{HistoryPoint, PoolInfo, PoolState, ValidatorInfo};

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
pub struct Db {
    conn: Connection,
}

impl Db {
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

            -- Cached pools
            CREATE TABLE IF NOT EXISTS cached_pools (
                network TEXT NOT NULL,
                id INTEGER NOT NULL,
                name TEXT NOT NULL,
                state TEXT NOT NULL,
                member_count INTEGER NOT NULL,
                points INTEGER NOT NULL,
                commission REAL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, id)
            );

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
        point: &HistoryPoint,
    ) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO staking_history
                (network, address, era, reward, bonded, apy)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                network.to_string(),
                address,
                point.era,
                point.rewards as i64,
                point.staked as i64,
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
        points: &[HistoryPoint],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                r#"
                INSERT OR REPLACE INTO staking_history
                    (network, address, era, reward, bonded, apy)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
            )?;

            let network_str = network.to_string();
            for point in points {
                stmt.execute(params![
                    &network_str,
                    address,
                    point.era,
                    point.rewards as i64,
                    point.staked as i64,
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
    ) -> Result<Vec<HistoryPoint>> {
        let mut stmt = if let Some(limit) = limit {
            self.conn.prepare(&format!(
                r#"
                SELECT era, reward, bonded, apy
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
                SELECT era, reward, bonded, apy
                FROM staking_history
                WHERE network = ?1 AND address = ?2
                ORDER BY era DESC
                "#,
            )?
        };

        let rows = stmt.query_map(params![network.to_string(), address], |row| {
            Ok(HistoryPoint {
                era: row.get(0)?,
                rewards: row.get::<_, i64>(1)? as u128,
                staked: row.get::<_, i64>(2)? as u128,
                apy: row.get(3)?,
            })
        })?;

        let mut points = Vec::new();
        for row in rows {
            points.push(row?);
        }
        // Reverse to get ascending order (oldest first) for charts
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

    /// Delete all history entries for an address.
    pub fn delete_address_history(&self, address: &str) -> Result<u32> {
        let deleted = self.conn.execute(
            "DELETE FROM staking_history WHERE address = ?1",
            params![address],
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

    // ==================== Cached Validators ====================

    /// Get cached validators for a network, ordered by APY descending.
    pub fn get_cached_validators(&self, network: Network) -> Result<Vec<ValidatorInfo>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT v.address, v.commission, v.blocked, v.total_stake, v.own_stake,
                   v.nominator_count, v.points, v.apy,
                   COALESCE(i.display_name, '') as name
            FROM cached_validators v
            LEFT JOIN validator_identities i ON v.network = i.network AND v.address = i.address
            WHERE v.network = ?1
            ORDER BY v.apy DESC
            "#,
        )?;

        let rows = stmt.query_map(params![network.to_string()], |row| {
            let name_str: String = row.get(8)?;
            Ok(ValidatorInfo {
                address: row.get(0)?,
                name: if name_str.is_empty() { None } else { Some(name_str) },
                commission: row.get(1)?,
                blocked: row.get::<_, i32>(2)? != 0,
                total_stake: row.get::<_, i64>(3)? as u128,
                own_stake: row.get::<_, i64>(4)? as u128,
                nominator_count: row.get(5)?,
                apy: Some(row.get(7)?),
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
        validators: &[ValidatorInfo],
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
                    0, // points - usually not in ValidatorInfo unless we update the struct
                    v.apy.unwrap_or(0.0),
                    era,
                ])?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    // ==================== Cached Pools ====================

    /// Get cached pools for a network.
    pub fn get_cached_pools(&self, network: Network) -> Result<Vec<PoolInfo>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, name, state, member_count, points, commission
            FROM cached_pools
            WHERE network = ?1
            "#,
        )?;

        let rows = stmt.query_map(params![network.to_string()], |row| {
            let state_str: String = row.get(2)?;
            let state = match state_str.as_str() {
                "Open" => PoolState::Open,
                "Blocked" => PoolState::Blocked,
                _ => PoolState::Destroying,
            };
            
            Ok(PoolInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                state,
                member_count: row.get(3)?,
                total_bonded: row.get::<_, i64>(4)? as u128,
                commission: row.get(5)?,
            })
        })?;

        let mut pools = Vec::new();
        for row in rows {
            pools.push(row?);
        }
        Ok(pools)
    }

    /// Store pools in the cache.
    pub fn set_cached_pools(
        &mut self,
        network: Network,
        pools: &[PoolInfo],
    ) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;
        {
            // Clear old pools for this network
            tx.execute(
                "DELETE FROM cached_pools WHERE network = ?1",
                params![network.to_string()],
            )?;

            let mut stmt = tx.prepare(
                r#"
                INSERT INTO cached_pools
                    (network, id, name, state, member_count, points, commission)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
            )?;

            let network_str = network.to_string();
            for p in pools {
                let state_str = match p.state {
                    PoolState::Open => "Open",
                    PoolState::Blocked => "Blocked",
                    PoolState::Destroying => "Destroying",
                };
                
                stmt.execute(params![
                    &network_str,
                    p.id,
                    &p.name,
                    state_str,
                    p.member_count,
                    p.total_bonded as i64,
                    p.commission,
                ])?;
                count += 1;
            }
        }
        tx.commit()?;
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
