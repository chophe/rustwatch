# rustwatch

Local activity memory for macOS — a lightweight, Pieces-inspired CLI that captures keyboard context, classifies activities with cloud LLMs, stores memory locally, and exposes it via MCP.

## Features

- **Capture daemon** (`rustwatchd`) — keyboard, clipboard, focus changes, optional screenshots
- **SQLite event log** — raw events, session segments, activities
- **Memory layer** — local vector + graph search (SQLite default; LanceDB + SurrealDB via optional backends crate)
- **Cloud LLM analysis** — OpenAI or Anthropic activity labeling
- **CLI + TUI** — status, chart, memory search, interactive dashboard
- **MCP server** — stdio JSON-RPC for Cursor / Claude Desktop

## Build

```bash
cargo build --release
```

Optional LanceDB + SurrealDB backends (separate crate, heavy deps):

```bash
cargo build --release --manifest-path crates/rustwatch-memory-backends/Cargo.toml
```

Optional fastembed for real local embeddings:

```bash
cargo build --release -p rustwatch-memory --features fastembed
```

## Binaries

| Binary | Purpose |
|--------|---------|
| `rustwatch` | CLI |
| `rustwatchd` | Background capture daemon |
| `rustwatch-mcp` | MCP memory server |

## Quick start (macOS)

```bash
cargo build --release

# Grant permissions first
./target/release/rustwatch permissions

# Start daemon
./target/release/rustwatch start

# Watch events
./target/release/rustwatch tail

# Analyze with OpenAI (set OPENAI_API_KEY)
./target/release/rustwatch analyze --today

# Activity chart
./target/release/rustwatch chart --date 2026-08-01

# Memory search
./target/release/rustwatch memory search "rustwatch development"

# Interactive TUI
./target/release/rustwatch tui
```

## MCP (Cursor)

```json
{
  "mcpServers": {
    "rustwatch": {
      "command": "/absolute/path/to/target/release/rustwatch-mcp",
      "args": []
    }
  }
}
```

## Config

Created at `~/.rustwatch/config.toml` on first run.

## Data layout

| Path | Contents |
|------|----------|
| `~/.rustwatch/rustwatch.db` | Events, segments, activities |
| `~/.rustwatch/memory.db` | Vector memory chunks (default backend) |
| `~/.rustwatch/memory-graph.db` | Graph nodes/edges (default backend) |
| `~/.rustwatch/lance/` | LanceDB vectors (optional backends crate) |
| `~/.rustwatch/surreal/` | SurrealDB graph (optional backends crate) |
| `~/.rustwatch/screenshots/` | PNG captures |

## macOS permissions

1. **Input Monitoring** — keyboard capture
2. **Accessibility** — focused text fields (best-effort)
3. **Screen Recording** — window titles + screenshots

Restart the daemon after granting Screen Recording.

## License

MIT
