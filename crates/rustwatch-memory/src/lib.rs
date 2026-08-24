mod embedder;
mod graph;
mod rag;
mod sqlite_store;

pub use embedder::Embedder;
pub use graph::GraphStore;
pub use rag::GraphRag;
pub use rag::SearchHit;
pub use sqlite_store::{MemoryChunk, ScoredChunk, SqliteMemoryStore};

use rustwatch_core::{ActivityRecord, Config, DataPaths, SessionSegment, Store};

pub struct MemoryEngine {
    store: SqliteMemoryStore,
    graph: GraphStore,
    embedder: Embedder,
    expand_hops: u8,
}

impl MemoryEngine {
    pub async fn open(paths: &DataPaths, config: &Config) -> anyhow::Result<Self> {
        Ok(Self {
            store: SqliteMemoryStore::open(&paths.root.join("memory.db"))?,
            graph: GraphStore::open(&paths.root.join("memory-graph.db"))?,
            embedder: Embedder::new(&config.memory.embedding_model),
            expand_hops: config.memory.graph_expand_hops,
        })
    }

    pub async fn ingest_segments(&self, segments: &[SessionSegment]) -> anyhow::Result<usize> {
        let mut count = 0;
        for segment in segments {
            let text = format!(
                "App: {}\nTitle: {}\n{}",
                segment.app_name, segment.window_title, segment.text_buffer
            );
            let embedding = self.embedder.embed_one(&text);
            let chunk = MemoryChunk {
                chunk_id: uuid::Uuid::new_v4().to_string(),
                segment_id: Some(segment.id.clone()),
                activity_id: None,
                text,
                app_name: segment.app_name.clone(),
                window_title: segment.window_title.clone(),
                started_at: segment.started_at,
                ended_at: segment.ended_at,
                embedding,
            };
            self.store.upsert(&chunk)?;
            count += 1;
        }
        Ok(count)
    }

    pub async fn ingest_activities(
        &self,
        activities: &[ActivityRecord],
    ) -> anyhow::Result<usize> {
        let mut count = 0;
        for activity in activities {
            let text = format!(
                "Activity: {}\nCategory: {}\nApps: {}\nTopics: {}",
                activity.label,
                activity.category,
                activity.apps.join(", "),
                activity.topics.join(", ")
            );
            let embedding = self.embedder.embed_one(&text);
            let chunk_id = uuid::Uuid::new_v4().to_string();
            let chunk = MemoryChunk {
                chunk_id: chunk_id.clone(),
                segment_id: activity.segment_ids.first().cloned(),
                activity_id: Some(activity.id.clone()),
                text,
                app_name: activity.apps.first().cloned().unwrap_or_default(),
                window_title: activity.label.clone(),
                started_at: activity.started_at,
                ended_at: activity.ended_at,
                embedding,
            };
            self.store.upsert(&chunk)?;
            self.graph.upsert_activity(activity, &chunk_id)?;
            count += 1;
        }
        Ok(count)
    }

    pub async fn rebuild_from_store(&self, store: &Store) -> anyhow::Result<usize> {
        self.store.clear()?;
        self.graph.clear()?;
        let from = chrono::Utc::now() - chrono::Duration::days(3650);
        let to = chrono::Utc::now();
        let segments = store.list_segments_between(from, to)?;
        let mut count = self.ingest_segments(&segments).await?;
        for date_offset in 0..365 {
            let date = chrono::Utc::now().date_naive() - chrono::Days::new(date_offset);
            let activities = store.list_activities_for_date(date)?;
            count += self.ingest_activities(&activities).await?;
        }
        Ok(count)
    }

    pub async fn search(&self, query: &str, k: usize) -> anyhow::Result<Vec<SearchHit>> {
        let embedding = self.embedder.embed_one(query);
        let vector_hits = self.store.search(&embedding, k)?;
        let graph_hits = self.graph.expand_around_apps(&vector_hits, self.expand_hops)?;
        Ok(GraphRag::merge(query, vector_hits, graph_hits))
    }
}
