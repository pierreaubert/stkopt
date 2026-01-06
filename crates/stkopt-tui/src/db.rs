//! SQLite database for caching staking history.

use crate::action::StakingHistoryPoint;
use rusqlite::{params, Connection, Result};
use stkopt_core::Network;
use std::path::Path;

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
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let history = db
            .get_history(Network::Polkadot, "15oF4uV", None)
            .unwrap();

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
}
