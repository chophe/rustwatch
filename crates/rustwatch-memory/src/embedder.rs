use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[cfg(feature = "fastembed")]
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub struct Embedder {
    #[cfg(feature = "fastembed")]
    model: Option<TextEmbedding>,
    dims: usize,
}

impl Embedder {
    pub fn new(model_name: &str) -> Self {
        #[cfg(feature = "fastembed")]
        {
            let model = match model_name {
                "BGE-small-en-v1.5" => EmbeddingModel::BGESmallENV15,
                _ => EmbeddingModel::BGESmallENV15,
            };
            if let Ok(m) = TextEmbedding::try_new(InitOptions::new(model)) {
                return Self {
                    model: Some(m),
                    dims: 384,
                };
            }
        }
        let _ = model_name;
        Self { dims: 384 }
    }

    pub fn embed_one(&self, text: &str) -> Vec<f32> {
        #[cfg(feature = "fastembed")]
        if let Some(model) = &self.model {
            if let Ok(embeddings) = model.embed(vec![text.to_string()], None) {
                if let Some(v) = embeddings.into_iter().next() {
                    return v;
                }
            }
        }

        hash_embedding(text, self.dims)
    }
}

fn hash_embedding(text: &str, dims: usize) -> Vec<f32> {
    let mut vec = vec![0.0f32; dims];
    for (idx, token) in text.split_whitespace().enumerate() {
        let mut hasher = DefaultHasher::new();
        token.hash(&mut hasher);
        let h = hasher.finish();
        vec[idx % dims] += ((h % 1000) as f32) / 1000.0;
    }
    let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vec {
            *v /= norm;
        }
    }
    vec
}
