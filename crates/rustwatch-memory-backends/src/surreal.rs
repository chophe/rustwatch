use std::path::Path;

use rustwatch_core::ActivityRecord;
use rustwatch_memory::ScoredChunk;
use surrealdb::engine::local::Mem;
use surrealdb::Surreal;

pub struct SurrealGraphStore {
    db: Surreal<Mem>,
}

impl SurrealGraphStore {
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Surreal::new::<Mem>(()).await?;
        db.use_ns("rustwatch").use_db("memory").await?;
        Ok(Self { db })
    }

    pub async fn upsert_activity(
        &self,
        activity: &ActivityRecord,
        chunk_id: &str,
    ) -> anyhow::Result<()> {
        let _: Option<surrealdb::sql::Thing> = self
            .db
            .create(("activity", activity.id.as_str()))
            .content(serde_json::json!({
                "label": activity.label,
                "category": activity.category,
                "chunk_id": chunk_id,
            }))
            .await?;

        for app in &activity.apps {
            let app_id = slug(app);
            let _: Option<surrealdb::sql::Thing> = self
                .db
                .create(("app", app_id.as_str()))
                .content(serde_json::json!({ "name": app }))
                .await?;
            let _: Option<surrealdb::sql::Thing> = self
                .db
                .create("used_app")
                .content(serde_json::json!({
                    "in": format!("activity:{}", activity.id),
                    "out": format!("app:{app_id}"),
                }))
                .await?;
        }

        for topic in &activity.topics {
            let topic_id = slug(topic);
            let _: Option<surrealdb::sql::Thing> = self
                .db
                .create(("topic", topic_id.as_str()))
                .content(serde_json::json!({ "name": topic }))
                .await?;
            let _: Option<surrealdb::sql::Thing> = self
                .db
                .create("about_topic")
                .content(serde_json::json!({
                    "in": format!("activity:{}", activity.id),
                    "out": format!("topic:{topic_id}"),
                }))
                .await?;
        }
        Ok(())
    }

    pub async fn expand_around_apps(
        &self,
        hits: &[ScoredChunk],
        hops: u8,
    ) -> anyhow::Result<Vec<ScoredChunk>> {
        let mut expanded = Vec::new();
        for hit in hits {
            let mut response = self
                .db
                .query(
                    "SELECT ->used_app->app.name AS app, ->about_topic->topic.name AS topic \
                     FROM activity WHERE chunk_id = $chunk_id LIMIT $limit",
                )
                .bind(("chunk_id", hit.chunk_id.clone()))
                .bind(("limit", hops.max(1)))
                .await?;
            let rows: Vec<serde_json::Value> = response.take(0)?;
            for row in rows {
                if let Some(app) = row.get("app").and_then(|v| v.as_str()) {
                    expanded.push(ScoredChunk {
                        chunk_id: hit.chunk_id.clone(),
                        text: format!("Graph edge (used_app): {app}"),
                        app_name: hit.app_name.clone(),
                        score: hit.score * 0.85,
                    });
                }
                if let Some(topic) = row.get("topic").and_then(|v| v.as_str()) {
                    expanded.push(ScoredChunk {
                        chunk_id: hit.chunk_id.clone(),
                        text: format!("Graph edge (about_topic): {topic}"),
                        app_name: hit.app_name.clone(),
                        score: hit.score * 0.85,
                    });
                }
            }
        }
        Ok(expanded)
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
