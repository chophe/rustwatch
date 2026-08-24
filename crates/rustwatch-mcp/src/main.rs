use std::io::{self, BufRead, Write};

use rustwatch_core::{
    paths::load_or_create_config, DaemonClient, DaemonCommand, DaemonReply, DataPaths, Store,
};
use rustwatch_memory::MemoryEngine;
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(io::stderr)
        .init();

    let paths = DataPaths::new(None)?;
    paths.ensure_dirs()?;
    let config = load_or_create_config(&paths.config)?;
    let store = Store::open(&paths.sqlite)?;
    let memory = MemoryEngine::open(&paths, &config).await?;

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = serde_json::from_str(&line)?;
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        let result = match method {
            "initialize" => json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "rustwatch", "version": env!("CARGO_PKG_VERSION")}
            }),
            "tools/list" => json!({
                "tools": [
                    tool("search_activity_memory", "Hybrid search over activity memory", json!({"query": {"type":"string"}, "limit": {"type":"integer"}})),
                    tool("get_activity_timeline", "Activities for a date", json!({"date": {"type":"string"}})),
                    tool("get_segment_context", "Segments in time range", json!({"from": {"type":"string"}, "to": {"type":"string"}})),
                    tool("pause_capture", "Pause daemon capture", json!({})),
                    tool("resume_capture", "Resume daemon capture", json!({})),
                ]
            }),
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                handle_tool(name, &args, &paths, &store, &memory).await?
            }
            _ => json!({"error": format!("unknown method {method}")}),
        };

        let response = json!({"jsonrpc":"2.0","id":id,"result":result});
        writeln!(stdout, "{}", response)?;
        stdout.flush()?;
    }
    Ok(())
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type":"object","properties": schema, "required": []}
    })
}

async fn handle_tool(
    name: &str,
    args: &Value,
    paths: &DataPaths,
    store: &Store,
    memory: &MemoryEngine,
) -> anyhow::Result<Value> {
    match name {
        "search_activity_memory" => {
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            let hits = memory.search(query, limit).await?;
            Ok(json!({"content":[{"type":"text","text": serde_json::to_string_pretty(&hits)?}]}))
        }
        "get_activity_timeline" => {
            let date = args
                .get("date")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
            let parsed = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")?;
            let activities = store.list_activities_for_date(parsed)?;
            Ok(json!({"content":[{"type":"text","text": serde_json::to_string_pretty(&activities)?}]}))
        }
        "get_segment_context" => {
            let from = args.get("from").and_then(|v| v.as_str()).unwrap_or("");
            let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let from = chrono::DateTime::parse_from_rfc3339(from)?.with_timezone(&chrono::Utc);
            let to = chrono::DateTime::parse_from_rfc3339(to)?.with_timezone(&chrono::Utc);
            let segments = store.list_segments_between(from, to)?;
            Ok(json!({"content":[{"type":"text","text": serde_json::to_string_pretty(&segments)?}]}))
        }
        "pause_capture" => daemon_text(paths, DaemonCommand::Pause).await,
        "resume_capture" => daemon_text(paths, DaemonCommand::Resume).await,
        _ => Ok(json!({"content":[{"type":"text","text":"unknown tool"}]})),
    }
}

async fn daemon_text(paths: &DataPaths, cmd: DaemonCommand) -> anyhow::Result<Value> {
    let client = DaemonClient::new(&paths.socket);
    let text = match client.send(cmd).await? {
        DaemonReply::Ok => "ok".to_string(),
        DaemonReply::Error { message } => message,
        other => format!("{other:?}"),
    };
    Ok(json!({"content":[{"type":"text","text": text}]}))
}
