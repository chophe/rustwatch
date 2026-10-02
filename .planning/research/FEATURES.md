# Feature Research

**Domain:** Personal activity-memory / automatic time-tracking (macOS, local-first, solo developer)
**Researched:** 2026-10-02
**Confidence:** HIGH (cross-checked across ActivityWatch comparison, RescueTime/Rize docs, Timing/Qbserve docs, Pieces, Screenpipe/Rewind coverage)

## Feature Landscape

### Table Stakes (Users Expect These)

Features users assume exist. Missing these = product feels incomplete or untrustworthy. Every credible competitor (ActivityWatch, RescueTime, Timing, Qbserve, ManicTime) ships all of these.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Silent background capture (app + window title polling) | An automatic tracker that requires manual start/stop is a contradiction; users expect zero-friction capture | LOW | rustwatch has this (clipboard + active-window polling). Keep poll interval modest (5–10s); sub-second polling is an anti-feature |
| Idle detection + auto-pause | Without it, lunch/coffee inflates every report and users stop trusting the numbers; Timing/Qbserve/ActivityWatch all do this | MEDIUM | No explicit idle detector in rustwatch yet — gap. Implement via CGEventSource idle time; fold idle spans out of segments. Required before scoring means anything |
| Sleep/wake survival | Laptops sleep constantly; a daemon that dies or double-counts across sleep is perceived as broken | MEDIUM | Active requirement in PROJECT.md. Handle power notifications, gap-tolerant segmentation |
| Timeline view of the day | The single most-used screen in every tracker (ActivityWatch timeline, Timing detail view, Pieces activity feed) | MEDIUM | rustwatch TUI needs this as the default view, not a secondary screen |
| Time-by-app/category aggregation | "Where did my day go?" is the core question; pie/bar breakdown is the minimum answer | LOW | `chart` exists in CLI; needs TUI parity + category rollup (currently cloud-LLM dependent — needs graceful unknown-category state) |
| Keyword search over history | Users remember fragments ("that Stripe docs page"); every tool offers at least title/URL search | LOW | rustwatch has FTS5 table but it is reportedly unused — wiring it up is table stakes, not a differentiator |
| Local-first storage by default | ActivityWatch/Qbserve/ManicTime/Screenpipe all store locally; post-Rewind-shutdown (Dec 2025) users distrust cloud-memory startups | LOW | Already the architecture; must be *visible* in UX (status line showing "all data local") |
| Per-app / per-site exclusions | Banking, password managers, health, personal chat — users will not run a tracker they cannot blindfold; Timing has an Exclusions tab, Qbserve excludes private browsing | MEDIUM | Active requirement. Must apply at capture time (never stored), not just hidden at query time |
| Pause / private mode | "Pause for 30 min" is in every tracker; without it users quit the daemon instead of pausing it | LOW | Hotkey or tray/CLI toggle; auto-resume timer expected |
| macOS permissions onboarding + detection | Input Monitoring + Accessibility + Screen Recording gates kill every macOS tracker on first run; Timing/Qbserve all walk users through it | LOW | Active requirement ("correct permissions detection"). Detect each TCC state, deep-link to Settings pane, degrade gracefully per missing grant |
| Auto-start on login (launchd) | A memory tool with gaps isn't memory; users expect it running after reboot without thinking | LOW | `launchd` install exists; needs a `doctor`/status check confirming installed + running |
| Data export (JSON/CSV) | Lock-in fear, especially for local tools; ActivityWatch/Timing export; trivial to offer, expensive to omit in trust terms | LOW | `export` exists in CLI — keep it, add memory/Q&A-scope export |
| Retention + storage budget | Screenshot capture grows disk unboundedly; users expect "keep N days / max X GB" with automatic pruning | LOW | Sharded screenshots/ exist; needs a prune policy wired to config, else disk-full is the #1 support ticket |

### Differentiators (Competitive Advantage)

Features that set rustwatch apart. Align with Core Value (reliable local memory) and the Pieces/Screenpipe-shaped gap left by Rewind's shutdown.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Natural-language Ask / Q&A over memory ("what did I work on today?") | The headline use case in PROJECT.md and the Rewind/Pieces promise; turns a log into memory | HIGH | Needs hybrid retrieval + LLM synthesis; quality depends entirely on memory-correctness work (real embeddings, graph traversal). Empty/wrong answers destroy trust — gate behind retrieval-confidence thresholds |
| LLM auto-classification into activities/topics | Removes the manual tagging tax (ManicTime's weakness); Rize/Rize-style auto-categorization is why users pay | MEDIUM | Exists (OpenAI/Anthropic) but is the fragility point: cost, latency, offline gaps. Local-provider fallback (Ollama/llama.cpp) converts this from liability to differentiator |
| Multi-provider LLM choice incl. local endpoint | Nobody else offers cloud + local classification side-by-side in one tool; matches user's stated preference and hedges API cost/outage | MEDIUM | Active requirement. Design as a trait, not a flag — each new provider otherwise becomes a rewrite |
| Semantic + hybrid + graph memory search | Keyword-only search (ActivityWatch level) can't answer "that auth refactor discussion"; Pieces' LTM and Screenpipe's AI search are the bar | HIGH | Vector store + FTS5 + graph exist structurally but reportedly hollow (hash-fallback embedder, unused FTS, hops-as-LIMIT). Making them real is the highest-leverage engineering in the project |
| Screenshot memory (visual timeline + recall) | Screenpipe/Rewind proved screenshots are the highest-fidelity memory signal; window-change-triggered capture is cheap and effective | MEDIUM | Exists via xcap; three triggers (interval + window-change + hotkey) already decided. Differentiator is *recall* (show me the screenshot in search results), not just capture |
| Hotkey-annotate to daily log | Frictionless journaling ("⌨ note this") attached to the timeline; nobody in the comparison set does annotated-capture well — it's the bridge between passive tracking and active journaling | LOW | Active requirement. Keep the hotkey global, the note UI instant (<200ms to visible), attach note to current segment |
| Daily productivity score + goals | RescueTime's signature feature and Rize's daily email; gives users a reason to open the app every morning | MEDIUM | Scoring rubric must be transparent and user-tunable (RescueTime's fixed categories frustrate devs whose "distracting" sites are work). Score-from-categories × user weights, never a black box |
| Daily/weekly digest (morning brief) | Rize's "beautiful report in your inbox" is a retention engine; a terminal-native version (CLI `digest` + TUI card) fits solo-dev workflow | LOW | Cheap once classification works; pure presentation layer. High perceived value per effort |
| MCP server for agent access | Pieces ships an MCP server for long-term memory; letting the user's own coding agents query work history is unique to the AI-native segment | LOW | 5 tools exist; differentiator is quality of retrieval underneath, plus scoped permissions (agents shouldn't read banking-app titles either) |
| Secret-redaction gate before any cloud egress | Post-2024 delineator: Screenpipe stays local-first, enterprise buyers demand it; "passwords/tokens never reach the LLM" as a *verified* property is marketable trust | MEDIUM | Regex gate exists; needs real-secret test corpus + audit log of what was redacted. Consider it a feature (redaction report), not just plumbing |
| Project/client auto-tagging from signals | Rize/Timely auto-assign time to projects/clients; for freelancers this converts tracking into invoices | HIGH | Valuable but model-heavy; rule-based v1 (per-app/per-path project rules a la Timing keyword rules) captures 80% at LOW cost. Full auto-tagging is v2+ |

### Anti-Features (Commonly Requested, Often Problematic)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Cloud upload of raw screenshots/keystrokes by default | Enables cloud AI features, cross-device | Radioactive for a keystroke+screenshot tool; Rewind's arc shows the trust cost; violates PROJECT.md privacy constraint | Redacted-text-only egress, explicit per-feature opt-in, screenshots never leave device (dead `send_screenshots_to_llm` config must stay dead or explicitly opt-in) |
| Verbatim keystroke content stored long-term | "Perfect recall" of everything typed | Keylogger optics + blast radius on DB theft; passwords typed into non-password fields bypass field-level redaction | Store window/app context + derived summaries, never raw key streams; short rolling buffer only |
| Distraction blocking / focus enforcement | RescueTime's premium hook; sounds productive | Adversarial relationship with the user; solo-dev tool should inform, not punish; scope creep into accessibility APIs | Score + digest + gentle daily goals; blocking is a separate product |
| Employer/team surveillance features | Obvious monetization path (Time Doctor model) | Destroys solo-user trust irreparably; contradicts local-first positioning; multi-device/team is explicitly out of scope | Stay single-user; per-day productivity is personal analytics |
| Manual timers as primary input | Toggl familiarity | Contradicts automatic-memory core value; dual systems (manual + auto) create reconciliation UX nobody finishes | Auto-capture primary; manual *correction* (re-tag a segment, split/merge) instead of manual timers |
| Sub-second / always-on high-frequency capture | "More data = better memory" intuition | Battery + CPU + DB bloat for zero recall gain; macOS throttles aggressively | 5s window poll, 5-min screenshot interval, event-driven window-change captures (already the decided design) |
| Real-time cloud dashboard / web UI | Feels modern | Requires sync infra, auth, hosting — an entire second product contradicting local-only scope | TUI dashboard + local CLI; export JSON for anyone wanting their own web view |
| Windows/Linux capture in v1 | Wider audience | macOS APIs (CGEventTap, xcap, TCC) don't transfer; stub exists and returns Unsupported — keep it a stub | macOS-only v1 per PROJECT.md; revisit only with a dedicated platform phase |
| Full auto-project billing/invoicing suite | Freelancer money argument | Invoicing rules, rates, clients, tax — a second domain; Qbserve does this and it's half their codebase | Export clean per-project hours (CSV); let real invoicing tools consume it |

## Feature Dependencies

```
[Ask Q&A]
    └──requires──> [Hybrid search returns relevant results]
                       └──requires──> [Real embeddings end-to-end]
                       └──requires──> [Classification produces topics]
                                           └──requires──> [Reliable capture + redaction gate]

[Productivity score]
    └──requires──> [Classification produces categories]
    └──requires──> [Idle detection] (else score is fiction)

[Daily digest]
    └──requires──> [Classification] + [Score]

[Hotkey-annotate]
    └──requires──> [Timeline/segment model] (note attaches to a segment)

[MCP agent tools]
    └──requires──> [Hybrid search] + [Exclusion enforcement at query time]

[Exclusions / Pause] ──conflicts──> [Memory completeness]
    (documented tension: excluded time is invisible to Ask; UX must say "2h excluded today")

[Cloud classification] ──conflicts──> [Offline operation]
    (local-provider fallback resolves the conflict — hence its priority)
```

### Dependency Notes

- **Ask requires hybrid search requires real embeddings:** the #1 dependency chain in the project. Every "intelligence" feature is presentation over retrieval; retrieval is currently the hollow part (hash embedder, unused FTS, hops-as-LIMIT). This chain dictates phase ordering.
- **Score requires idle detection:** scoring un-idled data produces numbers users can disprove by inspection ("I was at lunch"), permanently devaluing the score feature.
- **Exclusions conflict with completeness:** not a reason to drop exclusions (they're table stakes for trust) — but Ask/digest UX must disclose excluded gaps rather than silently presenting partial days as whole.
- **Cloud vs offline conflict:** resolved by local-provider fallback, which is why multi-provider support is a P1 enabler, not a P2 nicety.

## MVP Definition

Ruthless cut: the MVP is *reliable memory you can interrogate*, not a full tracker suite.

### Launch With (v1)

- [ ] Reliable capture (keyboard context + window poll + 3 screenshot triggers) with sleep survival and no non-ASCII panics — without this nothing else matters (Core Value)
- [ ] Permissions detection + onboarding — else half of installs silently capture nothing
- [ ] Idle detection — else all downstream numbers are wrong
- [ ] Redaction gate + per-app exclusions + pause — else the tool is unsafe to run
- [ ] Segment folding (event stream → sessions) — the unit everything else aggregates
- [ ] Classification (one cloud provider working + local fallback) — categories/topics
- [ ] Keyword search (wire up existing FTS5) — minimum viable recall
- [ ] Timeline + time-by-app/category TUI dashboard — the daily answer
- [ ] Config honesty (every field wired or removed) — dead knobs destroy trust in a privacy tool
- [ ] Export (JSON/CSV) — trust + escape hatch

### Add After Validation (v1.x)

- [ ] Real embeddings + hybrid search — trigger: keyword search demonstrably misses ("I know it's in there")
- [ ] Ask Q&A (CLI → TUI → MCP in that order) — trigger: hybrid search relevance judged good on a fixed eval set
- [ ] Productivity score + daily digest — trigger: classification stable for 2+ weeks of dogfooding
- [ ] Hotkey-annotate — trigger: timeline/segment model stable (small, but depends on it)
- [ ] MCP server hardening (scoped permissions) — trigger: user actually wires agents to it
- [ ] Retention pruning policy — trigger: first disk-usage complaint or 30 days of screenshots, whichever first

### Future Consideration (v2+)

- [ ] Graph expansion that truly traverses (multi-hop related work) — defer: needs proven single-hop relevance first
- [ ] Rule-based project tagging (Timing-style keyword rules) — defer: manual rules UI is real work, validate demand first
- [ ] Full auto project/client detection + invoicing export — defer: second domain
- [ ] Cross-device / sync — defer: explicitly out of scope, contradicts local-first trust story

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| Reliable capture + sleep survival + panic fixes | HIGH | MEDIUM | P1 |
| Permissions detection/onboarding | HIGH | LOW | P1 |
| Idle detection | HIGH | MEDIUM | P1 |
| Redaction gate + exclusions + pause | HIGH | MEDIUM | P1 |
| Segment folding (existing, harden) | HIGH | LOW | P1 |
| Classification (cloud + local fallback) | HIGH | MEDIUM | P1 |
| Keyword (FTS5) search wired up | HIGH | LOW | P1 |
| Timeline + breakdown dashboard (TUI) | HIGH | MEDIUM | P1 |
| Config honesty cleanup | MEDIUM | LOW | P1 |
| Export | MEDIUM | LOW | P1 |
| Real embeddings + hybrid search | HIGH | HIGH | P2 |
| Ask Q&A | HIGH | HIGH | P2 |
| Productivity score + digest | HIGH | MEDIUM | P2 |
| Hotkey-annotate | MEDIUM | LOW | P2 |
| MCP hardening/scoping | MEDIUM | LOW | P2 |
| Retention pruning | MEDIUM | LOW | P2 |
| Graph multi-hop traversal | MEDIUM | HIGH | P3 |
| Rule-based project tagging | MEDIUM | MEDIUM | P3 |
| Auto-billing/invoicing | LOW | HIGH | P3 |
| Web dashboard / cloud sync / teams | LOW | HIGH | P3 (out of scope) |
| Distraction blocking | LOW | MEDIUM | P3 (anti-feature, don't) |

**Priority key:**
- P1: Must have for launch
- P2: Should have, add when possible
- P3: Nice to have, future consideration

## Competitor Feature Analysis

| Feature | ActivityWatch | RescueTime | Timing / Qbserve (Mac-only) | Pieces / Screenpipe | Our Approach |
|---------|---------------|------------|-----------------------------|---------------------|--------------|
| Capture | App+window+idle, local, open source | App+web, cloud-stored, blocking+alerts | Deep Mac integration (doc paths, URLs), invoice export, exclusions tab | Screen/audio/IDE stream into AI timeline; local-first | Match: keyboard+screen, macOS-native, local-first |
| Classification | Manual buckets/rules | Auto productivity categories + score | Keyword rules, project assignment | LLM-driven LTM, semantic recall | LLM auto-classify (cloud + local choice) — exceeds rules-based |
| Memory search | Keyword, custom queries | Reports, limited search | Timeline search | Natural-language AI search + MCP server | Hybrid vector+keyword+graph + Ask + MCP — match the AI bar, keep data local |
| Scoring/goals | None (DIY) | Productivity score + goals + alerts (signature) | Project hours, invoicing | None (recall, not productivity) | Adopt RescueTime-style score, transparent + user-weighted |
| Privacy | Local, strongest OSS story | Cloud-stored (trust-me model) | Local (Qbserve), Mac sandbox | Local-first, redact/controls | Strongest story: local + verified redaction gate + exclusions + screenshots never leave |
| Digest/reporting | Custom dashboards | Daily review, weekly reports | Invoices, timesheets | Timeline/activity feed | Terminal-native digest (CLI + TUI card) before any email/web |
| Pricing pressure | Free OSS | $7–15/mo | ~$10+/one-time-plus | Freemium/cloud-tied | Free local tool; user's only cost is optional LLM API usage |

## Sources

- ActivityWatch vs RescueTime/ManicTime/ScreenTime comparison (activitywatch.net/blog/comparing-time-trackers) — HIGH confidence, vendor-authored but factually detailed
- RescueTime vs Rize scoring/categorization coverage (rize.io, focusmo.app, rescuetime.com) — MEDIUM-HIGH, vendor + review consensus
- Timing exclusions/idle/preferences docs (timingapp.com/help) — HIGH, primary source
- Qbserve Mac feature/privacy docs (qotoqot.com/qbserve) — MEDIUM-HIGH, primary source
- Pieces LTM-2.7 / timeline / MCP coverage (pieces.app, docs.pieces.app, Product Hunt, dev.to hands-on) — MEDIUM, marketing + independent hands-on agree on shape
- Screenpipe vs Rewind / local-first positioning (docs.screenpipe.com, screenpipe.com) — MEDIUM, vendor-authored; Rewind shutdown Dec 2025 corroborated across sources
- PROJECT.md validated/active requirements — HIGH, grounds MVP cut in actual codebase state

---
*Feature research for: local activity memory / personal productivity tracker (rustwatch)*
*Researched: 2026-10-02*
