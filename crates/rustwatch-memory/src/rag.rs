use crate::ScoredChunk;

pub struct GraphRag;

impl GraphRag {
    pub fn merge(query: &str, vector_hits: Vec<ScoredChunk>, graph_hits: Vec<ScoredChunk>) -> Vec<SearchHit> {
        let mut merged: Vec<SearchHit> = Vec::new();
        let query_lower = query.to_lowercase();

        for hit in vector_hits.into_iter().chain(graph_hits) {
            let keyword_boost = if hit.text.to_lowercase().contains(&query_lower) {
                0.2
            } else {
                0.0
            };
            let score = hit.score + keyword_boost;
            if let Some(existing) = merged.iter_mut().find(|h| h.chunk_id == hit.chunk_id) {
                existing.score = existing.score.max(score);
            } else {
                merged.push(SearchHit {
                    chunk_id: hit.chunk_id,
                    text: hit.text,
                    app_name: hit.app_name,
                    score,
                });
            }
        }

        merged.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        merged
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchHit {
    pub chunk_id: String,
    pub text: String,
    pub app_name: String,
    pub score: f32,
}
