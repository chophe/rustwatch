# rustwatch

## What This Is

Local activity memory for macOS — a lightweight, Pieces-inspired engine that captures keyboard context and screen state, classifies activities with LLMs (cloud or local), stores memory locally in SQLite vector + graph stores, and lets the user search, ask questions, and score daily work via CLI, TUI dashboard, and MCP.

Solo-developer tool for recalling what was done, asking "what did I work on today?", and measuring productive time share.

## Core Value

Reliable local memory of what the user did — if capture, classification, or search silently drops data, nothing else matters.

## Requirements

### Validated

Existing engine behavior (inferred from codebase — working but needs hardening):

- ✓ Keyboard capture via CGEventTap (macOS) — existing
- ✓ Clipboard + active window app/title polling — existing
- ✓ Window/monitor screenshots via xcap — existing
- ✓ Daemon (`rustwatchd`) with Unix-socket IPC + launchd install — existing
- ✓ SQLite event log (events, segments, activities, screenshots) with refinery migrations — existing
- ✓ Event stream → session segment fold (`SegmentGrouper`) — existing
- ✓ Cloud LLM classification (OpenAI / Anthropic) with regex redaction gate — existing
- ✓ Local memory stores (SQLite vector + FTS5 table + graph nodes/edges) with hash-fallback embedder — existing
- ✓ CLI (`status`, `tail`, `analyze`, `chart`, `memory search`, `export`) + ratatui TUI + stdio JSON-RPC MCP server (5 tools) — existing

### Active

- [ ] Reliable capture: keyboard + screen (every 5 min + on window change + hotkey-annotate to daily logs), no panics on non-ASCII/CJK/emoji, survives sleep, correct macOS permissions detection
- [ ] Screenshot modes configurable (interval, window-change, hotkey with annotation note attached to daily log)
- [ ] LLM choice: OpenAI API + Anthropic + local (Ollama / llama.cpp OpenAI-compatible endpoint) for classification; local embeddings option (fastembed) with deterministic fallback
- [ ] Privacy enforced: secret redaction (passwords/tokens/keys never reach LLM or DB), per-app exclusions, local-first (screenshots stay on device by default; only redacted text goes to cloud unless explicit opt-in)
- [ ] Memory correctness: real embeddings end-to-end, hybrid vector + keyword search returns relevant results, graph expansion actually traverses
- [ ] Search + Q&A everywhere: CLI `memory search` + `ask`, TUI search/Ask box, MCP tools for agents
- [ ] Daily scoring + dashboard: timeline, time-by-app/category, productivity score, top topics, daemon/privacy status
- [ ] Config honesty: every `config.toml` field is wired or removed (no dead `vector_backend`, `batch_interval`, `send_screenshots_to_llm`, etc.)

### Out of Scope

- Windows/Linux capture (stub returns UnsupportedPlatform) — macOS only for v1
- Cloud sync / multi-device / team sharing — local single-user only
- Mobile app — desktop CLI/TUI/MCP only
- Real-time collaboration or shared graphs — out of scope

## Context

- Brownfield Rust workspace (edition 2021, 7 members + 1 orphan backends crate): `rustwatch-core`, `rustwatch-capture`, `rustwatch-daemon`, `rustwatch-cli`, `rustwatch-analyze`, `rustwatch-memory`, `rustwatch-mcp`
- Three processes over shared SQLite (`~/.rustwatch/rustwatch.db`, `memory.db`, `memory-graph.db`, `screenshots/` sharded by date); only network egress is outbound LLM classification
- Codebase map: `.planning/codebase/` (STACK, ARCHITECTURE, STRUCTURE, CONVENTIONS, TESTING, INTEGRATIONS, CONCERNS) — see CONCERNS.md for known bugs (non-ASCII panics x3, hardcoded permissions, dead config fields, unused FTS, `hops`-as-LIMIT, no busy_timeout, orphan backends crate, no CI/tests)
- Prior plan: `docs/TESTING_PLAN.md` (five-phase test plan, unimplemented)
- User runs on macOS, wants cloud LLMs for intelligence but all raw data local

## Constraints

- **Platform**: Capture requires macOS (Input Monitoring + Accessibility + Screen Recording); other OSes build but capture is stubbed — why: OS APIs are macOS-specific
- **Stack**: Rust + tokio + rusqlite (bundled) + serde — why: existing workspace, local-first with no system SQLite dependency
- **Privacy**: Raw keystrokes/screenshots never leave device without redaction + explicit opt-in — why: keystroke + screen capture is highly sensitive
- **Config**: `~/.rustwatch/config.toml` is file-first; only API keys via env (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`) — why: existing convention

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Harden existing engine, not rebuild | Engine already implements capture→LLM→search; bugs are fixable | — Pending |
| Capture = keyboard + screen | User chose keyboard+screen over full-context or screenshots-only | — Pending |
| Cloud LLMs + local data, local models optional | User: "use cloud llms and data will be on local machine", plus Ollama/llama.cpp choice | — Pending |
| Screenshots: interval + window-change + hotkey | User chose all three; 5-min default, hotkey annotates daily logs | — Pending |
| Providers: OpenAI + Anthropic + local endpoint | User chose all incl. Anthropic | — Pending |
| UX: CLI + TUI + MCP all in v1 | User chose all three; dashboard = timeline, shares, score, Q&A | — Pending |
| Local-first privacy (redact + app exclusions) | User picked redact-secrets + app exclusions; screenshots local by default | — Pending |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-10-02 after initialization*
