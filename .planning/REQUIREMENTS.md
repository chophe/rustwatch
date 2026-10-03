# Requirements: rustwatch

**Defined:** 2026-10-03
**Core Value:** Reliable local memory of what the user did — if capture, classification, or search silently drops data, nothing else matters.

## v1 Requirements

Requirements for hardened v1. Each maps to roadmap phases.

### Capture

- [ ] **CAPT-01**: User can run daemon that captures keystroke context + active app/window titles continuously on macOS
- [ ] **CAPT-02**: User gets screenshots every 5 minutes (configurable interval) during active use
- [ ] **CAPT-03**: User gets a screenshot on every window/app change
- [ ] **CAPT-04**: User can press a global hotkey to capture screen + attach an annotation note to today's log in under 200ms visible feedback
- [ ] **CAPT-05**: Daemon survives sleep/wake, never panics on non-ASCII/CJK/emoji input, and never silently drops events (no try_lock-and-drop loss)
- [ ] **CAPT-06**: User sees correct macOS permission states (Input Monitoring, Accessibility, Screen Recording) with onboarding guidance and per-grant graceful degradation
- [ ] **CAPT-07**: Idle time is detected and excluded from active segments so reports never count lunch/coffee as work

### Privacy

- [ ] **PRIV-01**: User's secrets (passwords, tokens, API keys) are redacted before reaching LLM, DB, MCP, export, or TUI, verified by a secret-corpus test
- [ ] **PRIV-02**: User can exclude apps/sites (e.g. password managers, banking) so excluded content is never stored — enforced at capture time on keyboard and screenshot paths
- [ ] **PRIV-03**: User can pause capture for 30 min (or resume manually) with auto-resume, and excluded/paused gaps are disclosed in Ask/digest UX
- [ ] **PRIV-04**: Screenshots stay on local device by default; only redacted text summaries are sent to cloud LLMs unless user explicitly opts in

### Classification

- [ ] **CLASS-01**: User can classify pending segments with OpenAI API into activities with categories/topics
- [ ] **CLASS-02**: User can switch classification provider to Anthropic or a local endpoint (Ollama / llama.cpp OpenAI-compatible base_url) without code changes
- [ ] **CLASS-03**: User can generate local embeddings (fastembed, default-on) with a loud startup error if no embedder initializes — never silent hash fallback

### Memory

- [ ] **MEM-01**: User can keyword-search history via FTS5 and get relevant title/context hits
- [ ] **MEM-02**: User can hybrid-search (vector KNN + FTS5 with RRF fusion) and get semantically relevant results with graceful degradation when embeddings are unavailable
- [ ] **MEM-03**: User can export events/segments/activities/memory scope to JSON/CSV
- [ ] **MEM-04**: User's screenshot storage is bounded by a retention policy (age + size cap, surfaced in status) with automatic pruning

### Search & Agents

- [ ] **SRCH-01**: User can run CLI memory search and CLI ask ("what did I work on today?") with retrieval-confidence gating (no confident-sounding empty answers)
- [ ] **SRCH-02**: User's coding agents can query memory through MCP tools with scoped permissions (agents cannot read excluded-app content)

### Dashboard & Scoring

- [ ] **DASH-01**: User can open TUI dashboard showing today's timeline as the default view with time-by-app/category breakdown
- [ ] **DASH-02**: User can see a transparent productivity score (category shares x user-tunable weights) plus top apps/topics for the day
- [ ] **DASH-03**: User can read a terminal-native daily digest (CLI digest + TUI card) summarizing timeline, score, and highlights
- [ ] **DASH-04**: User can search and Ask from inside the TUI (search box + Ask box over the same memory)

### System

- [ ] **SYS-01**: Every config.toml field is wired or removed — no dead knobs (vector_backend, batch_interval, send_screenshots_to_llm, ui flags); single-instance daemon lock with clear second-instance message
- [ ] **SYS-02**: User can run launchd auto-start with a doctor/status check confirming installed + running + DB writable

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Intelligence

- **INTL-01**: Multi-hop graph traversal for related-work discovery (needs proven single-hop relevance first)
- **INTL-02**: Rule-based project/client auto-tagging (Timing-style keyword rules) with invoicing CSV export

### Platform

- **PLAT-01**: Windows/Linux capture backends (currently stub returns UnsupportedPlatform)
- **PLAT-02**: Cross-device sync and cloud backup of local memory

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Cloud upload of raw screenshots/keystrokes by default | Radioactive trust cost for a keystroke+screenshot tool; violates local-first constraint — redacted text only, explicit opt-in |
| Verbatim long-term keystroke archive | Keylogger optics + DB-theft blast radius; store context + summaries, short rolling buffer only |
| Distraction blocking / focus enforcement | Adversarial UX; solo-dev tool informs, does not punish |
| Employer/team surveillance | Destroys solo-user trust; single-user local tool only |
| Manual timers as primary input | Contradicts automatic-memory core value; manual correction (re-tag/split/merge) instead |
| Real-time cloud dashboard / web UI | Second product (sync+auth+hosting); TUI + CLI + local export instead |
| Full auto-billing/invoicing suite | Second domain; clean CSV export lets real invoicing tools consume it |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| CAPT-01 | Phase 1 | Pending |
| CAPT-02 | Phase 1 | Pending |
| CAPT-03 | Phase 1 | Pending |
| CAPT-04 | Phase 1 | Pending |
| CAPT-05 | Phase 1 | Pending |
| CAPT-06 | Phase 1 | Pending |
| CAPT-07 | Phase 1 | Pending |
| PRIV-01 | Phase 2 | Pending |
| PRIV-02 | Phase 2 | Pending |
| PRIV-03 | Phase 2 | Pending |
| PRIV-04 | Phase 2 | Pending |
| CLASS-01 | Phase 3 | Pending |
| CLASS-02 | Phase 3 | Pending |
| CLASS-03 | Phase 3 | Pending |
| MEM-01 | Phase 4 | Pending |
| MEM-02 | Phase 4 | Pending |
| MEM-03 | Phase 4 | Pending |
| MEM-04 | Phase 4 | Pending |
| SRCH-01 | Phase 5 | Pending |
| SRCH-02 | Phase 5 | Pending |
| DASH-01 | Phase 5 | Pending |
| DASH-02 | Phase 5 | Pending |
| DASH-03 | Phase 5 | Pending |
| DASH-04 | Phase 5 | Pending |
| SYS-01 | Phase 1 | Pending |
| SYS-02 | Phase 1 | Pending |

**Coverage:**
- v1 requirements: 26 total
- Mapped to phases: 26
- Unmapped: 0

---
*Requirements defined: 2026-10-03*
*Last updated: 2026-10-03 after roadmap creation (26/26 mapped)*
