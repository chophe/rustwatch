#[derive(Debug, Clone)]
pub struct MemoryChunk {
    pub chunk_id: String,
    pub segment_id: Option<String>,
    pub activity_id: Option<String>,
    pub text: String,
    pub app_name: String,
    pub window_title: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct ScoredChunk {
    pub chunk_id: String,
    pub text: String,
    pub app_name: String,
    pub score: f32,
}

pub struct SqliteMemoryStore {
    conn: rusqlite::Connection,
}

impl SqliteMemoryStore {
    pub fn open(path: &std::path::Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS memory_chunks (
                chunk_id TEXT PRIMARY KEY,
                segment_id TEXT,
                activity_id TEXT,
                text TEXT NOT NULL,
                app_name TEXT NOT NULL,
                window_title TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT NOT NULL,
                embedding BLOB NOT NULL
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                chunk_id UNINDEXED,
                text,
                app_name,
                window_title
            );
            ",
        )?;
        Ok(Self { conn })
    }

    pub fn upsert(&self, chunk: &MemoryChunk) -> anyhow::Result<()> {
        let embedding_bytes: Vec<u8> = chunk
            .embedding
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        self.conn.execute(
            "INSERT OR REPLACE INTO memory_chunks
             (chunk_id, segment_id, activity_id, text, app_name, window_title, started_at, ended_at, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                chunk.chunk_id,
                chunk.segment_id,
                chunk.activity_id,
                chunk.text,
                chunk.app_name,
                chunk.window_title,
                chunk.started_at.to_rfc3339(),
                chunk.ended_at.to_rfc3339(),
                embedding_bytes,
            ],
        )?;
        self.conn.execute(
            "INSERT OR REPLACE INTO memory_fts (chunk_id, text, app_name, window_title)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![chunk.chunk_id, chunk.text, chunk.app_name, chunk.window_title],
        )?;
        Ok(())
    }

    pub fn search(&self, query_embedding: &[f32], k: usize) -> anyhow::Result<Vec<ScoredChunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, text, app_name, window_title, embedding FROM memory_chunks",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Vec<u8>>(4)?,
            ))
        })?;

        let mut scored = Vec::new();
        for row in rows {
            let (chunk_id, text, app_name, window_title, bytes) = row?;
            let emb = bytes_to_f32(&bytes);
            let score = cosine(query_embedding, &emb);
            scored.push(ScoredChunk {
                chunk_id,
                text: if text.is_empty() { window_title } else { text },
                app_name,
                score,
            });
        }
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        Ok(scored)
    }

    pub fn clear(&self) -> anyhow::Result<()> {
        self.conn.execute("DELETE FROM memory_chunks", [])?;
        self.conn.execute("DELETE FROM memory_fts", [])?;
        Ok(())
    }
}

fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for i in 0..n {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}
