use std::path::Path;
use std::sync::Arc;

use arrow_array::{FixedSizeListArray, RecordBatch, RecordBatchIterator, StringArray};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection, Table};
use rustwatch_memory::{MemoryChunk, ScoredChunk};

const TABLE: &str = "memory_chunks";
const DIMS: i32 = 384;

pub struct LanceMemoryStore {
    table: Table,
}

impl LanceMemoryStore {
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db: Connection = connect(path.to_str().unwrap()).execute().await?;
        let table = match db.open_table(TABLE).execute().await {
            Ok(table) => table,
            Err(_) => {
                let schema = Arc::new(Schema::new(vec![
                    Field::new("chunk_id", DataType::Utf8, false),
                    Field::new("text", DataType::Utf8, false),
                    Field::new("app_name", DataType::Utf8, false),
                    Field::new(
                        "vector",
                        DataType::FixedSizeList(
                            Arc::new(Field::new("item", DataType::Float32, true)),
                            DIMS,
                        ),
                        false,
                    ),
                ]));
                let empty = RecordBatch::new_empty(schema.clone());
                db.create_table(TABLE, Box::new(RecordBatchIterator::new(
                    vec![Ok(empty)],
                    schema,
                )))
                .execute()
                .await?
            }
        };
        Ok(Self { table })
    }

    pub async fn upsert(&self, chunk: &MemoryChunk) -> anyhow::Result<()> {
        let _ = self
            .table
            .delete(&format!("chunk_id = '{}'", escape(&chunk.chunk_id)))
            .await;

        let schema = self.table.schema().await?;
        let batch = chunk_to_batch(chunk, schema)?;
        self.table
            .add(Box::new(RecordBatchIterator::new(
                vec![Ok(batch)],
                self.table.schema().await?,
            )))
            .execute()
            .await?;
        Ok(())
    }

    pub async fn search(&self, embedding: &[f32], k: usize) -> anyhow::Result<Vec<ScoredChunk>> {
        let mut stream = self
            .table
            .query()
            .nearest_to(embedding)?
            .limit(k)
            .execute()
            .await?;

        let mut hits = Vec::new();
        while let Some(batch) = stream.try_next().await? {
            let ids = batch
                .column_by_name("chunk_id")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let texts = batch
                .column_by_name("text")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let apps = batch
                .column_by_name("app_name")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let scores = batch
                .column_by_name("_distance")
                .and_then(|c| c.as_any().downcast_ref::<arrow_array::Float32Array>());

            if let (Some(ids), Some(texts), Some(apps)) = (ids, texts, apps) {
                for i in 0..ids.len() {
                    let score = scores.map(|s| 1.0 - s.value(i)).unwrap_or(0.5);
                    hits.push(ScoredChunk {
                        chunk_id: ids.value(i).to_string(),
                        text: texts.value(i).to_string(),
                        app_name: apps.value(i).to_string(),
                        score,
                    });
                }
            }
        }
        Ok(hits)
    }
}

fn chunk_to_batch(chunk: &MemoryChunk, schema: Arc<Schema>) -> anyhow::Result<RecordBatch> {
    let chunk_id = StringArray::from(vec![chunk.chunk_id.as_str()]);
    let text = StringArray::from(vec![chunk.text.as_str()]);
    let app_name = StringArray::from(vec![chunk.app_name.as_str()]);
    let values = arrow_array::Float32Array::from(chunk.embedding.clone());
    let vector = FixedSizeListArray::new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        DIMS,
        Arc::new(values),
        None,
    );
    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(chunk_id),
            Arc::new(text),
            Arc::new(app_name),
            Arc::new(vector),
        ],
    )?)
}

fn escape(value: &str) -> String {
    value.replace('\'', "''")
}
