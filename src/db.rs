use rusqlite::{Connection, Result, params};

pub struct ScoreEntry {
    pub name: String,
    pub score: u32,
    pub created_at: String,
}

pub struct Leaderboard {
    conn: Connection,
}
impl Leaderboard {
    pub fn new(conn: Connection) -> Result<Self> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS leaderboard (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                score INTEGER NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_leaderboard_score ON leaderboard (score DESC)",
            [],
        )?;

        Ok(Self { conn })
    }

    pub fn save(&self, name: &str, score: u32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO leaderboard (name, score) VALUES (?1, ?2)",
            params![name, score],
        )?;
        Ok(())
    }
    pub fn get_top_10(&self) -> Result<Vec<ScoreEntry>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT name, score, strftime('%m-%d %H:%M', created_at, 'localtime') 
             FROM leaderboard 
             ORDER BY score DESC 
             LIMIT 10",
        )?;

        let entries = stmt
            .query_map([], |row| {
                Ok(ScoreEntry {
                    name: row.get(0)?,
                    score: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;

        Ok(entries)
    }
}
