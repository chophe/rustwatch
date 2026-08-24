use async_trait::async_trait;
use rustwatch_core::{
    ActivityBatchResponse, ActivityLabel, ActivityRecord, Config, SegmentBatch, Store,
};

use crate::redact::Redactor;

#[async_trait]
pub trait ActivityClassifier: Send + Sync {
    async fn classify(&self, batch: SegmentBatch) -> anyhow::Result<Vec<ActivityLabel>>;
}

pub fn build_classifier(config: &Config) -> anyhow::Result<Box<dyn ActivityClassifier>> {
    match config.analyze.provider.as_str() {
        "openai" => Ok(Box::new(OpenAiClassifier::new(config)?)),
        "anthropic" => Ok(Box::new(AnthropicClassifier::new(config)?)),
        other => anyhow::bail!("unsupported provider: {other}"),
    }
}

pub async fn analyze_pending(store: &Store, config: &Config) -> anyhow::Result<Vec<ActivityRecord>> {
    let segments = store.list_unanalyzed_segments(50)?;
    if segments.is_empty() {
        return Ok(Vec::new());
    }

    let redactor = Redactor::new(config)?;
    let filtered: Vec<_> = segments
        .into_iter()
        .filter(|s| !redactor.is_excluded_app(&s.app_name))
        .map(|mut s| {
            s.text_buffer = redactor.scrub(&s.text_buffer);
            s
        })
        .collect();

    if filtered.is_empty() {
        return Ok(Vec::new());
    }

    let classifier = build_classifier(config)?;
    let labels = classifier
        .classify(SegmentBatch {
            segments: filtered.clone(),
        })
        .await?;

    let mut out = Vec::new();
    for label in labels {
        let id = Store::new_activity_id();
        let record = ActivityRecord {
            id: id.clone(),
            label: label.label,
            category: label.category,
            confidence: label.confidence,
            started_at: label.started_at,
            ended_at: label.ended_at,
            apps: label.apps,
            topics: label.topics,
            segment_ids: filtered.iter().map(|s| s.id.clone()).collect(),
        };
        store.insert_activity(&record)?;
        out.push(record);
    }
    Ok(out)
}

struct OpenAiClassifier {
    client: reqwest::Client,
    model: String,
    api_key: String,
}

impl OpenAiClassifier {
    fn new(config: &Config) -> anyhow::Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY not set"))?;
        Ok(Self {
            client: reqwest::Client::new(),
            model: config.analyze.model.clone(),
            api_key,
        })
    }
}

#[async_trait]
impl ActivityClassifier for OpenAiClassifier {
    async fn classify(&self, batch: SegmentBatch) -> anyhow::Result<Vec<ActivityLabel>> {
        let prompt = build_prompt(&batch);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": "Return JSON only with shape {\"activities\":[{\"label\",\"category\",\"confidence\",\"started_at\",\"ended_at\",\"apps\",\"topics\"}]}"},
                {"role": "user", "content": prompt}
            ],
            "response_format": {"type": "json_object"}
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        let content = response["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("{}");
        let parsed: ActivityBatchResponse = serde_json::from_str(content)?;
        Ok(parsed.activities)
    }
}

struct AnthropicClassifier {
    client: reqwest::Client,
    model: String,
    api_key: String,
}

impl AnthropicClassifier {
    fn new(config: &Config) -> anyhow::Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;
        Ok(Self {
            client: reqwest::Client::new(),
            model: config.analyze.model.clone(),
            api_key,
        })
    }
}

#[async_trait]
impl ActivityClassifier for AnthropicClassifier {
    async fn classify(&self, batch: SegmentBatch) -> anyhow::Result<Vec<ActivityLabel>> {
        let prompt = build_prompt(&batch);
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 2048,
            "messages": [
                {"role": "user", "content": format!("Return JSON only. {prompt}")}
            ]
        });

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        let content = response["content"][0]["text"].as_str().unwrap_or("{}");
        let parsed: ActivityBatchResponse = serde_json::from_str(content)?;
        Ok(parsed.activities)
    }
}

fn build_prompt(batch: &SegmentBatch) -> String {
    let mut lines = String::from("Classify user activities from these session segments:\n");
    for segment in &batch.segments {
        lines.push_str(&format!(
            "- segment {} | app={} | title={} | started={} | ended={} | text={}\n",
            segment.id,
            segment.app_name,
            segment.window_title,
            segment.started_at,
            segment.ended_at,
            segment.text_buffer,
        ));
    }
    lines
}
