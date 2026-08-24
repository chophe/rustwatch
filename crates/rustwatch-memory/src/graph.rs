use rustwatch_core::ActivityRecord;

use crate::ScoredChunk;

pub struct GraphStore {
    conn: rusqlite::Connection,
}

impl GraphStore {
    pub fn open(path: &std::path::Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS graph_nodes (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                label TEXT NOT NULL,
                props_json TEXT
            );
            CREATE TABLE IF NOT EXISTS graph_edges (
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                rel TEXT NOT NULL,
                props_json TEXT
            );
            ",
        )?;
        Ok(Self { conn })
    }

    pub fn upsert_activity(&self, activity: &ActivityRecord, chunk_id: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO graph_nodes (id, kind, label, props_json) VALUES (?1, 'activity', ?2, ?3)",
            rusqlite::params![
                activity.id,
                activity.label,
                serde_json::json!({
                    "chunk_id": chunk_id,
                    "category": activity.category,
                })
                .to_string()
            ],
        )?;

        for app in &activity.apps {
            let app_id = slug(app);
            self.conn.execute(
                "INSERT OR REPLACE INTO graph_nodes (id, kind, label, props_json) VALUES (?1, 'app', ?2, '{}')",
                rusqlite::params![app_id, app],
            )?;
            self.conn.execute(
                "INSERT INTO graph_edges (from_id, to_id, rel, props_json) VALUES (?1, ?2, 'used_app', '{}')",
                rusqlite::params![activity.id, app_id],
            )?;
        }

        for topic in &activity.topics {
            let topic_id = slug(topic);
            self.conn.execute(
                "INSERT OR REPLACE INTO graph_nodes (id, kind, label, props_json) VALUES (?1, 'topic', ?2, '{}')",
                rusqlite::params![topic_id, topic],
            )?;
            self.conn.execute(
                "INSERT INTO graph_edges (from_id, to_id, rel, props_json) VALUES (?1, ?2, 'about_topic', '{}')",
                rusqlite::params![activity.id, topic_id],
            )?;
        }
        Ok(())
    }

    pub fn expand_around_apps(
        &self,
        hits: &[ScoredChunk],
        hops: u8,
    ) -> anyhow::Result<Vec<ScoredChunk>> {
        let mut expanded = Vec::new();
        let limit = hops.max(1) as i64;
        for hit in hits {
            let mut stmt = self.conn.prepare(
                "SELECT n.label, e.rel FROM graph_edges e
                 JOIN graph_nodes n ON n.id = e.to_id
                 WHERE e.from_id IN (
                    SELECT id FROM graph_nodes WHERE json_extract(props_json, '$.chunk_id') = ?1
                 ) LIMIT ?2",
            )?;
            let rows = stmt.query_map(rusqlite::params![hit.chunk_id, limit], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (label, rel) = row?;
                expanded.push(ScoredChunk {
                    chunk_id: hit.chunk_id.clone(),
                    text: format!("Graph edge ({rel}): {label}"),
                    app_name: hit.app_name.clone(),
                    score: hit.score * 0.85,
                });
            }
        }
        Ok(expanded)
    }

    pub fn clear(&self) -> anyhow::Result<()> {
        self.conn.execute("DELETE FROM graph_edges", [])?;
        self.conn.execute("DELETE FROM graph_nodes", [])?;
        Ok(())
    }
}

fn slug(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}
