use anyhow::Result;
use rusqlite::{Connection, params};

#[derive(Debug, Clone)]
pub struct ScoreEntry {
    pub name: String,
    pub score: u32,
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct DbCache {
    pub top_scores: Vec<ScoreEntry>,
}

pub struct Repository {
    conn: Connection,
}

pub enum DbRequest {
    SaveScore { name: String, score: u32 },
}

impl Repository {
    pub fn new(conn: Connection) -> Result<Self> {
        self::setup_schema(&conn)?;
        Ok(Self { conn })
    }

    pub fn get_current_cache(&self) -> DbCache {
        DbCache {
            top_scores: self.get_top_scores(10).unwrap_or_default(),
        }
    }

    pub fn handle_request(&self, req: DbRequest) -> Option<DbCache> {
        match req {
            DbRequest::SaveScore { name, score } => {
                if self.save_score(&name, score).is_ok() {
                    return Some(self.get_current_cache());
                }
            }
        }
        None
    }

    pub fn save_score(&self, name: &str, score: u32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO leaderboard (name, score) VALUES (?1, ?2)",
            params![name, score],
        )?;
        Ok(())
    }

    pub fn get_top_scores(&self, limit: u32) -> Result<Vec<ScoreEntry>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT name, score, strftime('%m-%d %H:%M', created_at, 'localtime')
             FROM leaderboard
             ORDER BY score DESC
             LIMIT ?1",
        )?;

        let entries = stmt
            .query_map(params![limit], |row| {
                Ok(ScoreEntry {
                    name: row.get(0)?,
                    score: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

        Ok(entries)
    }
}

fn setup_schema(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS leaderboard (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            score INTEGER NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            fingerprint TEXT UNIQUE NOT NULL,
            username TEXT UNIQUE NOT NULL,
            created_at DATETIME DEFAULT (DATETIME('now', 'localtime'))
        );
        CREATE TABLE IF NOT EXISTS user_stats (
            user_id INTEGER PRIMARY KEY,
            high_score INTEGER DEFAULT 0,
            total_hits INTEGER DEFAULT 0,
            total_misses INTEGER DEFAULT 0,
            sessions INTEGER DEFAULT 0,
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS daily_activity (
            user_id INTEGER,
            date DATE DEFAULT (DATE('now', 'localtime')),
            count INTEGER DEFAULT 0,
            PRIMARY KEY (user_id, date),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_users_fingerprint ON users(fingerprint);
        CREATE INDEX IF NOT EXISTS idx_leaderboard_score ON leaderboard (score DESC);",
    )?;
    Ok(())
}
