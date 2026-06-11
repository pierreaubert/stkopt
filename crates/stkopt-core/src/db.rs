//! SQLite database for caching staking history, validator data, and chain metadata.
//!
//! This module provides a unified database layer used by both TUI and GPUI frontends.
//! The schema supports all features from both implementations.

use rusqlite::types::ValueRef;
use rusqlite::{Connection, Result, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::display::{DisplayPool, DisplayValidator, StakingHistoryPoint};
use crate::types::{Network, PoolState};

fn read_u128(row: &rusqlite::Row<'_>, index: usize) -> Result<u128> {
    match row.get_ref(index)? {
        ValueRef::Integer(value) if value >= 0 => Ok(value as u128),
        ValueRef::Integer(value) => Err(rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("negative integer cannot be read as u128: {}", value),
            )),
        )),
        ValueRef::Text(value) => {
            let value = std::str::from_utf8(value).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?;
            value.parse::<u128>().map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })
        }
        value => Err(rusqlite::Error::InvalidColumnType(
            index,
            "u128".to_string(),
            value.data_type(),
        )),
    }
}

fn read_option_u128(row: &rusqlite::Row<'_>, index: usize) -> Result<Option<u128>> {
    match row.get_ref(index)? {
        ValueRef::Null => Ok(None),
        _ => read_u128(row, index).map(Some),
    }
}

/// Cached chain metadata from the blockchain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedChainMetadata {
    /// Genesis block hash.
    pub genesis_hash: String,
    /// Runtime spec version.
    pub spec_version: u32,
    /// Transaction version.
    pub tx_version: u32,
    /// SS58 address prefix.
    pub ss58_prefix: u16,
    /// Token symbol (e.g., "DOT", "KSM").
    pub token_symbol: String,
    /// Token decimals (e.g., 10 for DOT, 12 for KSM).
    pub token_decimals: u8,
    /// Era duration in milliseconds.
    pub era_duration_ms: u64,
    /// Current era index at time of caching.
    pub current_era: u32,
}

/// Cached account status including balances and staking info.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CachedAccountStatus {
    /// Free (transferable) balance.
    pub free_balance: u128,
    /// Reserved balance.
    pub reserved_balance: u128,
    /// Frozen balance.
    pub frozen_balance: u128,
    /// Amount currently staked.
    pub staked_amount: u128,
    /// JSON-encoded list of nominated validator addresses.
    pub nominations_json: Option<String>,
    /// Nomination pool ID if in a pool.
    pub pool_id: Option<u32>,
    /// Pool points if in a pool.
    pub pool_points: Option<u128>,
}

/// Unified database wrapper for staking data storage.
///
/// This struct provides a single interface for both TUI and GPUI frontends.
/// It supports all features from both implementations including:
/// - Staking history with optional date field
/// - Validator identity caching
/// - Cached validators with APY data
/// - Nomination pool caching
/// - Account status caching
/// - Chain metadata caching
pub struct StakingDb {
    conn: Connection,
}

impl StakingDb {
    /// Open or create the database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let mut db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Open an in-memory database (for testing).
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Initialize the database schema.
    fn init_schema(&mut self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            -- Staking history table (date is optional for GPUI compatibility)
            CREATE TABLE IF NOT EXISTS staking_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                network TEXT NOT NULL,
                address TEXT NOT NULL,
                era INTEGER NOT NULL,
                date TEXT,
                reward TEXT NOT NULL,
                bonded TEXT NOT NULL,
                apy REAL NOT NULL,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(network, address, era)
            );

            CREATE INDEX IF NOT EXISTS idx_staking_history_lookup
                ON staking_history(network, address, era);

            CREATE INDEX IF NOT EXISTS idx_staking_history_era
                ON staking_history(network, address, era DESC);

            -- Validator identities (display names)
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
                total_stake TEXT NOT NULL,
                own_stake TEXT NOT NULL,
                nominator_count INTEGER NOT NULL,
                points INTEGER NOT NULL DEFAULT 0,
                apy REAL,
                era INTEGER NOT NULL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, address)
            );

            CREATE INDEX IF NOT EXISTS idx_cached_validators_network_apy
                ON cached_validators(network, apy DESC);

            -- Cached nomination pools
            CREATE TABLE IF NOT EXISTS cached_pools (
                network TEXT NOT NULL,
                id INTEGER NOT NULL,
                name TEXT NOT NULL,
                state TEXT NOT NULL,
                member_count INTEGER NOT NULL,
                total_bonded TEXT NOT NULL,
                commission REAL,
                apy REAL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, id)
            );

            -- Cached account status
            CREATE TABLE IF NOT EXISTS cached_account_status (
                network TEXT NOT NULL,
                address TEXT NOT NULL,
                free_balance TEXT NOT NULL DEFAULT '0',
                reserved_balance TEXT NOT NULL DEFAULT '0',
                frozen_balance TEXT NOT NULL DEFAULT '0',
                staked_amount TEXT NOT NULL DEFAULT '0',
                nominations_json TEXT,
                pool_id INTEGER,
                pool_points TEXT,
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

        let user_version: i32 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;

        // Clean up any orphaned tables from failed previous migrations
        self.conn.execute_batch(
            r#"
            DROP TABLE IF EXISTS cached_pools_old;
            DROP TABLE IF EXISTS cached_validators_old;
            DROP TABLE IF EXISTS staking_history_old;
            DROP TABLE IF EXISTS cached_account_status_old;
            "#,
        )?;

        // Always validate critical schemas regardless of user_version,
        // to handle DBs created by intermediate code versions.
        self.migrate_cached_pools_total_bonded_to_text()?;

        if user_version >= 2 {
            return Ok(());
        }

        self.migrate_cached_validator_stakes_to_text()?;
        self.purge_invalid_cached_validator_stakes()?;
        self.migrate_staking_history_balances_to_text()?;
        self.migrate_cached_account_status_balances_to_text()?;

        self.conn.execute_batch("PRAGMA user_version = 2;")?;
        Ok(())
    }

    fn migrate_cached_validator_stakes_to_text(&self) -> Result<()> {
        let mut stmt = self.conn.prepare("PRAGMA table_info(cached_validators)")?;
        let columns = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;

        let mut total_stake_type = None;
        let mut own_stake_type = None;
        for column in columns {
            let (name, column_type) = column?;
            match name.as_str() {
                "total_stake" => total_stake_type = Some(column_type),
                "own_stake" => own_stake_type = Some(column_type),
                _ => {}
            }
        }

        if total_stake_type.as_deref() == Some("TEXT") && own_stake_type.as_deref() == Some("TEXT")
        {
            return Ok(());
        }

        let result = self.conn.execute_batch(
            r#"
            BEGIN IMMEDIATE;

            DROP INDEX IF EXISTS idx_cached_validators_network_apy;

            ALTER TABLE cached_validators RENAME TO cached_validators_old;

            CREATE TABLE cached_validators (
                network TEXT NOT NULL,
                address TEXT NOT NULL,
                commission REAL NOT NULL,
                blocked INTEGER NOT NULL DEFAULT 0,
                total_stake TEXT NOT NULL,
                own_stake TEXT NOT NULL,
                nominator_count INTEGER NOT NULL,
                points INTEGER NOT NULL DEFAULT 0,
                apy REAL,
                era INTEGER NOT NULL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, address)
            );

            INSERT INTO cached_validators
                (network, address, commission, blocked, total_stake, own_stake,
                 nominator_count, points, apy, era, updated_at)
            SELECT network, address, commission, blocked, CAST(total_stake AS TEXT),
                   CAST(own_stake AS TEXT), nominator_count, points, apy, era, updated_at
            FROM cached_validators_old
            WHERE CAST(total_stake AS TEXT) NOT GLOB '-*'
              AND CAST(own_stake AS TEXT) NOT GLOB '-*';

            DROP TABLE cached_validators_old;

            CREATE INDEX IF NOT EXISTS idx_cached_validators_network_apy
                ON cached_validators(network, apy DESC);

            COMMIT;
            "#,
        );

        if let Err(err) = result {
            let _ = self.conn.execute_batch("ROLLBACK;");
            return Err(err);
        }

        Ok(())
    }

    fn purge_invalid_cached_validator_stakes(&self) -> Result<()> {
        let invalid_rows = {
            let mut stmt = self.conn.prepare(
                r#"
                SELECT network, address, CAST(total_stake AS TEXT), CAST(own_stake AS TEXT)
                FROM cached_validators
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;

            let mut invalid_rows = Vec::new();
            for row in rows {
                let (network, address, total_stake, own_stake) = row?;
                if total_stake.parse::<u128>().is_err() || own_stake.parse::<u128>().is_err() {
                    invalid_rows.push((network, address));
                }
            }
            invalid_rows
        };

        for (network, address) in invalid_rows {
            self.conn.execute(
                "DELETE FROM cached_validators WHERE network = ?1 AND address = ?2",
                params![network, address],
            )?;
        }

        Ok(())
    }

    fn migrate_staking_history_balances_to_text(&self) -> Result<()> {
        let mut stmt = self.conn.prepare("PRAGMA table_info(staking_history)")?;
        let columns = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;

        let mut reward_type = None;
        let mut bonded_type = None;
        for column in columns {
            let (name, column_type) = column?;
            match name.as_str() {
                "reward" => reward_type = Some(column_type),
                "bonded" => bonded_type = Some(column_type),
                _ => {}
            }
        }

        if reward_type.as_deref() == Some("TEXT") && bonded_type.as_deref() == Some("TEXT") {
            return Ok(());
        }

        let result = self.conn.execute_batch(
            r#"
            BEGIN IMMEDIATE;

            DROP INDEX IF EXISTS idx_staking_history_lookup;
            DROP INDEX IF EXISTS idx_staking_history_era;

            ALTER TABLE staking_history RENAME TO staking_history_old;

            CREATE TABLE staking_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                network TEXT NOT NULL,
                address TEXT NOT NULL,
                era INTEGER NOT NULL,
                date TEXT,
                reward TEXT NOT NULL,
                bonded TEXT NOT NULL,
                apy REAL NOT NULL,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(network, address, era)
            );

            INSERT INTO staking_history
                (id, network, address, era, date, reward, bonded, apy, created_at)
            SELECT id, network, address, era, date, CAST(reward AS TEXT), CAST(bonded AS TEXT), apy, created_at
            FROM staking_history_old;

            DROP TABLE staking_history_old;

            CREATE INDEX IF NOT EXISTS idx_staking_history_lookup
                ON staking_history(network, address, era);
            CREATE INDEX IF NOT EXISTS idx_staking_history_era
                ON staking_history(network, address, era DESC);

            COMMIT;
            "#,
        );

        if let Err(err) = result {
            let _ = self.conn.execute_batch("ROLLBACK;");
            return Err(err);
        }

        Ok(())
    }

    fn migrate_cached_pools_total_bonded_to_text(&mut self) -> Result<()> {
        let total_bonded_type = {
            let mut stmt = self.conn.prepare("PRAGMA table_info(cached_pools)")?;
            let columns = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })?;

            let mut t = None;
            for column in columns {
                let (name, column_type) = column?;
                if name == "total_bonded" {
                    t = Some(column_type);
                }
            }
            t
        };

        if total_bonded_type.as_deref() == Some("TEXT") {
            return Ok(());
        }

        // Case 1: total_bonded doesn't exist at all in old schema — just add it
        if total_bonded_type.is_none() {
            return self.conn.execute_batch(
                r#"
                ALTER TABLE cached_pools ADD COLUMN total_bonded TEXT NOT NULL DEFAULT '0';
                "#,
            );
        }

        // Case 2: total_bonded exists but is not TEXT — migrate via recreate
        let tx = self.conn.transaction()?;
        tx.execute("ALTER TABLE cached_pools RENAME TO cached_pools_old", [])?;
        tx.execute_batch(
            r#"
            CREATE TABLE cached_pools (
                network TEXT NOT NULL,
                id INTEGER NOT NULL,
                name TEXT NOT NULL,
                state TEXT NOT NULL,
                member_count INTEGER NOT NULL,
                total_bonded TEXT NOT NULL,
                commission REAL,
                apy REAL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, id)
            );
            "#,
        )?;
        let rows_inserted = tx.execute(
            r#"
            INSERT INTO cached_pools
                (network, id, name, state, member_count, total_bonded, commission, apy, updated_at)
            SELECT network, id, name, state, member_count, CAST(total_bonded AS TEXT), commission, apy, updated_at
            FROM cached_pools_old
            "#,
            [],
        )?;
        eprintln!("Migration inserted {} rows", rows_inserted);
        tx.execute("DROP TABLE cached_pools_old", [])?;
        tx.commit()?;

        Ok(())
    }

    fn migrate_cached_account_status_balances_to_text(&self) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare("PRAGMA table_info(cached_account_status)")?;
        let columns = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;

        let mut free_balance_type = None;
        let mut reserved_balance_type = None;
        let mut frozen_balance_type = None;
        let mut staked_amount_type = None;
        let mut pool_points_type = None;
        for column in columns {
            let (name, column_type) = column?;
            match name.as_str() {
                "free_balance" => free_balance_type = Some(column_type),
                "reserved_balance" => reserved_balance_type = Some(column_type),
                "frozen_balance" => frozen_balance_type = Some(column_type),
                "staked_amount" => staked_amount_type = Some(column_type),
                "pool_points" => pool_points_type = Some(column_type),
                _ => {}
            }
        }

        if free_balance_type.as_deref() == Some("TEXT")
            && reserved_balance_type.as_deref() == Some("TEXT")
            && frozen_balance_type.as_deref() == Some("TEXT")
            && staked_amount_type.as_deref() == Some("TEXT")
            && pool_points_type.as_deref() == Some("TEXT")
        {
            return Ok(());
        }

        let result = self.conn.execute_batch(
            r#"
            BEGIN IMMEDIATE;

            ALTER TABLE cached_account_status RENAME TO cached_account_status_old;

            CREATE TABLE cached_account_status (
                network TEXT NOT NULL,
                address TEXT NOT NULL,
                free_balance TEXT NOT NULL DEFAULT '0',
                reserved_balance TEXT NOT NULL DEFAULT '0',
                frozen_balance TEXT NOT NULL DEFAULT '0',
                staked_amount TEXT NOT NULL DEFAULT '0',
                nominations_json TEXT,
                pool_id INTEGER,
                pool_points TEXT,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, address)
            );

            INSERT INTO cached_account_status
                (network, address, free_balance, reserved_balance, frozen_balance,
                 staked_amount, nominations_json, pool_id, pool_points, updated_at)
            SELECT network, address, CAST(free_balance AS TEXT), CAST(reserved_balance AS TEXT),
                   CAST(frozen_balance AS TEXT), CAST(staked_amount AS TEXT),
                   nominations_json, pool_id, CAST(pool_points AS TEXT), updated_at
            FROM cached_account_status_old;

            DROP TABLE cached_account_status_old;

            COMMIT;
            "#,
        );

        if let Err(err) = result {
            let _ = self.conn.execute_batch("ROLLBACK;");
            return Err(err);
        }

        Ok(())
    }

    // ==================== Staking History ====================

    /// Store a single staking history point.
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
                point.reward.to_string(),
                point.bonded.to_string(),
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
                    point.reward.to_string(),
                    point.bonded.to_string(),
                    point.apy,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Get history for an address, ordered by era ascending (oldest first).
    pub fn get_history(
        &self,
        network: Network,
        address: &str,
        limit: Option<u32>,
    ) -> Result<Vec<StakingHistoryPoint>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT era, date, reward, bonded, apy
            FROM staking_history
            WHERE network = ?1 AND address = ?2
            ORDER BY era ASC
            LIMIT ?3
            "#,
        )?;

        let limit_val = limit.unwrap_or(u32::MAX);
        let rows = stmt.query_map(params![network.to_string(), address, limit_val], |row| {
            Ok(StakingHistoryPoint {
                era: row.get(0)?,
                date: row.get(1)?,
                reward: read_u128(row, 2)?,
                bonded: read_u128(row, 3)?,
                apy: row.get(4)?,
            })
        })?;

        let mut points = Vec::new();
        for row in rows {
            points.push(row?);
        }
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
    pub fn delete_address_history(&self, address: &str) -> Result<u32> {
        let deleted = self.conn.execute(
            "DELETE FROM staking_history WHERE address = ?1",
            params![address],
        )?;
        Ok(deleted as u32)
    }

    /// Delete old history entries beyond a certain count.
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

    // ==================== Validator Identities ====================

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

    /// Store or update a single validator identity.
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
    /// Validator names are populated from the validator_identities table via JOIN.
    pub fn get_cached_validators(&self, network: Network) -> Result<Vec<DisplayValidator>> {
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
            let apy_value: Option<f64> = row.get(7)?;
            Ok(DisplayValidator {
                address: row.get(0)?,
                name: if name_str.is_empty() {
                    None
                } else {
                    Some(name_str)
                },
                commission: row.get(1)?,
                blocked: row.get::<_, i32>(2)? != 0,
                total_stake: read_u128(row, 3)?,
                own_stake: read_u128(row, 4)?,
                nominator_count: row.get(5)?,
                points: row.get(6)?,
                apy: apy_value,
            })
        })?;

        let mut validators = Vec::new();
        for row in rows {
            validators.push(row?);
        }
        Ok(validators)
    }

    /// Store validators in the cache (replaces existing validators for this network).
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
                    v.total_stake.to_string(),
                    v.own_stake.to_string(),
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

    /// Get count of cached validators for a network.
    pub fn count_cached_validators(&self, network: Network) -> Result<u32> {
        let mut stmt = self
            .conn
            .prepare("SELECT COUNT(*) FROM cached_validators WHERE network = ?1")?;
        let count: u32 = stmt.query_row(params![network.to_string()], |row| row.get(0))?;
        Ok(count)
    }

    // ==================== Cached Pools ====================

    /// Get cached nomination pools for a network.
    pub fn get_cached_pools(&self, network: Network) -> Result<Vec<DisplayPool>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, name, state, member_count, total_bonded, commission, apy
            FROM cached_pools
            WHERE network = ?1
            "#,
        )?;

        let rows = stmt.query_map(params![network.to_string()], |row| {
            let state_str: String = row.get(2)?;
            let state = match state_str.as_str() {
                "Open" => PoolState::Open,
                "Blocked" => PoolState::Blocked,
                "Destroying" => PoolState::Destroying,
                _ => {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        format!("unknown pool state: {}", state_str).into(),
                    ));
                }
            };

            Ok(DisplayPool {
                id: row.get(0)?,
                name: row.get(1)?,
                state,
                member_count: row.get(3)?,
                total_bonded: read_u128(row, 4)?,
                commission: row.get(5)?,
                apy: row.get(6)?,
            })
        })?;

        let mut pools = Vec::new();
        for row in rows {
            pools.push(row?);
        }
        Ok(pools)
    }

    /// Store nomination pools in the cache (replaces existing pools for this network).
    pub fn set_cached_pools(&mut self, network: Network, pools: &[DisplayPool]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;
        {
            // Clear old pools for this network
            if let Err(e) = tx.execute(
                "DELETE FROM cached_pools WHERE network = ?1",
                params![network.to_string()],
            ) {
                // If the table doesn't exist, create it and continue
                if e.to_string().contains("no such table")
                    || e.to_string().contains("SQL error or missing database")
                {
                    tx.execute_batch(
                        r#"
                        CREATE TABLE IF NOT EXISTS cached_pools (
                            network TEXT NOT NULL,
                            id INTEGER NOT NULL,
                            name TEXT NOT NULL,
                            state TEXT NOT NULL,
                            member_count INTEGER NOT NULL,
                            total_bonded TEXT NOT NULL,
                            commission REAL,
                            apy REAL,
                            updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                            PRIMARY KEY (network, id)
                        );
                        "#,
                    )?;
                } else {
                    return Err(e);
                }
            }

            let mut stmt = tx.prepare(
                r#"
                INSERT INTO cached_pools
                    (network, id, name, state, member_count, total_bonded, commission, apy)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
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
                    p.total_bonded.to_string(),
                    p.commission,
                    p.apy,
                ])?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    /// Get count of cached pools for a network.
    pub fn count_cached_pools(&self, network: Network) -> Result<u32> {
        let mut stmt = self
            .conn
            .prepare("SELECT COUNT(*) FROM cached_pools WHERE network = ?1")?;
        let count: u32 = stmt.query_row(params![network.to_string()], |row| row.get(0))?;
        Ok(count)
    }

    // ==================== Chain Metadata ====================

    /// Get cached chain metadata for a network.
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
                era_duration_ms: row.get::<_, i64>(6)? as u64,
                current_era: row.get::<_, i64>(7)? as u32,
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
                meta.era_duration_ms as i64,
                meta.current_era as i64,
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
                free_balance: read_u128(row, 0)?,
                reserved_balance: read_u128(row, 1)?,
                frozen_balance: read_u128(row, 2)?,
                staked_amount: read_u128(row, 3)?,
                nominations_json: row.get(4)?,
                pool_id: row.get(5)?,
                pool_points: read_option_u128(row, 6)?,
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
                status.free_balance.to_string(),
                status.reserved_balance.to_string(),
                status.frozen_balance.to_string(),
                status.staked_amount.to_string(),
                &status.nominations_json,
                status.pool_id,
                status.pool_points.map(|p| p.to_string()),
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
            date: Some(format!("2025010{}", era % 10)),
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
            apy: Some(apy),
        }
    }

    fn make_test_pool(id: u32, state: PoolState) -> DisplayPool {
        DisplayPool {
            id,
            name: format!("Pool {}", id),
            state,
            member_count: 50,
            total_bonded: 500_000_000_000_000,
            commission: Some(0.03),
            apy: Some(0.12),
        }
    }

    // ==================== History Tests ====================

    #[test]
    fn test_insert_and_get_history() {
        let db = StakingDb::open_memory().unwrap();

        let point = StakingHistoryPoint::new(
            1500,
            "20250101".to_string(),
            1_000_000_000_000,
            100_000_000_000_000,
            0.15,
        );

        db.insert_history(Network::Polkadot, "15oF4uV", &point)
            .unwrap();

        let history = db.get_history(Network::Polkadot, "15oF4uV", None).unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].era, 1500);
        assert_eq!(history[0].reward, 1_000_000_000_000);
        assert_eq!(history[0].date, Some("20250101".to_string()));
    }

    #[test]
    fn test_insert_history_without_date() {
        let db = StakingDb::open_memory().unwrap();

        let point = StakingHistoryPoint::new_without_date(
            1500,
            1_000_000_000_000,
            100_000_000_000_000,
            0.15,
        );

        db.insert_history(Network::Polkadot, "15oF4uV", &point)
            .unwrap();

        let history = db.get_history(Network::Polkadot, "15oF4uV", None).unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].era, 1500);
        assert!(history[0].date.is_none());
    }

    #[test]
    fn test_insert_history_batch() {
        let mut db = StakingDb::open_memory().unwrap();
        let points: Vec<_> = (0..5).map(make_test_point).collect();

        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let history = db.get_history(Network::Polkadot, "addr1", None).unwrap();
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn test_get_missing_eras() {
        let mut db = StakingDb::open_memory().unwrap();

        let points = vec![make_test_point(1500), make_test_point(1502)];

        db.insert_history_batch(Network::Polkadot, "15oF4uV", &points)
            .unwrap();

        let missing = db
            .get_missing_eras(Network::Polkadot, "15oF4uV", 1499, 1503)
            .unwrap();

        assert_eq!(missing, vec![1499, 1501, 1503]);
    }

    #[test]
    fn test_get_history_with_limit() {
        let mut db = StakingDb::open_memory().unwrap();
        let points: Vec<_> = (1..=10).map(|i| make_test_point(1500 + i)).collect();
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let history = db.get_history(Network::Polkadot, "addr1", Some(3)).unwrap();
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_get_history_empty() {
        let db = StakingDb::open_memory().unwrap();
        let history = db
            .get_history(Network::Polkadot, "nonexistent", None)
            .unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn test_get_latest_era() {
        let mut db = StakingDb::open_memory().unwrap();
        let points = vec![
            make_test_point(1500),
            make_test_point(1505),
            make_test_point(1503),
        ];
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let latest = db.get_latest_era(Network::Polkadot, "addr1").unwrap();
        assert_eq!(latest, Some(1505));
    }

    #[test]
    fn test_get_latest_era_empty() {
        let db = StakingDb::open_memory().unwrap();
        let latest = db.get_latest_era(Network::Polkadot, "nonexistent").unwrap();
        assert!(latest.is_none());
    }

    #[test]
    fn test_count_history() {
        let mut db = StakingDb::open_memory().unwrap();
        let points: Vec<_> = (0..5).map(make_test_point).collect();
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let count = db.count_history(Network::Polkadot, "addr1").unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn test_count_history_empty() {
        let db = StakingDb::open_memory().unwrap();
        let count = db.count_history(Network::Polkadot, "nonexistent").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_delete_address_history() {
        let mut db = StakingDb::open_memory().unwrap();
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
        let mut db = StakingDb::open_memory().unwrap();
        let points: Vec<_> = (0..10).map(make_test_point).collect();
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let deleted = db.prune_history(Network::Polkadot, "addr1", 3).unwrap();
        assert_eq!(deleted, 7);

        let count = db.count_history(Network::Polkadot, "addr1").unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_insert_or_replace_history() {
        let db = StakingDb::open_memory().unwrap();

        let point1 = StakingHistoryPoint::new(
            1500,
            "20250101".to_string(),
            1_000_000_000_000,
            100_000_000_000_000,
            0.15,
        );
        db.insert_history(Network::Polkadot, "addr1", &point1)
            .unwrap();

        // Insert again with different values - should replace
        let point2 = StakingHistoryPoint::new(
            1500,
            "20250101".to_string(),
            2_000_000_000_000,
            100_000_000_000_000,
            0.16,
        );
        db.insert_history(Network::Polkadot, "addr1", &point2)
            .unwrap();

        let history = db.get_history(Network::Polkadot, "addr1", None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].reward, 2_000_000_000_000);
        assert_eq!(history[0].apy, 0.16);
    }

    #[test]
    fn test_history_preserves_u128() {
        let db = StakingDb::open_memory().unwrap();

        let point = StakingHistoryPoint::new(
            1500,
            "20250101".to_string(),
            i64::MAX as u128 + 42,
            i64::MAX as u128 + 7,
            0.15,
        );
        db.insert_history(Network::Polkadot, "addr1", &point)
            .unwrap();

        let history = db.get_history(Network::Polkadot, "addr1", None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].reward, i64::MAX as u128 + 42);
        assert_eq!(history[0].bonded, i64::MAX as u128 + 7);
    }

    // ==================== Validator Identity Tests ====================

    #[test]
    fn test_validator_identities() {
        let mut db = StakingDb::open_memory().unwrap();

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
        let mut db = StakingDb::open_memory().unwrap();
        let mut batch = HashMap::new();
        batch.insert("val1".to_string(), "V1".to_string());
        batch.insert("val2".to_string(), "V2".to_string());
        db.set_validator_identities_batch(Network::Polkadot, &batch)
            .unwrap();

        let count = db.count_validator_identities(Network::Polkadot).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_validator_identity_update() {
        let db = StakingDb::open_memory().unwrap();

        db.set_validator_identity(Network::Polkadot, "val1", "Name 1")
            .unwrap();
        db.set_validator_identity(Network::Polkadot, "val1", "Name 2")
            .unwrap();

        let identities = db.get_validator_identities(Network::Polkadot).unwrap();
        assert_eq!(identities.len(), 1);
        assert_eq!(identities.get("val1"), Some(&"Name 2".to_string()));
    }

    // ==================== Cached Validators Tests ====================

    #[test]
    fn test_cached_validators() {
        let mut db = StakingDb::open_memory().unwrap();
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
        let mut db = StakingDb::open_memory().unwrap();
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
        let mut db = StakingDb::open_memory().unwrap();
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
    fn test_cached_validators_preserves_u128_stake() {
        let mut db = StakingDb::open_memory().unwrap();
        let mut validator = make_test_validator("val1", 0.15);
        validator.total_stake = i64::MAX as u128 + 42;
        validator.own_stake = i64::MAX as u128 + 7;

        db.set_cached_validators(Network::Polkadot, 1500, &[validator.clone()])
            .unwrap();

        let cached = db.get_cached_validators(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].total_stake, validator.total_stake);
        assert_eq!(cached[0].own_stake, validator.own_stake);
    }

    #[test]
    fn test_cached_validators_migration_drops_negative_legacy_stakes() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE cached_validators (
                network TEXT NOT NULL,
                address TEXT NOT NULL,
                commission REAL NOT NULL,
                blocked INTEGER NOT NULL DEFAULT 0,
                total_stake INTEGER NOT NULL,
                own_stake INTEGER NOT NULL,
                nominator_count INTEGER NOT NULL,
                points INTEGER NOT NULL DEFAULT 0,
                apy REAL,
                era INTEGER NOT NULL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, address)
            );

            INSERT INTO cached_validators
                (network, address, commission, blocked, total_stake, own_stake,
                 nominator_count, points, apy, era)
            VALUES
                ('Polkadot', 'valid', 0.05, 0, 1000, 100, 10, 1, 0.12, 1500),
                ('Polkadot', 'invalid-total', 0.05, 0, -1, 100, 10, 1, 0.13, 1500),
                ('Polkadot', 'invalid-own', 0.05, 0, 1000, -1, 10, 1, 0.14, 1500);
            "#,
        )
        .unwrap();

        let mut db = StakingDb { conn };
        db.init_schema().unwrap();

        let cached = db.get_cached_validators(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].address, "valid");
        assert_eq!(cached[0].total_stake, 1000);
        assert_eq!(cached[0].own_stake, 100);
    }

    #[test]
    fn test_cached_validators_with_identity_join() {
        let mut db = StakingDb::open_memory().unwrap();

        // Set up identity
        db.set_validator_identity(Network::Polkadot, "val1", "My Validator")
            .unwrap();

        // Set up validator without name
        let validators = vec![DisplayValidator {
            address: "val1".to_string(),
            name: None,
            commission: 0.05,
            blocked: false,
            total_stake: 1_000_000,
            own_stake: 100_000,
            nominator_count: 10,
            points: 100,
            apy: Some(0.15),
        }];
        db.set_cached_validators(Network::Polkadot, 1500, &validators)
            .unwrap();

        let cached = db.get_cached_validators(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].name, Some("My Validator".to_string()));
    }

    #[test]
    fn test_blocked_validator_storage() {
        let mut db = StakingDb::open_memory().unwrap();

        let validators = vec![DisplayValidator {
            address: "val1".to_string(),
            name: None,
            commission: 0.05,
            blocked: true, // Blocked validator
            total_stake: 1_000_000,
            own_stake: 100_000,
            nominator_count: 10,
            points: 100,
            apy: Some(0.15),
        }];
        db.set_cached_validators(Network::Polkadot, 1500, &validators)
            .unwrap();

        let cached = db.get_cached_validators(Network::Polkadot).unwrap();
        assert!(cached[0].blocked);
    }

    // ==================== Cached Pools Tests ====================

    #[test]
    fn test_cached_pools() {
        let mut db = StakingDb::open_memory().unwrap();
        let pools = vec![
            make_test_pool(1, PoolState::Open),
            make_test_pool(2, PoolState::Blocked),
            make_test_pool(3, PoolState::Destroying),
        ];

        let count = db.set_cached_pools(Network::Polkadot, &pools).unwrap();
        assert_eq!(count, 3);

        let cached = db.get_cached_pools(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 3);
    }

    #[test]
    fn test_cached_pools_state_serialization() {
        let mut db = StakingDb::open_memory().unwrap();
        let pools = vec![
            make_test_pool(1, PoolState::Open),
            make_test_pool(2, PoolState::Blocked),
            make_test_pool(3, PoolState::Destroying),
        ];

        db.set_cached_pools(Network::Polkadot, &pools).unwrap();

        let cached = db.get_cached_pools(Network::Polkadot).unwrap();
        assert_eq!(cached[0].state, PoolState::Open);
        assert_eq!(cached[1].state, PoolState::Blocked);
        assert_eq!(cached[2].state, PoolState::Destroying);
    }

    #[test]
    fn test_cached_pools_preserves_u128() {
        let mut db = StakingDb::open_memory().unwrap();
        let mut pool = make_test_pool(1, PoolState::Open);
        pool.total_bonded = i64::MAX as u128 + 42;

        db.set_cached_pools(Network::Polkadot, &[pool.clone()])
            .unwrap();

        let cached = db.get_cached_pools(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].total_bonded, pool.total_bonded);
    }

    #[test]
    fn test_cached_pools_replaces_old() {
        let mut db = StakingDb::open_memory().unwrap();
        let pools1 = vec![make_test_pool(1, PoolState::Open)];
        db.set_cached_pools(Network::Polkadot, &pools1).unwrap();

        let pools2 = vec![
            make_test_pool(2, PoolState::Blocked),
            make_test_pool(3, PoolState::Destroying),
        ];
        db.set_cached_pools(Network::Polkadot, &pools2).unwrap();

        let cached = db.get_cached_pools(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 2);
        assert!(!cached.iter().any(|p| p.id == 1));
    }

    #[test]
    fn test_count_cached_pools() {
        let mut db = StakingDb::open_memory().unwrap();
        let pools = vec![
            make_test_pool(1, PoolState::Open),
            make_test_pool(2, PoolState::Blocked),
        ];
        db.set_cached_pools(Network::Polkadot, &pools).unwrap();

        let count = db.count_cached_pools(Network::Polkadot).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_cached_pools_empty() {
        let db = StakingDb::open_memory().unwrap();
        let cached = db.get_cached_pools(Network::Polkadot).unwrap();
        assert!(cached.is_empty());
    }

    // ==================== Chain Metadata Tests ====================

    #[test]
    fn test_chain_metadata() {
        let db = StakingDb::open_memory().unwrap();

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
        let db = StakingDb::open_memory().unwrap();
        let meta = db.get_chain_metadata(Network::Polkadot).unwrap();
        assert!(meta.is_none());
    }

    // ==================== Account Status Tests ====================

    #[test]
    fn test_cached_account_status() {
        let db = StakingDb::open_memory().unwrap();

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
        let db = StakingDb::open_memory().unwrap();
        let status = db
            .get_cached_account_status(Network::Polkadot, "nonexistent")
            .unwrap();
        assert!(status.is_none());
    }

    #[test]
    fn test_cached_account_status_without_optional_fields() {
        let db = StakingDb::open_memory().unwrap();

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
    fn test_cached_account_status_preserves_u128() {
        let db = StakingDb::open_memory().unwrap();

        let status = CachedAccountStatus {
            free_balance: i64::MAX as u128 + 1,
            reserved_balance: i64::MAX as u128 + 2,
            frozen_balance: i64::MAX as u128 + 3,
            staked_amount: i64::MAX as u128 + 4,
            nominations_json: None,
            pool_id: Some(42),
            pool_points: Some(i64::MAX as u128 + 5),
        };

        db.set_cached_account_status(Network::Polkadot, "addr1", &status)
            .unwrap();

        let loaded = db
            .get_cached_account_status(Network::Polkadot, "addr1")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.free_balance, status.free_balance);
        assert_eq!(loaded.reserved_balance, status.reserved_balance);
        assert_eq!(loaded.frozen_balance, status.frozen_balance);
        assert_eq!(loaded.staked_amount, status.staked_amount);
        assert_eq!(loaded.pool_points, status.pool_points);
    }

    // ==================== Network Isolation Tests ====================

    #[test]
    fn test_network_isolation_history() {
        let db = StakingDb::open_memory().unwrap();

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

    #[test]
    fn test_network_isolation_validators() {
        let mut db = StakingDb::open_memory().unwrap();

        let polkadot_validators = vec![make_test_validator("pol_val", 0.15)];
        let kusama_validators = vec![make_test_validator("ksm_val", 0.12)];

        db.set_cached_validators(Network::Polkadot, 1500, &polkadot_validators)
            .unwrap();
        db.set_cached_validators(Network::Kusama, 6000, &kusama_validators)
            .unwrap();

        let polkadot_cached = db.get_cached_validators(Network::Polkadot).unwrap();
        let kusama_cached = db.get_cached_validators(Network::Kusama).unwrap();

        assert_eq!(polkadot_cached.len(), 1);
        assert_eq!(polkadot_cached[0].address, "pol_val");
        assert_eq!(kusama_cached.len(), 1);
        assert_eq!(kusama_cached[0].address, "ksm_val");
    }

    // ==================== Clone/Debug Tests ====================

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
    fn test_cached_account_status_default() {
        let status = CachedAccountStatus::default();
        assert_eq!(status.free_balance, 0);
        assert_eq!(status.staked_amount, 0);
        assert!(status.nominations_json.is_none());
        assert!(status.pool_id.is_none());
    }

    /// Simulate a DB created with the old schema (total_bonded INTEGER)
    /// and verify init_schema migrates it correctly.
    #[test]
    fn test_cached_pools_migration_from_integer() {
        use rusqlite::Connection;

        let conn = Connection::open_in_memory().unwrap();
        // Create old schema
        conn.execute_batch(
            r#"
            CREATE TABLE cached_pools (
                network TEXT NOT NULL,
                id INTEGER NOT NULL,
                name TEXT NOT NULL,
                state TEXT NOT NULL,
                member_count INTEGER NOT NULL,
                total_bonded INTEGER NOT NULL,
                commission REAL,
                apy REAL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, id)
            );
            INSERT INTO cached_pools (network, id, name, state, member_count, total_bonded, commission, apy)
            VALUES ('Polkadot', 1, 'Old Pool', 'Open', 100, 500000000000000, 0.1, 0.15);
            "#,
        )
        .unwrap();

        // Check table info before init_schema
        {
            let mut stmt = conn.prepare("PRAGMA table_info(cached_pools)").unwrap();
            let cols: Vec<(String, String)> = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            eprintln!("Columns before init_schema: {:?}", cols);
        }

        let mut db = StakingDb { conn };
        db.init_schema().unwrap();

        // Check what we have after init_schema
        let raw_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM cached_pools", [], |r| r.get(0))
            .unwrap();
        eprintln!("Raw count after init_schema: {}", raw_count);
        let cached_before = db.get_cached_pools(Network::Polkadot).unwrap();
        eprintln!("Pools after init_schema: {}", cached_before.len());
        for p in &cached_before {
            eprintln!(
                "  - id={}, name={}, total_bonded={}",
                p.id, p.name, p.total_bonded
            );
        }

        // Check table info after init_schema
        {
            let mut stmt = db.conn.prepare("PRAGMA table_info(cached_pools)").unwrap();
            let cols: Vec<(String, String)> = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            eprintln!("Columns after init_schema: {:?}", cols);
        }
        // Check if old table still exists
        let old_exists: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='cached_pools_old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        eprintln!("cached_pools_old exists: {}", old_exists);

        // Should be able to insert with TEXT total_bonded after migration
        let pools = vec![make_test_pool(2, PoolState::Blocked)];
        db.set_cached_pools(Network::Polkadot, &pools).unwrap();

        let cached = db.get_cached_pools(Network::Polkadot).unwrap();
        eprintln!("Pools after set_cached_pools: {}", cached.len());
        for p in &cached {
            eprintln!(
                "  - id={}, name={}, total_bonded={}",
                p.id, p.name, p.total_bonded
            );
        }
        // set_cached_pools deletes old pools for the network, so only new pools remain
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].id, 2);
    }
}
