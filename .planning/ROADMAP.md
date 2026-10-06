# Roadmap: rustwatch

## Overview

Harden the existing rustwatch engine into reliable local memory for macOS, following the dependency chain: loss-free capture ledger + honest system foundation first, then privacy enforcement (before any new egress or read paths), then robust multi-provider classification with local embeddings, then real memory (FTS5 + hybrid vector search, export, retention), and finally the thin surfaces that present trustworthy data — CLI Ask, TUI dashboard with score/digest, and scoped MCP tools. Each phase delivers one coherent, verifiable capability; nothing downstream is trustworthy until the layer below it works.

## Phases

**Phase Numbering:**

- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: Reliable Capture & System Foundation** - Loss-free daemon capture (keyboard, screenshots, idle, permissions) with honest config and launchd doctor
- [ ] **Phase 2: Privacy Enforcement** - Secrets redacted everywhere, exclusions enforced at capture, pause mode, local-first screenshots
- [ ] **Phase 3: Classification & Local Embeddings** - Robust multi-provider LLM classification (OpenAI/Anthropic/local) with default-on local embeddings
- [ ] **Phase 4: Memory Search & Storage** - Real keyword + hybrid search, JSON/CSV export, bounded screenshot retention
- [ ] **Phase 5: Surfaces — Ask, Dashboard, MCP** - CLI Ask with confidence gating, TUI timeline/score/digest, scoped MCP agent tools

## Phase Details

### Phase 1: Reliable Capture & System Foundation

**Goal**: The daemon captures keyboard context, app/window state, and screenshots continuously without silent loss, survives sleep, reports permissions honestly, and every config knob is real
**Mode:** mvp
**Depends on**: Nothing (first phase)
**Requirements**: CAPT-01, CAPT-02, CAPT-03, CAPT-04, CAPT-05, CAPT-06, CAPT-07, SYS-01, SYS-02
**Success Criteria** (what must be TRUE):

  1. User runs the daemon for days — it survives sleep/wake, never panics on non-ASCII/CJK/emoji input, and never silently drops events (write errors surfaced, not swallowed)
  2. User gets screenshots on a configurable 5-minute interval, on every window/app change, and via global hotkey with an annotation attached to today's log; idle periods are excluded so reports never count lunch/coffee as work
  3. User sees correct Input Monitoring / Accessibility / Screen Recording permission states with onboarding guidance, and a missing grant degrades that capture path gracefully instead of capturing nothing silently
  4. Starting a second daemon instance exits with a clear message; `doctor`/`status` confirms launchd installed, daemon running, and DB writable
  5. Every `config.toml` field either works or is removed — no dead knobs; unknown keys warn at startup

**Plans**: 3/3 plans executed

Plans:

- [x] 01-01-PLAN.md — Loss-free ledger + daemon hardening + config honesty (CAPT-01, CAPT-05, SYS-01)
- [x] 01-02-PLAN.md — Screenshot triggers + idle detection + hotkey annotate (CAPT-02, CAPT-03, CAPT-04, CAPT-07)
- [x] 01-03-PLAN.md — Permissions onboarding + launchd doctor/status (CAPT-06, SYS-02)

### Phase 2: Privacy Enforcement

**Goal**: Privacy controls are enforced at every path — secrets never persist or egress, excluded apps are never recorded, capture can be paused, screenshots stay local by default
**Mode:** mvp
**Depends on**: Phase 1
**Requirements**: PRIV-01, PRIV-02, PRIV-03, PRIV-04
**Success Criteria** (what must be TRUE):

  1. A secret-corpus test proves passwords, tokens, and API keys never reach the LLM, DB, MCP, export, or TUI
  2. Content from excluded apps/sites (password managers, banking) is never stored — enforced at capture time on both keyboard and screenshot paths
  3. User can pause capture for 30 minutes (or resume manually) with auto-resume, and paused/excluded gaps are disclosed in Ask/digest UX
  4. Screenshots stay on the local device by default; only redacted text summaries are sent to cloud LLMs unless the user explicitly opts in

**Plans**: TBD

Plans:

- [ ] 02-01: Redactor at write path + every read boundary (MCP/export/TUI), broadened secret patterns, secret-corpus regression test
- [ ] 02-02: Capture-time app/site exclusions + pause/resume mode with gap disclosure

### Phase 3: Classification & Local Embeddings

**Goal**: Pending segments are classified accurately via any provider the user chooses (cloud or local), and embeddings are real and local by default
**Mode:** mvp
**Depends on**: Phase 2
**Requirements**: CLASS-01, CLASS-02, CLASS-03
**Success Criteria** (what must be TRUE):

  1. User classifies pending segments with OpenAI into activities with correct categories/topics — each activity maps to its own segments (no batch fan-out), bad LLM output quarantined, not accepted as fact
  2. User switches classification provider to Anthropic or a local endpoint (Ollama / llama.cpp OpenAI-compatible base_url) purely via config — no code changes
  3. Local embeddings (fastembed) generate by default; if no embedder initializes the daemon fails loudly at startup — never a silent hash fallback

**Plans**: TBD

Plans:

- [ ] 03-01: Provider seam + OpenAI hardening (injectable pipeline, defensive parsing, segment↔activity mapping fix, timeout/retry)
- [ ] 03-02: Anthropic + local endpoint providers, and default-on local embeddings with loud startup failure

### Phase 4: Memory Search & Storage

**Goal**: History is genuinely searchable (keyword + semantic), portable via export, and screenshot storage stays bounded
**Mode:** mvp
**Depends on**: Phase 3
**Requirements**: MEM-01, MEM-02, MEM-03, MEM-04
**Success Criteria** (what must be TRUE):

  1. User keyword-searches history via FTS5 and gets relevant title/context hits
  2. User hybrid-searches (vector KNN + FTS5 with RRF fusion) and gets semantically relevant results, with graceful keyword-only degradation and a visible warning when embeddings are unavailable
  3. User exports events/segments/activities/memory scope to JSON and CSV
  4. Screenshot storage stays within an age + size cap, is pruned automatically, and the caps are surfaced in `status`

**Plans**: TBD

Plans:

- [ ] 04-01: Wire FTS5 + real vector KNN + RRF hybrid search with lexical-always-live degradation
- [ ] 04-02: JSON/CSV export + screenshot retention policy (age + size cap, auto-prune, surfaced in status)

### Phase 5: Surfaces — Ask, Dashboard, MCP

**Goal**: User can interrogate and review their memory through CLI, TUI dashboard, and MCP — all reading trustworthy data from the layers below
**Mode:** mvp
**Depends on**: Phase 4
**Requirements**: SRCH-01, SRCH-02, DASH-01, DASH-02, DASH-03, DASH-04
**Success Criteria** (what must be TRUE):

  1. CLI `memory search` and CLI `ask` ("what did I work on today?") answer from real retrieval, and refuse low-confidence queries instead of producing confident-sounding empty answers
  2. TUI opens on today's timeline with time-by-app/category breakdown, plus a transparent productivity score (category shares × user-tunable weights) and top apps/topics for the day
  3. User reads a terminal-native daily digest (CLI digest + TUI card) summarizing timeline, score, and highlights, and can search and Ask from inside the TUI
  4. Coding agents query memory through MCP tools with scoped permissions — agents cannot read excluded-app content

**Plans**: TBD

Plans:

- [ ] 05-01: CLI `memory search` + `ask` with retrieval-confidence gating
- [ ] 05-02: TUI dashboard (timeline default, time-by-app/category, score, digest, search/Ask boxes)
- [ ] 05-03: MCP tools with scoped permissions + redaction boundary

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4 → 5

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Reliable Capture & System Foundation | 1/3 | In Progress|  |
| 2. Privacy Enforcement | 0/2 | Not started | - |
| 3. Classification & Local Embeddings | 0/2 | Not started | - |
| 4. Memory Search & Storage | 0/2 | Not started | - |
| 5. Surfaces — Ask, Dashboard, MCP | 0/3 | Not started | - |
