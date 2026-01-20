use anyhow::Result;
use rusqlite::{Connection, params};

#[derive(Debug, Clone)]
pub struct UserContext {
    pub id: i64,
    pub fingerprint: String,
    pub name: String,
    pub high_score: u32,
}

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
    SaveGame {
        user_id: i64,
        score: u32,
        hits: u32,
        misses: u32,
    },
    UpdateUsername {
        user_id: i64,
        new_name: String,
    },
    GetOrCreateUser {
        fingerprint: String,
        reply_tx: tokio::sync::oneshot::Sender<UserContext>,
    },
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
            DbRequest::GetOrCreateUser {
                fingerprint,
                reply_tx,
            } => {
                let user_context = self
                    .get_or_create_user_context(&fingerprint)
                    .expect("Critical DB error");

                let _ = reply_tx.send(user_context);
                None
            }
            DbRequest::SaveGame {
                user_id,
                score,
                hits,
                misses,
            } => {
                if self.save_game(user_id, score, hits, misses).is_ok() {
                    Some(self.get_current_cache())
                } else {
                    None
                }
            }
            DbRequest::UpdateUsername { user_id, new_name } => {
                if self.update_username(user_id, &new_name).is_ok() {
                    Some(self.get_current_cache())
                } else {
                    None
                }
            }
        }
    }

    pub fn save_game(&self, user_id: i64, score: u32, hits: u32, misses: u32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO user_stats (
            user_id, 
            high_score, 
            total_hits, 
            total_misses, 
            sessions, 
            high_score_at
        )
        VALUES (?1, ?2, ?3, ?4, 1, DATETIME('now'))
        ON CONFLICT(user_id) DO UPDATE SET
            high_score_at = CASE 
                WHEN ?2 > high_score THEN DATETIME('now') 
                ELSE high_score_at 
            END,
            high_score = MAX(high_score, ?2),
            total_hits = total_hits + ?3,
            total_misses = total_misses + ?4,
            sessions = sessions + 1",
            params![user_id, score, hits, misses],
        )?;
        Ok(())
    }

    pub fn get_top_scores(&self, limit: u32) -> Result<Vec<ScoreEntry>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT 
            u.username, 
            s.high_score, 
            strftime('%m-%d %H:%M', s.high_score_at)
         FROM users u
         JOIN user_stats s ON u.id = s.user_id
         WHERE s.high_score > 0
         ORDER BY s.high_score DESC, s.high_score_at ASC
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

    pub fn get_user_by_fingerprint(&self, fingerprint: &str) -> Result<Option<(i64, String)>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, username FROM users WHERE fingerprint = ?1")?;

        let mut rows = stmt.query(params![fingerprint])?;
        if let Some(row) = rows.next()? {
            Ok(Some((row.get(0)?, row.get(1)?)))
        } else {
            Ok(None)
        }
    }

    pub fn create_user(&self, fingerprint: &str, initial_name: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO users (fingerprint, username) VALUES (?1, ?2)",
            params![fingerprint, initial_name],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_username(&self, user_id: i64, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE users SET username = ?1 WHERE id = ?2",
            params![name, user_id],
        )?;
        Ok(())
    }

    pub fn get_or_create_user_context(&self, fingerprint: &str) -> Result<UserContext> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT u.id, u.username, IFNULL(s.high_score, 0) 
             FROM users u 
             LEFT JOIN user_stats s ON u.id = s.user_id 
             WHERE u.fingerprint = ?1",
        )?;

        let res = stmt.query_row(params![fingerprint], |row| {
            Ok(UserContext {
                id: row.get(0)?,
                fingerprint: fingerprint.to_string(),
                name: row.get(1)?,
                high_score: row.get(2)?,
            })
        });

        match res {
            Ok(ctx) => Ok(ctx),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let id = self.create_user(fingerprint, "NewPlayer")?;
                Ok(UserContext {
                    id,
                    fingerprint: fingerprint.to_string(),
                    name: "NewPlayer".to_string(),
                    high_score: 0,
                })
            }
            Err(e) => Err(e.into()),
        }
    }
}

fn setup_schema(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", &"WAL")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            fingerprint TEXT UNIQUE NOT NULL,
            username TEXT UNIQUE NOT NULL,
            created_at DATETIME DEFAULT (DATETIME('now'))
        );

        CREATE TABLE IF NOT EXISTS user_stats (
            user_id INTEGER PRIMARY KEY,
            high_score INTEGER DEFAULT 0,
            total_hits INTEGER DEFAULT 0,
            total_misses INTEGER DEFAULT 0,
            sessions INTEGER DEFAULT 0,
            high_score_at DATETIME DEFAULT (DATETIME('now')),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS daily_activity (
            user_id INTEGER,
            date DATE DEFAULT (DATE('now')),
            count INTEGER DEFAULT 0,
            PRIMARY KEY (user_id, date),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_stats_high_score ON user_stats (high_score DESC);",
    )?;
    Ok(())
}
