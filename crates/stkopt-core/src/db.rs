//! SQLite database for caching staking history, validator data, and chain metadata.
//!
//! This module provides a unified database layer used by both TUI and GPUI frontends.
//! The schema supports all features from both implementations.

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension, Result, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use crate::display::{DisplayPool, DisplayValidator, StakingHistoryPoint};
use crate::types::{Network, PoolState};

const SCHEMA_VERSION: i32 = 5;
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum age for startup validator/pool caches.
pub const DEFAULT_STARTUP_CACHE_MAX_AGE_SECS: i64 = 24 * 60 * 60;
/// Maximum era lag accepted for era-sensitive startup caches.
pub const DEFAULT_STARTUP_CACHE_MAX_ERA_LAG: u32 = 1;
/// Maximum age for account status read-through cache.
pub const DEFAULT_ACCOUNT_CACHE_MAX_AGE_SECS: i64 = 5 * 60;
/// Maximum APY accepted for cached history points.
pub const DEFAULT_HISTORY_MAX_APY: f64 = 0.50;
/// Maximum age for cached validator identities before they are considered stale.
pub const DEFAULT_IDENTITY_MAX_AGE_SECS: i64 = 24 * 60 * 60;

/// Shared cache policy used by the UI frontends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CachePolicy {
    /// Maximum age for validator and pool snapshots shown at startup.
    pub startup_max_age_secs: i64,
    /// Maximum era lag for validator and pool snapshots to suppress refresh.
    pub startup_max_era_lag: u32,
    /// Maximum age for account status read-through cache.
    pub account_max_age_secs: i64,
    /// Maximum cached history APY before a point is treated as missing.
    pub history_max_apy: f64,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            startup_max_age_secs: DEFAULT_STARTUP_CACHE_MAX_AGE_SECS,
            startup_max_era_lag: DEFAULT_STARTUP_CACHE_MAX_ERA_LAG,
            account_max_age_secs: DEFAULT_ACCOUNT_CACHE_MAX_AGE_SECS,
            history_max_apy: DEFAULT_HISTORY_MAX_APY,
        }
    }
}

/// Cache resource kinds tracked in `cache_snapshots`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    /// Cached validator display data.
    Validators,
    /// Cached nomination pool display data.
    Pools,
}

impl CacheKind {
    fn as_str(self) -> &'static str {
        match self {
            CacheKind::Validators => "validators",
            CacheKind::Pools => "pools",
        }
    }
}

/// Metadata for one cached resource snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheSnapshot {
    /// Era observed when the snapshot was written.
    pub era: u32,
    /// Whether the writer considered the snapshot complete.
    pub complete: bool,
    /// Number of rows written for the snapshot.
    pub row_count: u32,
    /// Snapshot age according to SQLite's clock.
    pub age_seconds: i64,
    /// Genesis hash observed when the snapshot was written.
    pub genesis_hash: Option<String>,
    /// Runtime spec version observed when the snapshot was written.
    pub spec_version: Option<u32>,
}

impl CacheSnapshot {
    /// Return true if the snapshot is recent enough for pre-connection UI paint.
    pub fn is_recent(&self, max_age_secs: i64) -> bool {
        self.row_count > 0 && self.age_seconds <= max_age_secs
    }

    /// Return true if the snapshot is fresh for a known active era.
    ///
    /// A `current_era` of zero means the real active era is not yet known, so
    /// the snapshot cannot be considered fresh for a specific era.
    pub fn is_fresh_for_era(&self, current_era: u32, max_age_secs: i64, max_era_lag: u32) -> bool {
        if current_era == 0 {
            return false;
        }

        if !self.is_recent(max_age_secs) {
            return false;
        }

        self.era > 0 && self.era.saturating_add(max_era_lag) >= current_era
    }

    /// Return true when snapshot runtime metadata matches current chain metadata.
    ///
    /// If no current metadata is available, the snapshot cannot be validated as
    /// matching, so this returns false.
    pub fn matches_chain_metadata(&self, current: &CachedChainMetadata) -> bool {
        self.genesis_hash.as_deref() == Some(current.genesis_hash.as_str())
            && self.spec_version == Some(current.spec_version)
    }
}

/// Freshness state for cached resources used during startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheFreshness {
    /// No usable cached data exists.
    Missing,
    /// Cached data can be displayed, but a live refresh is still needed.
    Stale,
    /// Cached data is recent enough and fresh for the current era.
    Fresh,
}

/// Cached data together with the metadata that determined whether to refresh.
#[derive(Debug, Clone, PartialEq)]
pub struct Cached<T> {
    /// Cached data. Empty when `freshness` is `Missing`.
    pub data: T,
    /// Snapshot metadata, if one exists.
    pub snapshot: Option<CacheSnapshot>,
    /// Freshness classification under the cache policy.
    pub freshness: CacheFreshness,
}

impl<T> Cached<T> {
    /// Return true when callers should fetch fresh chain data.
    pub fn needs_refresh(&self) -> bool {
        self.freshness != CacheFreshness::Fresh
    }

    /// Return true when this cache read has displayable data.
    pub fn is_displayable(&self) -> bool {
        self.freshness != CacheFreshness::Missing
    }
}

/// Startup cache reads for validators and pools.
#[derive(Debug, Clone, PartialEq)]
pub struct StartupDataCache {
    /// Cached validators plus snapshot freshness.
    pub validators: Cached<Vec<DisplayValidator>>,
    /// Cached pools plus snapshot freshness.
    pub pools: Cached<Vec<DisplayPool>>,
}

/// Shared startup data cache service.
pub struct StartupDataService;

/// Shared account status cache service.
pub struct AccountStatusService;

/// Shared history cache service.
pub struct HistoryService;

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
    /// JSON-encoded list of staking ledger unlocking chunks.
    pub unlocking_json: Option<String>,
    /// JSON-encoded list of pool member unbonding eras and balances.
    pub pool_unbonding_eras_json: Option<String>,
    /// Last recorded reward counter for pool membership.
    pub pool_last_recorded_reward_counter: u128,
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
        Self::configure_connection(&conn, true)?;
        let mut db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Open an in-memory database (for testing).
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure_connection(&conn, false)?;
        let mut db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn configure_connection(conn: &Connection, enable_wal: bool) -> Result<()> {
        conn.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        if enable_wal {
            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
        }
        Ok(())
    }

    /// Initialize the database schema.
    fn init_schema(&mut self) -> Result<()> {
        self.ensure_base_schema()?;

        let user_version: i32 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;

        // Clean up any orphaned tables from failed previous migrations.
        self.conn.execute_batch(
            r#"
            DROP TABLE IF EXISTS cached_pools_old;
            DROP TABLE IF EXISTS cached_validators_old;
            DROP TABLE IF EXISTS staking_history_old;
            DROP TABLE IF EXISTS cached_account_status_old;
            "#,
        )?;

        if user_version < 2 {
            self.migrate_to_v2_text_balances()?;
            self.conn.execute_batch("PRAGMA user_version = 2;")?;
        } else {
            // Validate critical schemas regardless of user_version, to handle
            // DBs created by intermediate builds that already bumped the pragma.
            self.migrate_to_v2_text_balances()?;
        }

        if user_version < 3 {
            self.migrate_to_v3_cache_snapshots()?;
            self.conn.execute_batch("PRAGMA user_version = 3;")?;
        } else {
            self.migrate_to_v3_cache_snapshots()?;
        }

        if user_version < 4 {
            self.migrate_to_v4_cache_snapshot_metadata()?;
            self.conn.execute_batch("PRAGMA user_version = 4;")?;
        } else {
            self.migrate_to_v4_cache_snapshot_metadata()?;
        }

        if user_version < 5 {
            self.migrate_to_v5_account_status_unlocking_and_pool()?;
            self.conn.execute_batch("PRAGMA user_version = 5;")?;
        } else {
            self.migrate_to_v5_account_status_unlocking_and_pool()?;
        }

        // SCHEMA_VERSION (5) is the highest migration this build knows about.
        // If user_version is newer we do not migrate down or bump the pragma,
        // but the idempotent schema checks above already created any missing
        // tables/columns known to this build.
        let _ = (user_version, SCHEMA_VERSION);
        Ok(())
    }

    fn ensure_base_schema(&self) -> Result<()> {
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
                unlocking_json TEXT,
                pool_unbonding_eras_json TEXT,
                pool_last_recorded_reward_counter TEXT NOT NULL DEFAULT '0',
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

            -- Per-resource cache snapshot metadata used for freshness checks.
            CREATE TABLE IF NOT EXISTS cache_snapshots (
                network TEXT NOT NULL,
                kind TEXT NOT NULL,
                era INTEGER NOT NULL DEFAULT 0,
                complete INTEGER NOT NULL DEFAULT 1,
                row_count INTEGER NOT NULL DEFAULT 0,
                genesis_hash TEXT,
                spec_version INTEGER,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, kind)
            );
            "#,
        )
    }

    fn migrate_to_v2_text_balances(&mut self) -> Result<()> {
        self.migrate_cached_pools_total_bonded_to_text()?;
        self.migrate_cached_validator_stakes_to_text()?;
        self.purge_invalid_cached_validator_stakes()?;
        self.migrate_staking_history_balances_to_text()?;
        self.migrate_cached_account_status_balances_to_text()?;
        Ok(())
    }

    fn migrate_to_v3_cache_snapshots(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS cache_snapshots (
                network TEXT NOT NULL,
                kind TEXT NOT NULL,
                era INTEGER NOT NULL DEFAULT 0,
                complete INTEGER NOT NULL DEFAULT 1,
                row_count INTEGER NOT NULL DEFAULT 0,
                genesis_hash TEXT,
                spec_version INTEGER,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, kind)
            );

            INSERT OR IGNORE INTO cache_snapshots
                (network, kind, era, complete, row_count, updated_at)
            SELECT network, 'validators', COALESCE(MAX(era), 0), 1, COUNT(*),
                   COALESCE(MAX(updated_at), CURRENT_TIMESTAMP)
            FROM cached_validators
            GROUP BY network;

            INSERT OR IGNORE INTO cache_snapshots
                (network, kind, era, complete, row_count, updated_at)
            SELECT p.network, 'pools', COALESCE(m.current_era, 0), 1, COUNT(*),
                   COALESCE(MAX(p.updated_at), CURRENT_TIMESTAMP)
            FROM cached_pools p
            LEFT JOIN chain_metadata m ON p.network = m.network
            GROUP BY p.network;
            "#,
        )?;
        Ok(())
    }

    fn table_has_column(&self, table: &str, column: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info({})", table))?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for name in columns {
            if name? == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn migrate_to_v4_cache_snapshot_metadata(&self) -> Result<()> {
        if !self.table_has_column("cache_snapshots", "genesis_hash")? {
            self.conn
                .execute_batch("ALTER TABLE cache_snapshots ADD COLUMN genesis_hash TEXT;")?;
        }
        if !self.table_has_column("cache_snapshots", "spec_version")? {
            self.conn
                .execute_batch("ALTER TABLE cache_snapshots ADD COLUMN spec_version INTEGER;")?;
        }

        self.conn.execute_batch(
            r#"
            UPDATE cache_snapshots
            SET genesis_hash = (
                    SELECT chain_metadata.genesis_hash
                    FROM chain_metadata
                    WHERE chain_metadata.network = cache_snapshots.network
                ),
                spec_version = (
                    SELECT chain_metadata.spec_version
                    FROM chain_metadata
                    WHERE chain_metadata.network = cache_snapshots.network
                )
            WHERE genesis_hash IS NULL OR spec_version IS NULL;
            "#,
        )?;
        Ok(())
    }

    fn migrate_to_v5_account_status_unlocking_and_pool(&self) -> Result<()> {
        if !self.table_has_column("cached_account_status", "unlocking_json")? {
            self.conn.execute_batch(
                "ALTER TABLE cached_account_status ADD COLUMN unlocking_json TEXT;",
            )?;
        }
        if !self.table_has_column("cached_account_status", "pool_unbonding_eras_json")? {
            self.conn.execute_batch(
                "ALTER TABLE cached_account_status ADD COLUMN pool_unbonding_eras_json TEXT;",
            )?;
        }
        if !self.table_has_column("cached_account_status", "pool_last_recorded_reward_counter")? {
            self.conn.execute_batch(
                "ALTER TABLE cached_account_status ADD COLUMN pool_last_recorded_reward_counter TEXT NOT NULL DEFAULT '0';",
            )?;
        }
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
        let (total_bonded_type, has_points, has_apy) = {
            let mut stmt = self.conn.prepare("PRAGMA table_info(cached_pools)")?;
            let columns = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })?;

            let mut total_bonded_type = None;
            let mut has_points = false;
            let mut has_apy = false;
            for column in columns {
                let (name, column_type) = column?;
                match name.as_str() {
                    "total_bonded" => total_bonded_type = Some(column_type),
                    "points" => has_points = true,
                    "apy" => has_apy = true,
                    _ => {}
                }
            }
            (total_bonded_type, has_points, has_apy)
        };

        if !has_apy {
            self.conn
                .execute_batch("ALTER TABLE cached_pools ADD COLUMN apy REAL;")?;
        }

        if total_bonded_type.as_deref() == Some("TEXT") && !has_points {
            return Ok(());
        }

        let total_bonded_expr = if total_bonded_type.is_some() {
            "CAST(total_bonded AS TEXT)"
        } else if has_points {
            "CAST(points AS TEXT)"
        } else {
            "'0'"
        };

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
        tx.execute(
            &format!(
                r#"
            INSERT INTO cached_pools
                (network, id, name, state, member_count, total_bonded, commission, apy, updated_at)
            SELECT network, id, name, state, member_count, {}, commission, apy, updated_at
            FROM cached_pools_old
            "#,
                total_bonded_expr
            ),
            [],
        )?;
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
    ///
    /// When a limit is provided, returns the latest `limit` eras while preserving
    /// ascending display order.
    pub fn get_history(
        &self,
        network: Network,
        address: &str,
        limit: Option<u32>,
    ) -> Result<Vec<StakingHistoryPoint>> {
        let network = network.to_string();
        let mut points = Vec::new();

        if let Some(limit) = limit {
            let mut stmt = self.conn.prepare(
                r#"
                SELECT era, date, reward, bonded, apy
                FROM (
                    SELECT era, date, reward, bonded, apy
                    FROM staking_history
                    WHERE network = ?1 AND address = ?2
                    ORDER BY era DESC
                    LIMIT ?3
                )
                ORDER BY era ASC
                "#,
            )?;

            let rows = stmt.query_map(params![network, address, limit], |row| {
                Ok(StakingHistoryPoint {
                    era: row.get(0)?,
                    date: row.get(1)?,
                    reward: read_u128(row, 2)?,
                    bonded: read_u128(row, 3)?,
                    apy: row.get(4)?,
                })
            })?;

            for row in rows {
                points.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                r#"
                SELECT era, date, reward, bonded, apy
                FROM staking_history
                WHERE network = ?1 AND address = ?2
                ORDER BY era ASC
                "#,
            )?;

            let rows = stmt.query_map(params![network, address], |row| {
                Ok(StakingHistoryPoint {
                    era: row.get(0)?,
                    date: row.get(1)?,
                    reward: read_u128(row, 2)?,
                    bonded: read_u128(row, 3)?,
                    apy: row.get(4)?,
                })
            })?;

            for row in rows {
                points.push(row?);
            }
        }
        Ok(points)
    }

    /// Get history for a specific inclusive era range, ordered by era ascending.
    pub fn get_history_range(
        &self,
        network: Network,
        address: &str,
        from_era: u32,
        to_era: u32,
    ) -> Result<Vec<StakingHistoryPoint>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT era, date, reward, bonded, apy
            FROM staking_history
            WHERE network = ?1 AND address = ?2 AND era >= ?3 AND era <= ?4
            ORDER BY era ASC
            "#,
        )?;

        let rows = stmt.query_map(
            params![network.to_string(), address, from_era, to_era],
            |row| {
                Ok(StakingHistoryPoint {
                    era: row.get(0)?,
                    date: row.get(1)?,
                    reward: read_u128(row, 2)?,
                    bonded: read_u128(row, 3)?,
                    apy: row.get(4)?,
                })
            },
        )?;

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

    /// Get eras that are missing or whose cached APY is above the accepted maximum.
    pub fn get_missing_eras_with_max_apy(
        &self,
        network: Network,
        address: &str,
        from_era: u32,
        to_era: u32,
        max_apy: f64,
    ) -> Result<Vec<u32>> {
        let stored: std::collections::HashSet<u32> = {
            let mut stmt = self.conn.prepare(
                r#"
                SELECT era FROM staking_history
                WHERE network = ?1 AND address = ?2 AND era >= ?3 AND era <= ?4
                  AND apy >= 0.0 AND apy <= ?5
                "#,
            )?;

            let rows = stmt.query_map(
                params![network.to_string(), address, from_era, to_era, max_apy],
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

    /// Get cached validator identities for a network that are at most
    /// `max_age_secs` old.
    pub fn get_validator_identities_within_age(
        &self,
        network: Network,
        max_age_secs: i64,
    ) -> Result<HashMap<String, String>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT address, display_name
            FROM validator_identities
            WHERE network = ?1
              AND (strftime('%s', 'now') - strftime('%s', updated_at)) <= ?2
            "#,
        )?;

        let rows = stmt.query_map(params![network.to_string(), max_age_secs], |row| {
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

    fn snapshot_is_usable(
        snapshot: Option<CacheSnapshot>,
        current_era: Option<u32>,
        max_age_secs: i64,
        max_era_lag: u32,
    ) -> bool {
        let Some(snapshot) = snapshot else {
            return false;
        };

        match current_era {
            Some(current_era) => snapshot.is_fresh_for_era(current_era, max_age_secs, max_era_lag),
            None => snapshot.is_recent(max_age_secs),
        }
    }

    fn startup_freshness(
        snapshot: Option<&CacheSnapshot>,
        current_era: u32,
        current_metadata: Option<&CachedChainMetadata>,
        policy: CachePolicy,
    ) -> CacheFreshness {
        let Some(snapshot) = snapshot else {
            return CacheFreshness::Missing;
        };

        if !snapshot.is_recent(policy.startup_max_age_secs) {
            return CacheFreshness::Missing;
        }

        let metadata_matches = current_metadata
            .map(|metadata| snapshot.matches_chain_metadata(metadata))
            .unwrap_or(false);

        if snapshot.complete
            && snapshot.is_fresh_for_era(
                current_era,
                policy.startup_max_age_secs,
                policy.startup_max_era_lag,
            )
            && metadata_matches
        {
            CacheFreshness::Fresh
        } else {
            CacheFreshness::Stale
        }
    }

    /// Get cache snapshot metadata for one cache kind.
    pub fn get_cache_snapshot(
        &self,
        network: Network,
        kind: CacheKind,
    ) -> Result<Option<CacheSnapshot>> {
        let result = self.conn.query_row(
            r#"
            SELECT era, complete, row_count,
                   MAX(0, CAST((julianday('now') - julianday(updated_at)) * 86400 AS INTEGER)),
                   genesis_hash, spec_version
            FROM cache_snapshots
            WHERE network = ?1 AND kind = ?2
            "#,
            params![network.to_string(), kind.as_str()],
            |row| {
                Ok(CacheSnapshot {
                    era: row.get::<_, i64>(0)?.max(0) as u32,
                    complete: row.get::<_, i32>(1)? != 0,
                    row_count: row.get::<_, i64>(2)?.max(0) as u32,
                    age_seconds: row.get::<_, Option<i64>>(3)?.unwrap_or(i64::MAX),
                    genesis_hash: row.get(4)?,
                    spec_version: row
                        .get::<_, Option<i64>>(5)?
                        .map(|version| version.max(0) as u32),
                })
            },
        );

        match result {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn set_cache_snapshot_tx(
        tx: &rusqlite::Transaction<'_>,
        network: Network,
        kind: CacheKind,
        era: u32,
        complete: bool,
        row_count: usize,
    ) -> Result<()> {
        let metadata = tx
            .query_row(
                r#"
                SELECT genesis_hash, spec_version
                FROM chain_metadata
                WHERE network = ?1
                "#,
                params![network.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;

        tx.execute(
            r#"
            INSERT OR REPLACE INTO cache_snapshots
                (network, kind, era, complete, row_count, genesis_hash, spec_version, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)
            "#,
            params![
                network.to_string(),
                kind.as_str(),
                era,
                if complete { 1 } else { 0 },
                row_count as i64,
                metadata.as_ref().map(|(genesis_hash, _)| genesis_hash),
                metadata
                    .as_ref()
                    .map(|(_, spec_version)| (*spec_version).max(0)),
            ],
        )?;
        Ok(())
    }

    fn should_replace_validator_cache(
        &self,
        network: Network,
        new_count: usize,
        complete: bool,
    ) -> Result<bool> {
        if complete {
            return Ok(true);
        }

        let existing = self
            .get_cache_snapshot(network, CacheKind::Validators)?
            .map(|snapshot| snapshot.row_count as usize)
            .unwrap_or_else(|| self.count_cached_validators(network).unwrap_or(0) as usize);

        if existing == 0 {
            return Ok(true);
        }

        if !self.cached_validators_have_chain_data(network)? {
            return Ok(true);
        }

        Ok(new_count.saturating_mul(10) >= existing.saturating_mul(9))
    }

    fn cached_validators_have_chain_data(&self, network: Network) -> Result<bool> {
        let (stake_count, apy_count): (i64, i64) = self.conn.query_row(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN CAST(total_stake AS TEXT) != '0' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN apy IS NOT NULL THEN 1 ELSE 0 END), 0)
            FROM cached_validators
            WHERE network = ?1
            "#,
            params![network.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        Ok(stake_count > 0 && apy_count > 0)
    }

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

    /// Get cached validators only if the snapshot is recent enough.
    pub fn get_recent_cached_validators(
        &self,
        network: Network,
        max_age_secs: i64,
    ) -> Result<Vec<DisplayValidator>> {
        if !Self::snapshot_is_usable(
            self.get_cache_snapshot(network, CacheKind::Validators)?,
            None,
            max_age_secs,
            DEFAULT_STARTUP_CACHE_MAX_ERA_LAG,
        ) {
            return Ok(Vec::new());
        }
        self.get_cached_validators(network)
    }

    /// Get cached validators only if the snapshot is fresh for the current era.
    pub fn get_fresh_cached_validators(
        &self,
        network: Network,
        current_era: u32,
        max_age_secs: i64,
        max_era_lag: u32,
    ) -> Result<Vec<DisplayValidator>> {
        if !Self::snapshot_is_usable(
            self.get_cache_snapshot(network, CacheKind::Validators)?,
            Some(current_era),
            max_age_secs,
            max_era_lag,
        ) {
            return Ok(Vec::new());
        }
        self.get_cached_validators(network)
    }

    /// Get cached validators for startup display and refresh decisions.
    pub fn get_startup_cached_validators(
        &self,
        network: Network,
        current_era: u32,
        policy: CachePolicy,
    ) -> Result<Cached<Vec<DisplayValidator>>> {
        let snapshot = self.get_cache_snapshot(network, CacheKind::Validators)?;
        let current_metadata = self.get_chain_metadata(network)?;
        let freshness = Self::startup_freshness(
            snapshot.as_ref(),
            current_era,
            current_metadata.as_ref(),
            policy,
        );
        let data = if freshness == CacheFreshness::Missing {
            Vec::new()
        } else {
            self.get_cached_validators(network)?
        };

        Ok(Cached {
            data,
            snapshot,
            freshness,
        })
    }

    /// Store validators in the cache (replaces existing validators for this network).
    pub fn set_cached_validators(
        &mut self,
        network: Network,
        era: u32,
        validators: &[DisplayValidator],
    ) -> Result<usize> {
        self.set_cached_validators_checked(network, era, validators, true)
    }

    /// Store validators in the cache with completeness metadata.
    ///
    /// Incomplete snapshots are allowed to populate an empty cache, but they do
    /// not replace a much larger existing cache. This protects the local DB from
    /// transient light-client or iterator interruptions.
    pub fn set_cached_validators_checked(
        &mut self,
        network: Network,
        era: u32,
        validators: &[DisplayValidator],
        complete: bool,
    ) -> Result<usize> {
        if !self.should_replace_validator_cache(network, validators.len(), complete)? {
            return Ok(0);
        }

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

            Self::set_cache_snapshot_tx(&tx, network, CacheKind::Validators, era, complete, count)?;
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
            ORDER BY
                CASE WHEN apy IS NULL THEN 1 ELSE 0 END,
                apy DESC,
                member_count DESC,
                id ASC
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

    /// Get cached pools only if the snapshot is recent enough.
    pub fn get_recent_cached_pools(
        &self,
        network: Network,
        max_age_secs: i64,
    ) -> Result<Vec<DisplayPool>> {
        if !Self::snapshot_is_usable(
            self.get_cache_snapshot(network, CacheKind::Pools)?,
            None,
            max_age_secs,
            DEFAULT_STARTUP_CACHE_MAX_ERA_LAG,
        ) {
            return Ok(Vec::new());
        }
        self.get_cached_pools(network)
    }

    /// Get cached pools only if the snapshot is fresh for the current era.
    pub fn get_fresh_cached_pools(
        &self,
        network: Network,
        current_era: u32,
        max_age_secs: i64,
        max_era_lag: u32,
    ) -> Result<Vec<DisplayPool>> {
        if !Self::snapshot_is_usable(
            self.get_cache_snapshot(network, CacheKind::Pools)?,
            Some(current_era),
            max_age_secs,
            max_era_lag,
        ) {
            return Ok(Vec::new());
        }
        self.get_cached_pools(network)
    }

    /// Get cached pools for startup display and refresh decisions.
    pub fn get_startup_cached_pools(
        &self,
        network: Network,
        current_era: u32,
        policy: CachePolicy,
    ) -> Result<Cached<Vec<DisplayPool>>> {
        let snapshot = self.get_cache_snapshot(network, CacheKind::Pools)?;
        let current_metadata = self.get_chain_metadata(network)?;
        let freshness = Self::startup_freshness(
            snapshot.as_ref(),
            current_era,
            current_metadata.as_ref(),
            policy,
        );
        let data = if freshness == CacheFreshness::Missing {
            Vec::new()
        } else {
            self.get_cached_pools(network)?
        };

        Ok(Cached {
            data,
            snapshot,
            freshness,
        })
    }

    /// Store nomination pools in the cache (replaces existing pools for this network).
    pub fn set_cached_pools(&mut self, network: Network, pools: &[DisplayPool]) -> Result<usize> {
        self.set_cached_pools_at_era(network, 0, pools)
    }

    /// Store nomination pools in the cache with the era observed when fetched.
    pub fn set_cached_pools_at_era(
        &mut self,
        network: Network,
        era: u32,
        pools: &[DisplayPool],
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

            Self::set_cache_snapshot_tx(&tx, network, CacheKind::Pools, era, true, count)?;
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
                   nominations_json, pool_id, pool_points, unlocking_json,
                   pool_unbonding_eras_json, pool_last_recorded_reward_counter
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
                unlocking_json: row.get(7)?,
                pool_unbonding_eras_json: row.get(8)?,
                pool_last_recorded_reward_counter: read_u128(row, 9)?,
            })
        });

        match result {
            Ok(status) => Ok(Some(status)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get cached account status if it is recent enough for read-through UI use.
    pub fn get_recent_cached_account_status(
        &self,
        network: Network,
        address: &str,
        max_age_secs: i64,
    ) -> Result<Option<CachedAccountStatus>> {
        let result = self.conn.query_row(
            r#"
            SELECT MAX(0, CAST((julianday('now') - julianday(updated_at)) * 86400 AS INTEGER))
            FROM cached_account_status
            WHERE network = ?1 AND address = ?2
            "#,
            params![network.to_string(), address],
            |row| row.get::<_, Option<i64>>(0),
        );

        let age_seconds = match result {
            Ok(Some(age_seconds)) => age_seconds,
            Ok(None) | Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e),
        };

        if age_seconds > max_age_secs {
            return Ok(None);
        }

        self.get_cached_account_status(network, address)
    }

    /// Get cached account status using a shared read-through cache policy.
    pub fn get_account_status_for_read_through(
        &self,
        network: Network,
        address: &str,
        policy: CachePolicy,
    ) -> Result<Option<CachedAccountStatus>> {
        self.get_recent_cached_account_status(network, address, policy.account_max_age_secs)
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
                 staked_amount, nominations_json, pool_id, pool_points, unlocking_json,
                 pool_unbonding_eras_json, pool_last_recorded_reward_counter, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, CURRENT_TIMESTAMP)
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
                &status.unlocking_json,
                &status.pool_unbonding_eras_json,
                status.pool_last_recorded_reward_counter.to_string(),
            ],
        )?;
        Ok(())
    }
}

impl StartupDataService {
    /// Load startup validator and pool caches under the shared cache policy.
    pub fn load(
        db: &StakingDb,
        network: Network,
        current_era: u32,
        policy: CachePolicy,
    ) -> Result<StartupDataCache> {
        Ok(StartupDataCache {
            validators: db.get_startup_cached_validators(network, current_era, policy)?,
            pools: db.get_startup_cached_pools(network, current_era, policy)?,
        })
    }
}

impl AccountStatusService {
    /// Load account status for read-through UI use under the shared policy.
    pub fn load_cached(
        db: &StakingDb,
        network: Network,
        address: &str,
        policy: CachePolicy,
    ) -> Result<Option<CachedAccountStatus>> {
        db.get_account_status_for_read_through(network, address, policy)
    }
}

impl HistoryService {
    /// Return the inclusive history window used by both apps.
    pub fn era_window(current_era: u32, num_eras: u32) -> (u32, u32) {
        (
            current_era.saturating_sub(num_eras),
            current_era.saturating_sub(1),
        )
    }

    /// Return true when a cached APY is valid under the cache policy.
    pub fn is_valid_cached_apy(apy: f64, policy: CachePolicy) -> bool {
        apy.is_finite() && (0.0..=policy.history_max_apy).contains(&apy)
    }

    /// Load cached history for an inclusive era range, filtering invalid APY rows
    /// and filling missing date strings for display.
    #[allow(clippy::too_many_arguments)]
    pub fn load_cached_range(
        db: &StakingDb,
        network: Network,
        address: &str,
        start_era: u32,
        end_era: u32,
        current_era: u32,
        current_era_start_ms: u64,
        era_duration_ms: u64,
        policy: CachePolicy,
    ) -> Result<Vec<StakingHistoryPoint>> {
        let mut history = db.get_history_range(network, address, start_era, end_era)?;
        history.retain(|point| Self::is_valid_cached_apy(point.apy, policy));

        for point in &mut history {
            if point.date.is_none() {
                point.date = Some(Self::calculate_era_date(
                    point.era,
                    current_era,
                    current_era_start_ms,
                    era_duration_ms,
                ));
            }
        }

        Ok(history)
    }

    /// Return eras missing from the valid cached history range.
    pub fn missing_eras(
        db: &StakingDb,
        network: Network,
        address: &str,
        start_era: u32,
        end_era: u32,
        policy: CachePolicy,
    ) -> Result<Vec<u32>> {
        db.get_missing_eras_with_max_apy(
            network,
            address,
            start_era,
            end_era,
            policy.history_max_apy,
        )
    }

    /// Load the latest cached history points for fallback display.
    pub fn load_latest(
        db: &StakingDb,
        network: Network,
        address: &str,
        limit: Option<u32>,
        policy: CachePolicy,
    ) -> Result<Vec<StakingHistoryPoint>> {
        let mut history = db.get_history(network, address, limit)?;
        history.retain(|point| Self::is_valid_cached_apy(point.apy, policy));
        Ok(history)
    }

    fn calculate_era_date(
        era: u32,
        current_era: u32,
        current_era_start_ms: u64,
        era_duration_ms: u64,
    ) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};

        let eras_ago = current_era.saturating_sub(era) as u64;
        let elapsed_ms = eras_ago.saturating_mul(era_duration_ms);
        let reference_ms = if current_era_start_ms > 0 {
            current_era_start_ms
        } else {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
                .unwrap_or_default()
        };
        let era_start_ms = reference_ms.saturating_sub(elapsed_ms);
        let era_start_ms = era_start_ms.min(i64::MAX as u64) as i64;

        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(era_start_ms)
            .unwrap_or_else(chrono::Utc::now)
            .format("%Y%m%d")
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_db_path(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "stkopt-{test_name}-{}-{nonce}.db",
            std::process::id()
        ))
    }

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

    fn make_test_metadata(current_era: u32) -> CachedChainMetadata {
        CachedChainMetadata {
            genesis_hash: "0x00".to_string(),
            spec_version: 1,
            tx_version: 1,
            ss58_prefix: 0,
            token_symbol: "DOT".to_string(),
            token_decimals: 10,
            era_duration_ms: 86_400_000,
            current_era,
        }
    }

    fn remove_test_db_files(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(PathBuf::from(format!("{}-wal", path.display())));
        let _ = std::fs::remove_file(PathBuf::from(format!("{}-shm", path.display())));
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
    fn test_get_missing_eras_with_max_apy_treats_unrealistic_cached_as_missing() {
        let mut db = StakingDb::open_memory().unwrap();

        let points = vec![
            StakingHistoryPoint {
                apy: 0.12,
                ..make_test_point(1500)
            },
            StakingHistoryPoint {
                apy: 0.95,
                ..make_test_point(1501)
            },
            StakingHistoryPoint {
                apy: -0.01,
                ..make_test_point(1502)
            },
        ];

        db.insert_history_batch(Network::Polkadot, "15oF4uV", &points)
            .unwrap();

        let missing = db
            .get_missing_eras_with_max_apy(Network::Polkadot, "15oF4uV", 1500, 1503, 0.50)
            .unwrap();

        assert_eq!(missing, vec![1501, 1502, 1503]);
    }

    #[test]
    fn test_history_service_filters_bad_cached_points_and_fills_dates() {
        let mut db = StakingDb::open_memory().unwrap();
        let points = vec![
            StakingHistoryPoint {
                date: None,
                apy: 0.12,
                ..make_test_point(1500)
            },
            StakingHistoryPoint {
                apy: 0.95,
                ..make_test_point(1501)
            },
        ];
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let cached = HistoryService::load_cached_range(
            &db,
            Network::Polkadot,
            "addr1",
            1500,
            1501,
            1502,
            1_735_862_400_000,
            86_400_000,
            CachePolicy::default(),
        )
        .unwrap();

        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].era, 1500);
        assert!(cached[0].date.is_some());

        let missing = HistoryService::missing_eras(
            &db,
            Network::Polkadot,
            "addr1",
            1500,
            1501,
            CachePolicy::default(),
        )
        .unwrap();
        assert_eq!(missing, vec![1501]);
    }

    #[test]
    fn test_get_history_range_limits_to_requested_eras() {
        let mut db = StakingDb::open_memory().unwrap();
        let points: Vec<_> = (1..=5).map(|i| make_test_point(1500 + i)).collect();
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let history = db
            .get_history_range(Network::Polkadot, "addr1", 1502, 1504)
            .unwrap();

        assert_eq!(
            history.iter().map(|point| point.era).collect::<Vec<_>>(),
            vec![1502, 1503, 1504]
        );
    }

    #[test]
    fn test_get_history_with_limit() {
        let mut db = StakingDb::open_memory().unwrap();
        let points: Vec<_> = (1..=10).map(|i| make_test_point(1500 + i)).collect();
        db.insert_history_batch(Network::Polkadot, "addr1", &points)
            .unwrap();

        let history = db.get_history(Network::Polkadot, "addr1", Some(3)).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(
            history.iter().map(|point| point.era).collect::<Vec<_>>(),
            vec![1508, 1509, 1510]
        );
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

    #[test]
    fn test_validator_identities_within_age_filters_old_entries() {
        let db = StakingDb::open_memory().unwrap();

        db.set_validator_identity(Network::Polkadot, "fresh", "Fresh Validator")
            .unwrap();
        db.set_validator_identity(Network::Polkadot, "old", "Old Validator")
            .unwrap();

        // Age the "old" record by two days.
        db.conn
            .execute(
                "UPDATE validator_identities SET updated_at = datetime('now', '-2 days') WHERE address = ?1",
                params!["old"],
            )
            .unwrap();

        let all = db.get_validator_identities(Network::Polkadot).unwrap();
        assert_eq!(all.len(), 2);

        let recent = db
            .get_validator_identities_within_age(Network::Polkadot, DEFAULT_IDENTITY_MAX_AGE_SECS)
            .unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent.get("fresh"), Some(&"Fresh Validator".to_string()));
        assert!(!recent.contains_key("old"));
    }

    #[test]
    fn test_validator_identities_within_age_keeps_fresh_entries() {
        let db = StakingDb::open_memory().unwrap();

        db.set_validator_identity(Network::Polkadot, "val1", "Validator One")
            .unwrap();

        let recent = db
            .get_validator_identities_within_age(Network::Polkadot, 60)
            .unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent.get("val1"), Some(&"Validator One".to_string()));
    }

    #[test]
    fn test_validator_identities_visible_across_db_clients() {
        let path = unique_test_db_path("validator-identities-visible-across-clients");
        {
            let mut writer = StakingDb::open(&path).unwrap();
            let reader = StakingDb::open(&path).unwrap();

            let mut batch = HashMap::new();
            batch.insert("val1".to_string(), "Validator One".to_string());
            batch.insert("val2".to_string(), "Validator Two".to_string());
            writer
                .set_validator_identities_batch(Network::Polkadot, &batch)
                .unwrap();

            let identities = reader.get_validator_identities(Network::Polkadot).unwrap();
            assert_eq!(identities.len(), 2);
            assert_eq!(identities.get("val1"), Some(&"Validator One".to_string()));
            assert_eq!(identities.get("val2"), Some(&"Validator Two".to_string()));
        }
        remove_test_db_files(&path);
    }

    #[test]
    fn test_sqlite_busy_timeout_waits_for_concurrent_writer() {
        let path = unique_test_db_path("sqlite-busy-timeout-waits");
        {
            let writer = StakingDb::open(&path).unwrap();
            writer.conn.execute_batch("BEGIN IMMEDIATE;").unwrap();

            let thread_path = path.clone();
            let handle = std::thread::spawn(move || {
                let db = StakingDb::open(&thread_path).unwrap();
                db.set_chain_metadata(Network::Polkadot, &make_test_metadata(1500))
                    .unwrap();
            });

            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(
                !handle.is_finished(),
                "writer should wait while another write transaction is active"
            );

            writer.conn.execute_batch("COMMIT;").unwrap();
            handle.join().unwrap();

            let reader = StakingDb::open(&path).unwrap();
            let meta = reader.get_chain_metadata(Network::Polkadot).unwrap();
            assert_eq!(meta.unwrap().current_era, 1500);
        }
        remove_test_db_files(&path);
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
    fn test_fresh_cached_validators_rejects_stale_age_and_era() {
        let mut db = StakingDb::open_memory().unwrap();
        let validators = vec![make_test_validator("val1", 0.15)];
        db.set_cached_validators(Network::Polkadot, 1500, &validators)
            .unwrap();

        let fresh = db
            .get_fresh_cached_validators(Network::Polkadot, 1500, 60, 1)
            .unwrap();
        assert_eq!(fresh.len(), 1);

        let old_era = db
            .get_fresh_cached_validators(Network::Polkadot, 1503, 60, 1)
            .unwrap();
        assert!(old_era.is_empty());

        db.conn
            .execute(
                "UPDATE cache_snapshots SET updated_at = datetime('now', '-2 days') WHERE network = ?1 AND kind = ?2",
                params![Network::Polkadot.to_string(), CacheKind::Validators.as_str()],
            )
            .unwrap();

        let stale = db
            .get_recent_cached_validators(Network::Polkadot, 60)
            .unwrap();
        assert!(stale.is_empty());
    }

    #[test]
    fn test_startup_cache_marks_recent_old_era_as_stale_but_displayable() {
        let mut db = StakingDb::open_memory().unwrap();
        let validators = vec![make_test_validator("val1", 0.15)];
        db.set_cached_validators(Network::Polkadot, 1500, &validators)
            .unwrap();

        let cached = db
            .get_startup_cached_validators(Network::Polkadot, 1503, CachePolicy::default())
            .unwrap();

        assert_eq!(cached.freshness, CacheFreshness::Stale);
        assert!(cached.needs_refresh());
        assert!(cached.is_displayable());
        assert_eq!(cached.data.len(), 1);
        assert_eq!(cached.snapshot.unwrap().era, 1500);
    }

    #[test]
    fn test_startup_data_service_loads_validators_and_pools_independently() {
        let mut db = StakingDb::open_memory().unwrap();
        db.set_chain_metadata(Network::Polkadot, &make_test_metadata(1500))
            .unwrap();
        let validators = vec![make_test_validator("val1", 0.15)];
        db.set_cached_validators(Network::Polkadot, 1500, &validators)
            .unwrap();

        let startup =
            StartupDataService::load(&db, Network::Polkadot, 1500, CachePolicy::default()).unwrap();

        assert_eq!(startup.validators.freshness, CacheFreshness::Fresh);
        assert_eq!(startup.validators.data.len(), 1);
        assert_eq!(startup.pools.freshness, CacheFreshness::Missing);
        assert!(startup.pools.data.is_empty());
        assert!(startup.pools.needs_refresh());
    }

    #[test]
    fn test_startup_cache_refreshes_on_runtime_metadata_mismatch() {
        let mut db = StakingDb::open_memory().unwrap();
        let mut meta = make_test_metadata(1500);
        db.set_chain_metadata(Network::Polkadot, &meta).unwrap();
        db.set_cached_validators(
            Network::Polkadot,
            1500,
            &[make_test_validator("val1", 0.15)],
        )
        .unwrap();

        let fresh = db
            .get_startup_cached_validators(Network::Polkadot, 1500, CachePolicy::default())
            .unwrap();
        assert_eq!(fresh.freshness, CacheFreshness::Fresh);
        assert_eq!(fresh.snapshot.as_ref().unwrap().spec_version, Some(1));

        meta.spec_version = 2;
        db.set_chain_metadata(Network::Polkadot, &meta).unwrap();

        let stale = db
            .get_startup_cached_validators(Network::Polkadot, 1500, CachePolicy::default())
            .unwrap();
        assert_eq!(stale.freshness, CacheFreshness::Stale);
        assert!(stale.is_displayable());
        assert!(stale.needs_refresh());
    }

    #[test]
    fn test_startup_cache_is_stale_when_current_era_unknown() {
        let mut db = StakingDb::open_memory().unwrap();
        let meta = make_test_metadata(1500);
        db.set_chain_metadata(Network::Polkadot, &meta).unwrap();
        db.set_cached_validators(
            Network::Polkadot,
            1500,
            &[make_test_validator("val1", 0.15)],
        )
        .unwrap();

        let cached = db
            .get_startup_cached_validators(Network::Polkadot, 0, CachePolicy::default())
            .unwrap();

        assert_eq!(cached.freshness, CacheFreshness::Stale);
        assert!(cached.is_displayable());
        assert!(cached.needs_refresh());
    }

    #[test]
    fn test_startup_cache_is_stale_when_metadata_missing() {
        let mut db = StakingDb::open_memory().unwrap();
        db.set_cached_validators(
            Network::Polkadot,
            1500,
            &[make_test_validator("val1", 0.15)],
        )
        .unwrap();

        let cached = db
            .get_startup_cached_validators(Network::Polkadot, 1500, CachePolicy::default())
            .unwrap();

        assert_eq!(cached.freshness, CacheFreshness::Stale);
        assert!(cached.is_displayable());
        assert!(cached.needs_refresh());
    }

    #[test]
    fn test_snapshot_is_not_fresh_for_zero_era() {
        let snapshot = CacheSnapshot {
            era: 1500,
            complete: true,
            row_count: 1,
            age_seconds: 1,
            genesis_hash: Some("0x00".to_string()),
            spec_version: Some(1),
        };

        assert!(!snapshot.is_fresh_for_era(0, 3600, 1));
    }

    #[test]
    fn test_matches_chain_metadata_requires_current_metadata() {
        let snapshot = CacheSnapshot {
            era: 1500,
            complete: true,
            row_count: 1,
            age_seconds: 1,
            genesis_hash: Some("0x00".to_string()),
            spec_version: Some(1),
        };

        let current = CachedChainMetadata {
            genesis_hash: "0x00".to_string(),
            spec_version: 1,
            ..make_test_metadata(1500)
        };
        assert!(snapshot.matches_chain_metadata(&current));

        let mismatched = CachedChainMetadata {
            genesis_hash: "0x01".to_string(),
            spec_version: 1,
            ..make_test_metadata(1500)
        };
        assert!(!snapshot.matches_chain_metadata(&mismatched));
    }

    #[test]
    fn test_incomplete_validator_cache_does_not_replace_larger_cache() {
        let mut db = StakingDb::open_memory().unwrap();
        let complete: Vec<_> = (0..10)
            .map(|index| make_test_validator(&format!("val{}", index), 0.10 + index as f64 * 0.01))
            .collect();
        db.set_cached_validators_checked(Network::Polkadot, 1500, &complete, true)
            .unwrap();

        let partial = vec![
            make_test_validator("partial1", 0.20),
            make_test_validator("partial2", 0.19),
        ];
        let written = db
            .set_cached_validators_checked(Network::Polkadot, 1501, &partial, false)
            .unwrap();
        assert_eq!(written, 0);

        let cached = db.get_cached_validators(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 10);
        assert!(
            !cached
                .iter()
                .any(|validator| validator.address == "partial1")
        );
    }

    #[test]
    fn test_incomplete_validator_cache_replaces_larger_cache_without_chain_data() {
        let mut db = StakingDb::open_memory().unwrap();
        let broken: Vec<_> = (0..10)
            .map(|index| {
                let mut validator = make_test_validator(&format!("broken{}", index), 0.0);
                validator.total_stake = 0;
                validator.own_stake = 0;
                validator.nominator_count = 0;
                validator.points = 0;
                validator.apy = None;
                validator
            })
            .collect();
        db.set_cached_validators_checked(Network::Polkadot, 1500, &broken, true)
            .unwrap();

        let partial = vec![
            make_test_validator("partial1", 0.20),
            make_test_validator("partial2", 0.19),
        ];
        let written = db
            .set_cached_validators_checked(Network::Polkadot, 1501, &partial, false)
            .unwrap();
        assert_eq!(written, 2);

        let cached = db.get_cached_validators(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 2);
        assert!(
            cached
                .iter()
                .any(|validator| validator.address == "partial1")
        );
    }

    #[test]
    fn test_incomplete_validator_cache_can_populate_empty_cache() {
        let mut db = StakingDb::open_memory().unwrap();
        let partial = vec![make_test_validator("partial1", 0.20)];
        let written = db
            .set_cached_validators_checked(Network::Polkadot, 1501, &partial, false)
            .unwrap();
        assert_eq!(written, 1);

        let snapshot = db
            .get_cache_snapshot(Network::Polkadot, CacheKind::Validators)
            .unwrap()
            .unwrap();
        assert!(!snapshot.complete);
        assert_eq!(snapshot.row_count, 1);
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
    fn test_cached_validators_visible_across_db_clients_with_identity_join() {
        let path = unique_test_db_path("cached-validators-visible-across-clients");
        {
            let mut writer = StakingDb::open(&path).unwrap();
            let reader = StakingDb::open(&path).unwrap();

            writer
                .set_validator_identity(Network::Polkadot, "val1", "Cached Name")
                .unwrap();
            writer
                .set_cached_validators(
                    Network::Polkadot,
                    1500,
                    &[DisplayValidator {
                        address: "val1".to_string(),
                        name: None,
                        commission: 0.05,
                        blocked: false,
                        total_stake: 1_000_000,
                        own_stake: 100_000,
                        nominator_count: 10,
                        points: 100,
                        apy: Some(0.15),
                    }],
                )
                .unwrap();

            let cached = reader.get_cached_validators(Network::Polkadot).unwrap();
            assert_eq!(cached.len(), 1);
            assert_eq!(cached[0].name, Some("Cached Name".to_string()));
            assert_eq!(cached[0].address, "val1");
        }
        let _ = std::fs::remove_file(path);
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
    fn test_fresh_cached_pools_rejects_stale_age_and_era() {
        let mut db = StakingDb::open_memory().unwrap();
        let pools = vec![make_test_pool(1, PoolState::Open)];
        db.set_cached_pools_at_era(Network::Polkadot, 1500, &pools)
            .unwrap();

        let fresh = db
            .get_fresh_cached_pools(Network::Polkadot, 1501, 60, 1)
            .unwrap();
        assert_eq!(fresh.len(), 1);

        let old_era = db
            .get_fresh_cached_pools(Network::Polkadot, 1502, 60, 1)
            .unwrap();
        assert!(old_era.is_empty());

        db.conn
            .execute(
                "UPDATE cache_snapshots SET updated_at = datetime('now', '-2 days') WHERE network = ?1 AND kind = ?2",
                params![Network::Polkadot.to_string(), CacheKind::Pools.as_str()],
            )
            .unwrap();

        let stale = db.get_recent_cached_pools(Network::Polkadot, 60).unwrap();
        assert!(stale.is_empty());
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

    #[test]
    fn test_cached_pools_visible_across_db_clients() {
        let path = unique_test_db_path("cached-pools-visible-across-clients");
        {
            let mut writer = StakingDb::open(&path).unwrap();
            let reader = StakingDb::open(&path).unwrap();

            writer
                .set_cached_pools(
                    Network::Polkadot,
                    &[
                        make_test_pool(1, PoolState::Open),
                        make_test_pool(2, PoolState::Blocked),
                    ],
                )
                .unwrap();

            let cached = reader.get_cached_pools(Network::Polkadot).unwrap();
            assert_eq!(cached.len(), 2);
            assert_eq!(cached[0].id, 1);
            assert_eq!(cached[1].id, 2);
        }
        let _ = std::fs::remove_file(path);
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
            ..CachedAccountStatus::default()
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
            ..CachedAccountStatus::default()
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
        assert!(loaded.unlocking_json.is_none());
        assert!(loaded.pool_unbonding_eras_json.is_none());
        assert_eq!(loaded.pool_last_recorded_reward_counter, 0);
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
            ..CachedAccountStatus::default()
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

    #[test]
    fn test_cached_account_status_round_trips_unlocking_and_pool_unbonding() {
        let db = StakingDb::open_memory().unwrap();

        let status = CachedAccountStatus {
            free_balance: 1_000_000,
            reserved_balance: 100_000,
            frozen_balance: 50_000,
            staked_amount: 500_000,
            nominations_json: Some("[\"val1\",\"val2\"]".to_string()),
            pool_id: Some(7),
            pool_points: Some(1_234_567),
            unlocking_json: Some(
                r#"[{"value":10000,"era":1501},{"value":20000,"era":1502}]"#.to_string(),
            ),
            pool_unbonding_eras_json: Some(
                serde_json::to_string(&vec![(1601u32, 5_000u128), (1602u32, 8_000u128)]).unwrap(),
            ),
            pool_last_recorded_reward_counter: 99_999,
        };

        db.set_cached_account_status(Network::Polkadot, "addr1", &status)
            .unwrap();

        let loaded = db
            .get_cached_account_status(Network::Polkadot, "addr1")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.free_balance, status.free_balance);
        assert_eq!(loaded.pool_id, status.pool_id);
        assert_eq!(loaded.pool_points, status.pool_points);
        assert_eq!(loaded.unlocking_json, status.unlocking_json);
        assert_eq!(
            loaded.pool_unbonding_eras_json,
            status.pool_unbonding_eras_json
        );
        assert_eq!(
            loaded.pool_last_recorded_reward_counter,
            status.pool_last_recorded_reward_counter
        );
    }

    #[test]
    fn test_recent_cached_account_status_rejects_stale_entries() {
        let db = StakingDb::open_memory().unwrap();
        let status = CachedAccountStatus {
            free_balance: 42,
            ..CachedAccountStatus::default()
        };
        db.set_cached_account_status(Network::Polkadot, "addr1", &status)
            .unwrap();

        assert!(
            db.get_recent_cached_account_status(Network::Polkadot, "addr1", 60)
                .unwrap()
                .is_some()
        );

        db.conn
            .execute(
                "UPDATE cached_account_status SET updated_at = datetime('now', '-2 days') WHERE network = ?1 AND address = ?2",
                params![Network::Polkadot.to_string(), "addr1"],
            )
            .unwrap();

        assert!(
            db.get_recent_cached_account_status(Network::Polkadot, "addr1", 60)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_account_status_service_uses_read_through_policy() {
        let db = StakingDb::open_memory().unwrap();
        let status = CachedAccountStatus {
            free_balance: 1000,
            reserved_balance: 100,
            frozen_balance: 50,
            staked_amount: 500,
            nominations_json: None,
            pool_id: None,
            pool_points: None,
            ..CachedAccountStatus::default()
        };
        db.set_cached_account_status(Network::Polkadot, "addr1", &status)
            .unwrap();

        let loaded = AccountStatusService::load_cached(
            &db,
            Network::Polkadot,
            "addr1",
            CachePolicy {
                account_max_age_secs: 60,
                ..CachePolicy::default()
            },
        )
        .unwrap();
        assert_eq!(loaded.unwrap().free_balance, 1000);

        db.conn
            .execute(
                "UPDATE cached_account_status SET updated_at = datetime('now', '-2 days') WHERE network = ?1 AND address = ?2",
                params![Network::Polkadot.to_string(), "addr1"],
            )
            .unwrap();

        let stale = AccountStatusService::load_cached(
            &db,
            Network::Polkadot,
            "addr1",
            CachePolicy {
                account_max_age_secs: 60,
                ..CachePolicy::default()
            },
        )
        .unwrap();
        assert!(stale.is_none());
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
            ..CachedAccountStatus::default()
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

    /// Simulate a DB created with the legacy `points` column and verify
    /// init_schema migrates it to canonical `total_bonded` storage.
    #[test]
    fn test_cached_pools_migration_from_legacy_points_column() {
        use rusqlite::Connection;

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE cached_pools (
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
            INSERT INTO cached_pools (network, id, name, state, member_count, points, commission)
            VALUES ('Polkadot', 1, 'Legacy Pool', 'Open', 100, 500000000000000, 0.1);
            "#,
        )
        .unwrap();

        let mut db = StakingDb { conn };
        db.init_schema().unwrap();

        assert!(db.table_has_column("cached_pools", "total_bonded").unwrap());
        assert!(!db.table_has_column("cached_pools", "points").unwrap());

        let cached = db.get_cached_pools(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].id, 1);
        assert_eq!(cached[0].total_bonded, 500_000_000_000_000);
        assert_eq!(cached[0].apy, None);

        let pools = vec![make_test_pool(2, PoolState::Open)];
        db.set_cached_pools(Network::Polkadot, &pools).unwrap();

        let cached = db.get_cached_pools(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].id, 2);
    }

    /// Simulate a DB created before cached pool APY was added and verify
    /// init_schema adds the nullable column before pool caching writes to it.
    #[test]
    fn test_cached_pools_migration_adds_missing_apy() {
        use rusqlite::Connection;

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE cached_pools (
                network TEXT NOT NULL,
                id INTEGER NOT NULL,
                name TEXT NOT NULL,
                state TEXT NOT NULL,
                member_count INTEGER NOT NULL,
                total_bonded TEXT NOT NULL,
                commission REAL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, id)
            );
            INSERT INTO cached_pools (network, id, name, state, member_count, total_bonded, commission)
            VALUES ('Polkadot', 1, 'Legacy Pool', 'Open', 100, '500000000000000', 0.1);
            "#,
        )
        .unwrap();

        let mut db = StakingDb { conn };
        db.init_schema().unwrap();

        let cached = db.get_cached_pools(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].id, 1);
        assert_eq!(cached[0].apy, None);

        let pools = vec![make_test_pool(2, PoolState::Open)];
        db.set_cached_pools(Network::Polkadot, &pools).unwrap();

        let cached = db.get_cached_pools(Network::Polkadot).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].id, 2);
        assert_eq!(cached[0].apy, Some(0.12));
    }

    #[test]
    fn test_v2_migration_creates_cache_snapshots() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
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
            CREATE TABLE chain_metadata (
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
            INSERT INTO cached_validators
                (network, address, commission, blocked, total_stake, own_stake,
                 nominator_count, points, apy, era)
            VALUES ('Polkadot', 'val1', 0.05, 0, '1000', '100', 10, 1, 0.12, 1500);
            INSERT INTO cached_pools
                (network, id, name, state, member_count, total_bonded, commission, apy)
            VALUES ('Polkadot', 1, 'Pool', 'Open', 100, '5000', 0.1, 0.11);
            INSERT INTO chain_metadata
                (network, genesis_hash, spec_version, tx_version, ss58_prefix,
                 token_symbol, token_decimals, era_duration_ms, current_era)
            VALUES ('Polkadot', '0x00', 1, 1, 0, 'DOT', 10, 86400000, 1501);
            PRAGMA user_version = 2;
            "#,
        )
        .unwrap();

        let mut db = StakingDb { conn };
        db.init_schema().unwrap();

        let validators = db
            .get_cache_snapshot(Network::Polkadot, CacheKind::Validators)
            .unwrap()
            .unwrap();
        assert_eq!(validators.era, 1500);
        assert_eq!(validators.row_count, 1);

        let pools = db
            .get_cache_snapshot(Network::Polkadot, CacheKind::Pools)
            .unwrap()
            .unwrap();
        assert_eq!(pools.era, 1501);
        assert_eq!(pools.row_count, 1);

        let version: i32 = db
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_v3_migration_adds_snapshot_runtime_metadata() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE chain_metadata (
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
            CREATE TABLE cache_snapshots (
                network TEXT NOT NULL,
                kind TEXT NOT NULL,
                era INTEGER NOT NULL DEFAULT 0,
                complete INTEGER NOT NULL DEFAULT 1,
                row_count INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (network, kind)
            );
            INSERT INTO chain_metadata
                (network, genesis_hash, spec_version, tx_version, ss58_prefix,
                 token_symbol, token_decimals, era_duration_ms, current_era)
            VALUES ('Polkadot', '0xabc', 42, 1, 0, 'DOT', 10, 86400000, 1500);
            INSERT INTO cache_snapshots
                (network, kind, era, complete, row_count)
            VALUES ('Polkadot', 'validators', 1500, 1, 1);
            PRAGMA user_version = 3;
            "#,
        )
        .unwrap();

        let mut db = StakingDb { conn };
        db.init_schema().unwrap();

        let snapshot = db
            .get_cache_snapshot(Network::Polkadot, CacheKind::Validators)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.genesis_hash.as_deref(), Some("0xabc"));
        assert_eq!(snapshot.spec_version, Some(42));

        let version: i32 = db
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }
}
