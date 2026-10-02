---
last_mapped_commit: c8ba2a9e65b063c9dbc60fc393d554bb99e9ca7a
last_mapped_at: 2026-10-02
---
# Codebase Concerns

**Analysis Date:** 2026-10-02

Rust workspace for a macOS activity-memory daemon. Seven crates, ~3,200 lines of Rust, **zero tests, zero CI**. Every finding below is verified against source at the cited path and (where noted) reproduced with a command.

---

## Tech Debt

### `rustwatch-memory-backends` is orphaned from the workspace and cannot be built

- **Issue:** The crate exists at `crates/rustwatch-memory-backends/` but is absent from the `members` array in `Cargo.toml:3-11`. It also does not use `version.workspace = true` like its siblings (`crates/rustwatch-memory-backends/Cargo.toml:3-6`), so it is fully detached from workspace dependency versions.
- **Evidence:** `cargo metadata --manifest-path crates/rustwatch-memory-backends/Cargo.toml` fails with:
  ```
  error: current package believes it's in a workspace when it's not:
  current:   .../crates/rustwatch-memory-backends/Cargo.toml
  workspace: .../Cargo.toml
  ```
  `lancedb`, `surrealdb`, and `arrow-array` appear **0 times** in `Cargo.lock`, so this crate has never been resolved or compiled.
- **Files:** `Cargo.toml:3-11`, `crates/rustwatch-memory-backends/Cargo.toml`, `crates/rustwatch-memory-backends/src/lance.rs` (134 lines), `crates/rustwatch-memory-backends/src/surreal.rs` (124 lines)
- **Impact:** The build command documented in `README.md` ("Optional LanceDB + SurrealDB backends") is broken. 258 lines of code in `lance.rs` + `surreal.rs` are unverified, never compiled, and may not even type-check. Arrow is pinned to `53` while `lancedb 0.17` almost certainly resolves a different major — the `RecordBatch::try_new` / `FixedSizeListArray` calls at `lance.rs:110-130` are guesses.
- **Fix:** Add `"crates/rustwatch-memory-backends"` to `workspace.members`, switch its manifest to `*.workspace = true`, then `cargo check -p rustwatch-memory-backends`. Alternatively add it to `workspace.exclude` plus an empty `[workspace]` table, and stop documenting it as buildable. `docs/TESTING_PLAN.md` already lists this fix under Phase 0 step 4.

### Config surface is ~40% dead

Config fields that are declared, defaulted, documented to users, and **never read** by any code path:

| Field | Declared | Read anywhere? |
|---|---|---|
| `analyze.vision_model` | `crates/rustwatch-core/src/config.rs:35` | No |
| `analyze.batch_interval_minutes` | `crates/rustwatch-core/src/config.rs:36` | No |
| `privacy.send_screenshots_to_llm` | `crates/rustwatch-core/src/config.rs:58` | No |
| `memory.vector_backend` | `crates/rustwatch-core/src/config.rs:41` | No |
| `memory.graph_backend` | `crates/rustwatch-core/src/config.rs:42` | No |
| `memory.surreal_engine` | `crates/rustwatch-core/src/config.rs:43` | No |
| `data.sqlite_path` / `lance_path` / `surreal_path` | `crates/rustwatch-core/src/config.rs:18-20` | No |
| `ui.tui_enabled` / `ui.progress_bars` | `crates/rustwatch-core/src/config.rs:51-52` | No |
| `Config::paths()` | `crates/rustwatch-core/src/config.rs:107` | Never called |
| `capture.accessibility_poll_ms` | `crates/rustwatch-core/src/config.rs:26` | Accepted, prefixed `_`, ignored (`macos.rs:61`) |

- **Impact:** Users editing `config.toml` get silent no-ops. `send_screenshots_to_llm = false` reads like a privacy switch but nothing implements it. The whole `DataConfig` struct is dead — `Store::open(&paths.sqlite)` (`crates/rustwatch-daemon/src/main.rs:36`) uses `DataPaths`, not config.
- **Fix:** Either wire each field or delete it. Do not ship a config key that a user can set and observe no effect on.

### Dead code that costs real resources

- **FTS5 index written, never queried.** `crates/rustwatch-memory/src/sqlite_store.rs:45-50` creates `memory_fts` and `:78-82` writes a row on every upsert — but no `MATCH` query exists anywhere in the repo. Pure write amplification on every ingest.
- **`rmcp` optional dependency, never used.** Declared at `crates/rustwatch-mcp/Cargo.toml:24-31`, feature `rmcp` is off by default and `crates/rustwatch-mcp/src/main.rs` hand-rolls the JSON-RPC loop. It appears in `Cargo.lock` and is compiled into nothing.
- **`hash_content()` never called.** `crates/rustwatch-capture/src/platform/macos.rs:355-359`.
- **`CaptureEventKind::Copy` is never constructed.** Declared at `crates/rustwatch-core/src/events.rs:24-27`; the only reference is the match arm at `crates/rustwatch-core/src/segment.rs:54`. Clipboard content is captured on paste but never on copy.
- **`bundle_id` is always `None`.** `crates/rustwatch-capture/src/platform/macos.rs:238` hardcodes it; the column exists in `crates/rustwatch-core/migrations/V1__initial.sql:21`.
- **`ScreenshotRecord.segment_id` is always `None`.** `crates/rustwatch-daemon/src/main.rs:74` — the writer has the `ActiveSegment` in hand but never links the screenshot to it.
- **`SurrealGraphStore::open(path)` ignores `path`.** `crates/rustwatch-memory-backends/src/surreal.rs:13-20` takes a path, `create_dir_all`s it, then opens `Surreal::new::<Mem>(())`. The whole graph is discarded on process exit while `paths.surreal` and `surreal_path` are created and reported as the data layout in `README.md`.

### README documents a data path the code never uses

- **Issue:** `README.md` states config is "Created at `~/.rustwatch/config.toml`" and lists `~/.rustwatch/rustwatch.db` etc. `Config::default()` also says `~/.rustwatch` (`crates/rustwatch-core/src/config.rs:63`). But every binary calls `DataPaths::new(None)` (`crates/rustwatch-cli/src/main.rs:84`, `crates/rustwatch-daemon/src/main.rs:24`, `crates/rustwatch-mcp/src/main.rs:16`), which resolves through `ProjectDirs::from("com","chophe","rustwatch")` at `crates/rustwatch-core/src/paths.rs:52-53` → on macOS `~/Library/Application Support/com.chophe.rustwatch`.
- **Impact:** Every README command in Quick start operates on a path the user has never heard of. Anyone trying to inspect or delete their data looks in the wrong place — a privacy problem for a keystroke logger.
- **Fix:** Make `DataPaths::new(Some(config.data.dir.clone()))` authoritative, or change the docs. Pick one and assert it in a test.

### `install` makes a repo path a build dependency

- **Issue:** `crates/rustwatch-cli/src/commands.rs:29` does `include_str!("../../../deploy/macos/com.rustwatch.plist")`. Moving or renaming `deploy/` breaks the build with an opaque `couldn't read` error, and the binary cannot be built outside this repo layout.
- **Adjacent fragility:** `commands::install` (`:23-27`) and `commands::start` (`:44-48`) both locate `rustwatchd` via `current_exe().parent().parent().join("rustwatchd")`. Under `cargo run` that resolves to `target/rustwatchd`, which does not exist; under a global install it assumes both binaries share a parent directory. Neither case is checked — `install` writes a broken `ProgramArguments` path into a launchd plist and reports success.
- **Fix:** Embed the plist via `rustfs`/`include_dir` over a packaged resource dir, or resolve the sibling path with an existence check and a clear error.

### Error-handling paradigms are mixed within one crate

`crates/rustwatch-core/src/error.rs` defines a `thiserror` enum with a stringly-typed `Other(String)` catch-all, used to wrap migration failures at `crates/rustwatch-core/src/db.rs:23-25` and `xcap` errors at `crates/rustwatch-capture/src/platform/macos.rs:309`. Meanwhile `crates/rustwatch-core/src/paths.rs:81` returns `anyhow::Result<crate::Config>` from a crate whose own error type exists. Callers then convert between the two.

- **Fix:** Pick one boundary convention. `rustwatch-core` returns `crate::Result<T>`; binary crates use `anyhow`.

---

## Known Bugs

### PANIC: byte-slice on a non-char boundary in `memory search`

- **Symptoms:** `rustwatch memory search "<query>"` aborts the process with `byte index 120 is not a char boundary; it is inside 'X' (bytes 118..121) of string`.
- **Files:** `crates/rustwatch-cli/src/commands.rs:240-242`
  ```rust
  let snippet = if hit.text.len() > 120 {
      format!("{}...", &hit.text[..120])
  ```
- **Trigger:** Any hit whose `text` exceeds 120 bytes with a multi-byte character before offset 120. The text is assembled from window titles and captured keystrokes (`crates/rustwatch-memory/src/lib.rs:34-37`), so any emoji, accented Latin, CJK, or curly quote does it. With the default fake embedder every chunk scores nonzero, so **any** query that returns ≥1 hit over a long text can panic.
- **Fix:** Use `char_indices`/`chars().take(120)` for the snippet, matching the correct approach already used at `crates/rustwatch-core/src/segment.rs:116-118`.

### PANIC: `String::truncate` on a non-char boundary in the LLM redaction path

- **Symptoms:** `rustwatch analyze` panics before any segment reaches the LLM.
- **Files:** `crates/rustwatch-analyze/src/redact.rs:30-32`
  ```rust
  if out.len() > self.max_chars {
      out.truncate(self.max_chars);
  }
  ```
  `max_chars` is `config.memory.chunk_max_chars` (default 2000, `crates/rustwatch-core/src/config.rs:91`). `String::truncate` panics whenever `max_chars < len` and is not on a char boundary.
- **Trigger:** A captured `text_buffer` over 2000 bytes whose 2000th byte lands mid-character. Multi-byte content in any segment pushes this from unlikely to routine.
- **Fix:** `out.chars().take(self.max_chars).collect()`.

### PANIC: `replace_range` on a non-char boundary in the capture buffer

- **Symptoms:** The daemon writer task panics; the async task dies silently and no further events are ever persisted.
- **Files:** `crates/rustwatch-core/src/segment.rs:106-114`
  ```rust
  if buffer.len() + text.len() > MAX_BUFFER_CHARS {   // MAX_BUFFER_CHARS = 16_384
      let keep = MAX_BUFFER_CHARS.saturating_sub(text.len());
      if keep < buffer.len() {
          buffer.replace_range(..buffer.len() - keep, "");
  ```
  `buffer.len() - keep` is an arbitrary byte offset; `replace_range` panics if it is not a char boundary.
- **Trigger:** Sustained typing (>16 KiB of buffer, ~1,600 lines) containing any multi-byte character. This is the single most likely panic in the whole system.
- **Fix:** Operate on char boundaries, or store the buffer as `Vec<char>`/a ring buffer with explicit byte-safe eviction.

### `meta_held` latches on after Cmd+Tab, causing permanent clipboard reads

- **Symptoms:** After any Cmd-combo that changes focus (Cmd+Tab is the canonical case), `meta_held` stays `true` forever. Every subsequent `Key::V` triggers a **clipboard read** (`macos.rs:128-136`), and every key event is labelled with the `Meta` modifier (`macos.rs:165-171`).
- **Files:** `crates/rustwatch-capture/src/platform/macos.rs:120-160` — `meta_held` is set on `KeyDown(MetaLeft|MetaRight)` and cleared only on a matching `KeyUp`. Cmd+Tab moves focus while Meta is down; the `KeyUp` for Meta is delivered to the *newly focused* app's tap context and is routinely missed. There is no timeout, no cross-check against `current_app_context`, and no reset on focus change.
- **Impact:** Continuous `NSPasteboard` traffic and wrong modifier metadata on the entire rest of the session.
- **Fix:** Read live modifier state from the event's modifier flags rather than tracking press/release, or reset `meta_held` inside `run_focus_loop` on every `FocusChange`.

### Shift is not tracked, so captured text is always lowercase

- **Files:** `crates/rustwatch-capture/src/platform/macos.rs:252-296` — `key_to_text` maps `Key::A => "a"` unconditionally; `active_modifiers` (`:165-171`) only ever reports `Meta`. No CapsLock handling, no Shift, no non-US layouts.
- **Impact:** `text_buffer` — the primary input to LLM classification and to memory embeddings — is systematically corrupted for roughly half of all typed characters. This is a correctness bug in the product's core data, not just capture.
- **Fix:** Derive the character from `KeyEvent` modifiers, or drop `TextDelta` synthesis entirely and rely on the accessibility text-field snapshot.

### `rustwatch permissions` always reports everything as missing

- **Files:** `crates/rustwatch-capture/src/platform/macos.rs:45-55` returns a hardcoded struct with `input_monitoring: false, accessibility: false, screen_recording: false`.
- **Impact:** `README.md` tells users to run `rustwatch permissions` before `rustwatch start`. The command always prints `missing missing missing`, which means nothing. The user cannot diagnose a TCC failure, and the notes ("After granting Screen Recording, restart rustwatchd") are the only useful output.
- **Fix:** Query the actual TCC status (`AXIsProcessTrustedWithOptions`, `CGPreflightScreenCaptureAccess`), or delete the command and the README step rather than shipping a diagnostic that lies.

### `capture_screenshot` reports success without writing a file

- **Files:** `crates/rustwatch-capture/src/platform/macos.rs:320-346` — in the `ScreenshotScope::Window` arm, if no focused window is found **and** `Monitor::all()` fails or is empty, control falls out of both branches, hits `debug!` at `:344`, and returns `Ok(path)` for a file that was never created.
- **Impact:** `DaemonReply::Screenshot { path }` reports a path that does not exist. `rustwatch screenshot` prints "Saved screenshot: …" (`crates/rustwatch-cli/src/commands.rs:154`). No `Screenshot` row is inserted into the DB (`crates/rustwatch-daemon/src/main.rs:68-76`), so the record of the failure vanishes too.
- **Fix:** Add an `else { return Err(...) }` arm.

### Stale pid file deadlocks the daemon lifecycle

- **Symptoms:** After any unclean daemon exit (crash, `kill -9`, logout), `rustwatch start` prints "Daemon already appears to be running" forever and never starts a daemon.
- **Files:** `crates/rustwatch-cli/src/commands.rs:39-42` gates on `paths.pid_file.exists()` with **no liveness check**. `crates/rustwatch-daemon/src/main.rs:42` writes the pid but installs **no SIGTERM/SIGINT handler** and never removes the file, so the default signal disposition kills the process and orphans the file. Only `commands::stop` (`:71`) removes it.
- **Fix:** Check liveness with `kill(pid, 0)` before short-circuiting; remove the pid file in a `Drop`/signal handler on the daemon side.

### `rustwatch stop` sends SIGTERM to an unvalidated pid

- **Files:** `crates/rustwatch-cli/src/commands.rs:66-70` parses whatever integer is in `daemon.pid` and calls `libc::kill(pid, SIGTERM)` with no check that the process is `rustwatchd`.
- **Impact:** Combined with PID reuse, or a corrupted/attacker-written pid file inside the data dir, `rustwatch stop` terminates an unrelated process.
- **Fix:** Verify the process name (or use a pid file with the start time) before signalling.

### The MCP server dies on one malformed input line

- **Files:** `crates/rustwatch-mcp/src/main.rs:30` — `let request: Value = serde_json::from_str(&line)?;` is inside the `for line in stdin.lock().lines()` loop with `?`. A single bad JSON line propagates out of `main` and terminates the server. `handle_tool` at `:56` has the same problem: any tool error (`chrono::ParseError` at `:96`/`:103`, DB error, daemon unreachable at `:118`) kills the process.
- **Impact:** One malformed client message permanently breaks the MCP integration until the host restarts it. Combined with `DaemonNotRunning` being a normal condition, `pause_capture`/`resume_capture` when the daemon is down terminates the server.
- **Fix:** Wrap the per-line handling in `match` and return a JSON-RPC `error` object instead of propagating.

### The MCP server emits protocol-invalid error responses

- **Files:** `crates/rustwatch-mcp/src/main.rs:58` returns `json!({"error": format!("unknown method {method}")})` as the **`result`** field of a success envelope (`:61`). JSON-RPC 2.0 requires a top-level `error` member with a numeric `code` and string `message`, and `result` must be omitted.
- **Impact:** Strict MCP clients reject or misinterpret unknown-method responses.
- **Fix:** Return `{"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":...}}`.

### No timeout on any IPC call — the TUI can hang permanently

- **Files:** `crates/rustwatch-core/src/ipc.rs:61-66` (`DaemonClient::send`) and `:74-79` (`handle_connection`) both `read_exact` on a length-prefixed frame with **no `tokio::time::timeout`**. `rg 'timeout'` over the whole repo returns nothing.
- **Trigger:** `crates/rustwatch-cli/src/tui.rs:31` calls `client.send(DaemonCommand::Status)` every 250 ms. If the daemon accepts the connection then stalls (blocked on `try_lock` contention, or a partially-written frame), the TUI blocks inside `.await` — and because it is blocked it never calls `event::read()`, so **`q` and Ctrl-C stop working**. The only escape is killing the process from another terminal.
- **Fix:** Wrap both read/write sequences in `tokio::time::timeout(Duration::from_secs(2), ...)`.

### `analyze_pending` assigns every segment to every activity

- **Files:** `crates/rustwatch-analyze/src/classifier.rs:48-64` — inside the `for label in labels` loop, `segment_ids: filtered.iter().map(|s| s.id.clone()).collect()`.
- **Impact:** A batch of up to 50 segments (`classifier.rs:22`) produces N activity rows each claiming **all 50** segments, regardless of which segments the LLM actually attributed to that activity. The `segment_ids` relation is meaningless, and it is the join key used by `list_unanalyzed_segments` (`crates/rustwatch-core/src/db.rs:161`). Every downstream graph edge (`crates/rustwatch-memory/src/graph.rs:48-70`) and every memory chunk's `segment_id` (`crates/rustwatch-memory/src/lib.rs:73`) inherits the corruption.
- **Fix:** Have the LLM return the segment ids it used per activity and validate them against the batch.

### `list_unanalyzed_segments` uses an unindexable `LIKE` and can produce false negatives

- **Files:** `crates/rustwatch-core/src/db.rs:157-165`
  ```sql
  LEFT JOIN activities a ON a.segment_ids_json LIKE '%' || s.id || '%'
  WHERE a.id IS NULL
  ```
- **Impact:** Full scan of `segments` × `activities` with no usable index. Worse, it is a *string containment* test on a JSON blob: a segment id that happens to appear as a substring of a stored `segment_ids_json` marks a *different* segment as analyzed, so it is silently skipped forever. There is no `analyzed` flag, no index, and no way to re-queue.
- **Fix:** Normalize into a `segment_activity(segment_id, activity_id)` join table with a real index.

### `parse_ts` silently rewrites corrupt timestamps to "now"

- **Files:** `crates/rustwatch-core/src/db.rs:252-256`
  ```rust
  DateTime::parse_from_rfc3339(&raw)
      .map(|dt| dt.with_timezone(&Utc))
      .unwrap_or_else(|_| Utc::now())
  ```
- **Impact:** A single malformed `timestamp` value — hand-edited DB, a partial write, a future format change — makes that row appear to have happened at query time. `list_segments_between` and `list_activities_for_date` return it for whatever day is being queried, so timeline queries get polluted with phantom events that are indistinguishable from real ones. There is no log line.
- **Fix:** Return `Result` and surface the parse failure.

### `SegmentGrouper` never persists an in-progress session

- **Files:** `crates/rustwatch-core/src/segment.rs:28-81` — a segment is only returned when a **new** `FocusChange` arrives (`on_focus` at `:72`), or via `flush()` (`:63`).
- **Impact:** `SegmentGrouper::flush()` at `crates/rustwatch-daemon/src/main.rs:79` only runs after `event_rx.recv()` returns `None`, i.e. after **every** sender is dropped — which never happens while the daemon runs. Combined with the absent signal handler (see stale pid bug), an entire workday spent in one application produces **no segment row at all**. The `text_buffer` lives only in memory and is lost on any exit. For an app that runs all day (an editor, a terminal, a browser), this is the common case, not the edge case.
- **Fix:** Add an idle-flush timer (e.g. flush the active segment after N seconds of no events) so long single-app sessions are checkpointed.

### CLI flags and subcommands that lie

- **`rustwatch analyze --today`** — the flag is declared at `crates/rustwatch-cli/src/main.rs:40-43` and discarded at `:99` (`today: _`). `analyze_pending` at `crates/rustwatch-analyze/src/classifier.rs:22` always takes the oldest 50 unanalyzed segments regardless of date. `--today` is accepted, echoed in `--help`, and does nothing.
- **`rustwatch memory graph --around X`** — `crates/rustwatch-cli/src/commands.rs:279-285` is a one-line alias for `memory_search`. There is no way to query the graph directly. The command exists only because `GraphStore::expand_around_apps` is already invoked inside `MemoryEngine::search` (`crates/rustwatch-memory/src/lib.rs:107`).

### Two error-swallowing sites on the daemon hot path

`crates/rustwatch-daemon/src/main.rs:62-77` — every write result is discarded with `let _ =`:

```rust
if let Ok(store) = writer_store.try_lock() {
    let _ = store.insert_event(&event);
    let _ = store.insert_segment(&segment);
    let _ = store.insert_screenshot(&...);
}
```

- **Impact A (silent loss):** If the store mutex is contended, `try_lock` returns `Err` and the **entire event is dropped** — no log, no counter, no retry. The mutex is also taken by the IPC `Tail` handler at `:126`. So `rustwatch tail` actively causes event loss in the daemon, silently.
- **Impact B (silent corruption):** Any `SQLITE_BUSY`, constraint violation, or disk-full error on `insert_event` is dropped. The daemon reports `events_captured=N` while the `events` table holds fewer rows. `crates/rustwatch-cli/src/commands.rs:84-87` prints both numbers side by side and they will not match, with nothing explaining why.
- **Note:** `events_captured` is incremented at `:61` **before** the `try_lock` and before the write, so it counts *observed* events, never *persisted* ones.

---

## Security Considerations

### The daemon socket is unauthenticated and exposes screen capture and full keystroke history

- **Files:** `crates/rustwatch-core/src/ipc.rs:51-67` (client), `:70-86` (server); `crates/rustwatch-daemon/src/main.rs:86-163`
- **Risk:** `~/.rustwatch/daemon.sock` is created by `UnixListener::bind` with default permissions (world-connectable under a typical `022` umask). There is **no peer-credential check** (no `SO_PEERCRED`/`LOCAL_PEERCRED`). Any process running as any local user can connect and issue:
  - `DaemonCommand::Screenshot { window: false }` → capture arbitrary screen contents to disk (`macos.rs:308-319`)
  - `DaemonCommand::Tail { limit: usize }` → dump every captured keystroke, window title, and pasted clipboard content as JSON (`crates/rustwatch-core/src/ipc.rs:35`, `crates/rustwatch-cli/src/commands.rs:128-145`)
  - `Pause` / `Resume` → disrupt capture
- **Impact:** On any multi-user macOS machine this is a remote (local-user) read of everything the user typed, including passwords typed outside the two hardcoded exclude-list apps.
- **Fix:** `chmod 0600` the socket after bind, or verify peer uid via `std::os::unix::net::UnixStream::peer_cred` and reject mismatches. Gate `Screenshot` and `Tail` behind an explicit opt-in flag.

### Unbounded IPC frame length → remote allocation DoS

- **Files:** `crates/rustwatch-core/src/ipc.rs:63-65` and `:76-78`
  ```rust
  let req_len = u32::from_be_bytes(len_buf) as usize;
  let mut req_buf = vec![0u8; req_len];
  stream.read_exact(&mut req_buf).await?;
  ```
- **Risk:** A 4-byte prefix of `0xFFFFFFFF` requests a **4 GiB** allocation. There is no maximum frame size. Combined with the unauthenticated socket above, any local process can abort or OOM the daemon with a 4-byte write.
- **Fix:** Cap at a sane limit (e.g. 8 MiB) and return a `DaemonReply::Error` above it. Also cap `DaemonCommand::Tail { limit }`.

### Keyboard capture ignores the exclude list entirely

- **Files:** `crates/rustwatch-capture/src/platform/macos.rs:71-74` spawns `run_keyboard_loop(tx, paused_kb)` with **no `exclude_apps` parameter**. The exclusion filter exists only in `run_focus_loop` at `:191`:
  ```rust
  if exclude_apps.iter().any(|app| current.app_name.contains(app)) { continue; }
  ```
- **Impact:** `1Password` and `Keychain Access` are in the default exclude list (`crates/rustwatch-core/src/config.rs:74-77`), and the README frames them as protected. In fact **every keystroke typed into those apps is captured, converted to `TextDelta` at `macos.rs:138-144`, and written to SQLite in plaintext.** The focus loop only suppresses `FocusChange` events for excluded apps, so the segments are not created for them — but the raw `events` rows containing the master passwords and secret keys are.
- **Fix:** Pass `exclude_apps` into `run_keyboard_loop` and check the current app context before emitting any event. This is the highest-severity finding in the codebase for a tool whose stated purpose includes capturing everything you type.

### Redaction protects exactly one of six egress paths

- **Files:** `crates/rustwatch-analyze/src/redact.rs:25-34` is invoked **only** from `crates/rustwatch-analyze/src/classifier.rs:27-35`, immediately before the LLM call.
- **Unredacted egresses:**
  1. SQLite — `insert_event` (`db.rs:36`) and `insert_segment` (`db.rs:49`) persist raw `text_buffer` and raw `Paste { content }`.
  2. Memory embeddings — `crates/rustwatch-memory/src/lib.rs:34-37` embeds untruncated `segment.text_buffer`.
  3. `rustwatch export` — `crates/rustwatch-cli/src/commands.rs:170-186` writes all segments as plaintext JSON, one file per run, never cleaned up.
  4. MCP `get_segment_context` — `crates/rustwatch-mcp/src/main.rs:100-107` returns full segment JSON to any connected MCP client (i.e. to the LLM agent).
  5. `DaemonCommand::Tail` — `crates/rustwatch-cli/src/commands.rs:131-139` prints raw event payloads to the terminal.
  6. TUI — `crates/rustwatch-cli/src/tui.rs:45-59` renders `label` and `apps` (both LLM-derived from raw text).
- **Impact:** `privacy.redact_patterns` (default `sk-[A-Za-z0-9]+`, `config.rs:99`) creates the impression of a privacy boundary that does not exist. Turning redaction off for one egress requires turning it off for all, and turning it on protects only the OpenAI/Anthropic call.
- **Fix:** Redact at **capture** time, before `insert_event`, so every downstream consumer inherits it. Add a documented "redaction happens at ingest" note.

### Everything is stored in plaintext with no retention policy

- **Risk:** `text_buffer` holds raw keystrokes and pasted clipboard contents. `Paste { content }` (`crates/rustwatch-core/src/events.rs:27-29`) holds whatever was on the clipboard, which routinely includes passwords and 2FA codes. Stored in `~/.rustwatch/rustwatch.db` with no encryption, no expiry, no size cap, and no `VACUUM`.
- **Aggravating:** The `events` table receives a row for **every keystroke** (`macos.rs:146-153`). There is no pruning query anywhere in the repo, and `Store::stats()` (`db.rs:227-241`) counts rows but never reports bytes on disk. A user who runs this for a year has no way to bound or even observe the growth.
- **Fix:** Encryption at rest (SQLCipher), an explicit retention window with a prune job, and a `--purge` command. At minimum, document the plaintext-at-rest posture prominently.

### HTML chart injection from LLM output

- **Files:** `crates/rustwatch-analyze/src/chart.rs:42-53`
  ```rust
  rows.push_str(&format!(
      "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}m</td></tr>",
      activity.started_at.format("%H:%M"), activity.label, activity.category, ...
  ```
- **Risk:** No HTML escaping on `label` or `category`. `label` is LLM output derived from captured window titles and keystrokes, so a window titled `<img src=x onerror=...>` propagates through classification into a stored activity and then into a `.html` file the user is told to open (`crates/rustwatch-cli/src/commands.rs:219-222`).
- **Fix:** Escape `& < > " '` in `render_html`.

### `GraphStore::slug` collapses distinct entities onto one node id

- **Files:** `crates/rustwatch-memory/src/graph.rs:112-123` maps every non-alphanumeric character to `_`, and the same `slug()` is used for both app names (`:49`) and topics (`:61`), both written to the single `graph_nodes` table with `INSERT OR REPLACE` (`:36`, `:51`, `:63`).
- **Impact:** An activity about the app `rust` and one about the topic `rust` write the **same node id**. The second write overwrites the first's `kind` and `label`, and both sets of `used_app`/`about_topic` edges then hang off a node whose kind is wrong. `"Google Chrome"` / `"Google-Chrome"` / `"GoogleChrome"` all collapse to distinct-but-adjacent ids (`google_chrome` vs `googlechrome`) — fine — but `"C++"` and `"C#"` both become `"c__"`.
- **Fix:** Prefix the namespace (`app:…`, `topic:…`) and add `kind` to the node primary key.

### Launchd logs land in a world-readable location

- **Files:** `deploy/macos/com.rustwatch.plist` writes stdout/stderr to `/tmp/rustwatchd.out.log` and `/tmp/rustwatchd.err.log`.
- **Risk:** `/tmp` is world-readable on macOS. The daemon logs `warn!(?err, "keyboard capture stopped")` and connection errors (`crates/rustwatch-daemon/src/main.rs:72`, `:85`, `:160`) — file paths and socket locations, not secrets, but it is a predictable on-disk artifact for a keystroke logger.
- **Fix:** Point `StandardOutPath`/`StandardErrorPath` inside the user's data dir with `0600`.

### Manual string escaping in a LanceDB filter expression

- **Files:** `crates/rustwatch-memory-backends/src/lance.rs:53-56`
  ```rust
  .delete(&format!("chunk_id = '{}'", escape(&chunk.chunk_id)))
  ```
  with `escape` doubling single quotes only (`:132-134`).
- **Risk:** In practice `chunk_id` is a `Uuid::new_v4()` string (`crates/rustwatch-memory/src/lib.rs:40`) so this is currently safe — but it is an injection-shaped API with a hand-rolled escaper. The whole crate is unbuilt, so nothing verifies this.
- **Fix:** Use LanceDB's bound-parameter API rather than string interpolation.

---

## Performance Bottlenecks

### Vector search is a full-table scan with in-Rust cosine

- **Files:** `crates/rustwatch-memory/src/sqlite_store.rs:86-115`
  ```rust
  let mut stmt = self.conn.prepare(
      "SELECT chunk_id, text, app_name, window_title, embedding FROM memory_chunks")?;
  ...
  let score = cosine(query_embedding, &emb);   // every row, in a loop
  scored.sort_by(...);
  scored.truncate(k);
  ```
- **Problem:** `SELECT` has **no `WHERE`, no `LIMIT`, no ANN index**. Every query reads every chunk (including the full `text` column and the 1536-byte embedding blob), deserializes all of them, computes cosine for each, sorts the entire result set, *then* truncates to `k`.
- **Impact:** Latency is O(total_chunks) on every single `rustwatch memory search`, every MCP `search_activity_memory`, and every TUI refresh. At 10k chunks (one day's work) this is already seconds; at 100k it is unusable. The `truncate(k)` at the end proves the author knew `k` was small.
- **Improvement path:** (a) select only `chunk_id` + `embedding` first, rank, then fetch text for the top `k`; (b) store embeddings as a fixed-size BLOB and use `sqlite-vec`/`vss` for ANN; (c) at minimum add `LIMIT` and drop `text` from the scan.

### Re-ingesting the same segments creates duplicate chunks forever

- **Files:** `crates/rustwatch-memory/src/lib.rs:39-49` generates `chunk_id: uuid::Uuid::new_v4()` on **every** call to `ingest_segments`. `SqliteMemoryStore::upsert` (`crates/rustwatch-memory/src/sqlite_store.rs:62-77`) keys on `chunk_id`, so `INSERT OR REPLACE` never dedupes anything.
- **Trigger:** `rustwatch memory ingest` (without `--rebuild`) ingests the last 7 days (`crates/rustwatch-cli/src/commands.rs:271-273`) and creates fresh ids for every one. `rustwatch analyze` also calls `ingest_activities` (`commands.rs:202`).
- **Impact:** Running ingest daily for a week gives ~7× the chunks for identical content. Search returns the same segment `k` times. This directly amplifies the full-scan problem above, and `memory.db` grows without bound. There is no `segment_id` uniqueness constraint and no `ON CONFLICT` target that would help.
- **Fix:** Make `chunk_id` deterministic — e.g. `format!("seg:{}", segment.id)` and `format!("act:{}", activity.id)` — so re-ingest is genuinely idempotent.

### The capture buffer does an O(16 KiB) memmove on every keystroke once full

- **Files:** `crates/rustwatch-core/src/segment.rs:106-114`
- **Problem:** Once `text_buffer` reaches `MAX_BUFFER_CHARS` (16,384), *every* subsequent character triggers `buffer.replace_range(..buffer.len() - keep, "")`, which shifts ~16 KiB.
- **Impact:** A permanent ~16 KB copy per keystroke on the daemon's writer task, on the hottest path in the system. A `VecDeque` with `pop_front`, or a two-segment rolling buffer, removes this entirely.
- **Note:** The eviction also keeps the **tail** and drops the **head** — the opposite of what activity classification wants, since the beginning of a session carries its context. There is no `truncated: bool` flag on `SessionSegment` (`crates/rustwatch-core/src/events.rs:70-80`), so a downstream consumer cannot tell the buffer was cut.

### Memory rebuild issues 365 sequential queries

- **Files:** `crates/rustwatch-memory/src/lib.rs:89-102` — a `for date_offset in 0..365` loop, each iteration a separate `list_activities_for_date` (`crates/rustwatch-core/src/db.rs:182-203`), each of which is a range query on the indexed `started_at`.
- **Impact:** 365 round trips on every `rustwatch memory ingest --rebuild`, with a spinner and no progress reporting. The segment query uses `now - 3650 days` (`:92`) but activities only reach back 365 days (`:96`) — **activity older than a year is silently unreachable by rebuild**, even though `list_activities_for_date` itself has no limit.
- **Fix:** One `SELECT … WHERE started_at BETWEEN ? AND ?` over the activities table, matching the range the segment query already uses.

### The TUI does 4 IPC round trips plus a full-day query every 250 ms

- **Files:** `crates/rustwatch-cli/src/tui.rs:27-43` — each loop iteration constructs a **new** `DaemonClient` (`:30`), opens a fresh `UnixStream`, sends `Status`, and separately runs `store.list_activities_for_date(today)` (`:28`).
- **Impact:** ~4 socket connections + a day-scoped table scan per 250 ms, i.e. ~16 connections/second sustained while the TUI is open. On top of this, the IPC calls have no timeout, so a daemon hiccup freezes the UI unkillably (see the timeout bug above).
- **Fix:** Poll at 1-2 s, reuse one connection, and only re-query activities when the segment count changes.

### No `busy_timeout` with four processes on one SQLite file

- **Files:** `crates/rustwatch-core/src/db.rs:21-22` sets `journal_mode = WAL` and nothing else. `rg 'pragma'` across the repo returns exactly this one line.
- **Impact:** Four processes open the same `rustwatch.db` — the daemon writer, the TUI, any CLI command, and the MCP server (`crates/rustwatch-mcp/src/main.rs:19`). WAL allows concurrent readers but only **one** writer, and the default `busy_timeout` is 0, so any contention returns `SQLITE_BUSY` **immediately** rather than waiting. The daemon hides this with `try_lock` + `let _ =` (`crates/rustwatch-daemon/src/main.rs:62-76`); the read paths propagate it straight to the user as a `rusqlite::Error` (`db.rs:154`, `:202`, `:224`). Running `rustwatch status` while the daemon is capturing is a realistic way to hit this.
- **Fix:** `conn.busy_timeout(Duration::from_secs(5))?` in `Store::open`, and the same in `SqliteMemoryStore::open` (`crates/rustwatch-memory/src/sqlite_store.rs:31`) and `GraphStore::open` (`crates/rustwatch-memory/src/graph.rs:14`) — neither of which sets WAL at all.

### `graph_edges` is unindexed, keyless, and append-only

- **Files:** `crates/rustwatch-memory/src/graph.rs:23-28` — no `PRIMARY KEY`, no `UNIQUE(from_id, to_id, rel)`, **no index on `from_id`**. `upsert_activity` writes with plain `INSERT` (`:55`, `:67`), never `OR REPLACE`.
- **Impact:** Re-ingesting the same activity appends duplicate edges forever. `expand_around_apps` (`:82-88`) does `WHERE e.from_id IN (SELECT id FROM graph_nodes WHERE json_extract(props_json,'$.chunk_id') = ?1)` — a `json_extract` over every node plus an unindexed edge scan, **per hit**. With 10 search hits that is 10 full scans.
- **Fix:** `UNIQUE(from_id, to_id, rel)`, an index on `from_id`, and a real `chunk_id` column instead of hiding it in `props_json`.

### `segment_ids_json LIKE '%' || s.id || '%'` cannot use an index

- **Files:** `crates/rustwatch-core/src/db.rs:157-165`. A leading-wildcard `LIKE` over a JSON text column, in the `LEFT JOIN` of the query that gates **all** LLM analysis.
- **Impact:** O(segments × activities) per `rustwatch analyze` run, on the database that is being written to continuously by the daemon. This is the query that decides what work remains, so it runs on every analyze invocation.
- **Fix:** Normalized join table (see the `LIKE` bug above).

---

## Fragile Areas

### `SegmentGrouper`'s byte-length heuristics

- **Files:** `crates/rustwatch-core/src/segment.rs:45-53`, `:106-118`
- **Why fragile:** Three different notions of "size" are mixed. `MAX_BUFFER_CHARS = 16_384` is compared against `String::len()`, which is **bytes**. `truncate()` at `:116-118` caps by **chars**, so a 16,384-char multi-byte string can be 4× the intended byte budget — the cap does not actually cap. The `TextFieldSnapshot` branch (`:47`) compares `active.text_buffer.len() < value.len()` to decide "is the snapshot bigger", which is a proxy for "is it newer" that is wrong whenever the user deletes text and retypes a shorter version.
- **Safe modification:** Do not touch these comparisons without first switching the whole module to a consistent unit (chars or bytes) and adding the tests listed in `docs/TESTING_PLAN.md` Phase 1. This is the highest-risk module in the repo and has zero tests.
- **Test coverage:** None.

### Timestamps stored as TEXT and compared lexicographically

- **Files:** `crates/rustwatch-core/src/db.rs:39`, `:110-112`, `:138-139`, `:186-187`; write side uses `to_rfc3339()` throughout.
- **Why fragile:** Every range query and every `ORDER BY` is a **string** comparison. This is correct *only* while every writer uses `DateTime<Utc>::to_rfc3339()`, which emits a fixed `+00:00` suffix and thus sorts correctly. It breaks silently the moment anything writes a local-offset timestamp (`2026-08-01T09:00:00+02:00`), a `Z`-suffixed variant, or a different fractional-second precision — the ordering quietly becomes wrong with no error. `idx_events_timestamp` and `idx_segments_started_at` (`crates/rustwatch-core/migrations/V1__initial.sql:6`, `:20`) index the text, so a plan change is needed, not just a data change.
- **Note:** `ended_at` is filtered (`db.rs:139`, `:139` `ended_at <= ?2`) with **no index** on it.
- **Safe modification:** Store epoch milliseconds as `INTEGER` before adding any writer. Do not add a new writer to the existing TEXT columns.
- **Test coverage:** None.

### `refinery` migrations run from four processes at open time

- **Files:** `crates/rustwatch-core/src/db.rs:23-25`, invoked from `Store::open` which is called by the daemon, the CLI (every subcommand), the TUI, and the MCP server.
- **Why fragile:** Migrations take a write lock. On first run after an upgrade, if the daemon is already up (which `deploy/macos/com.rustwatch.plist` guarantees via `KeepAlive`), every CLI invocation fails with `migration failed: …` (or `SQLITE_BUSY`, given the missing `busy_timeout`) until the daemon is restarted. The error is wrapped into `Error::Other(String)` (`db.rs:24`), losing the underlying `rusqlite::Error` type.
- **Safe modification:** Separate "migrate" from "open"; only the daemon migrates, or run migrations behind a file lock before opening.
- **Test coverage:** None.

### `rustwatch-mcp` opens two more SQLite files with no coordination

- **Files:** `crates/rustwatch-mcp/src/main.rs:19-20` — `Store::open(&paths.sqlite)` and `MemoryEngine::open(&paths, &config)` which opens `memory.db` and `memory-graph.db` (`crates/rustwatch-memory/src/lib.rs:24-25`).
- **Why fragile:** The MCP server is a long-lived process holding three connections open. Combined with the CLI opening the same files ad hoc and none of them setting `busy_timeout`, cross-process contention is guaranteed once an MCP client and the daemon are both live.
- **Safe modification:** Add `busy_timeout` before adding any fourth writer.

### `GraphRag::merge` score arithmetic

- **Files:** `crates/rustwatch-memory/src/rag.rs:6-31` — `existing.score = existing.score.max(score)` when a `chunk_id` repeats.
- **Why fragile:** The graph expansion path (`crates/rustwatch-memory/src/graph.rs:94-99`) synthesizes `ScoredChunk`s that **reuse the parent's `chunk_id`**, so dedup-by-`chunk_id` in `merge` collapses them. Whether a graph edge result survives into the final ranking depends on the exact interleaving of vector hits and graph hits and on the flat `+0.2` keyword boost. The `max` (rather than sum or count) means multiple edges from one chunk contribute nothing beyond the best one. Small scoring changes here silently change result ordering with no test to catch it.
- **Safe modification:** Change scores and the dedup key together, and pin the current behavior with the tests in `docs/TESTING_PLAN.md` Phase 1.
- **Test coverage:** None.

### `cosine` silently scores a truncated prefix

- **Files:** `crates/rustwatch-memory/src/sqlite_store.rs:131-149` — `let n = a.len().min(b.len());` then normalizes using only `a[..n]` and `b[..n]`.
- **Why fragile:** If a stored embedding ever has a different dimensionality from the query (e.g. a user enables the `fastembed` feature after having built `memory.db` with the 384-dim hash embedder, and the real model's output is not 384), the score is computed over a prefix of both vectors and is **numerically meaningless with no error**. `MemoryEngine::open` (`crates/rustwatch-memory/src/lib.rs:26`) constructs the embedder with no check against what is already on disk.
- **Safe modification:** Return `Err` on dimension mismatch, and record the embedding model name + dims alongside `memory_chunks`.
- **Test coverage:** None.

### `DefaultHasher` output is not stable across Rust versions

- **Files:** `crates/rustwatch-memory/src/embedder.rs:46-61` — `std::collections::hash_map::DefaultHasher`.
- **Why fragile:** `DefaultHasher::new()` uses a **fixed** key (not randomly seeded), so it *is* deterministic within a build — but the algorithm is explicitly not guaranteed stable across Rust releases. A toolchain upgrade silently invalidates every embedding in `memory.db`, at which point stored vectors and freshly-computed query vectors are drawn from different hash spaces and **search returns essentially arbitrary results with no error and no warning**. Nothing records which embedder or hash version produced a row.
- **Safe modification:** Use an explicitly-versioned hash (e.g. FNV-1a) or store an `embed_model` column and refuse to search a mismatched index.
- **Test coverage:** None.

---

## Scaling Limits

### SQLite event log — one row per keystroke, no ceiling

- **Current capacity:** Every `KeyDown`/`KeyRepeat` writes a row (`crates/rustwatch-capture/src/platform/macos.rs:146-153` → `crates/rustwatch-daemon/src/main.rs:63`). A fast typist at 80 wpm produces ~10k rows/hour; a full day is ~200k rows, each with a JSON `payload_json` and `app_json`.
- **Limit:** No pruning, no partitioning, no `VACUUM`, no size reporting. `Store::stats()` (`db.rs:227-241`) returns row counts only. After a few months the DB is multi-GB and `list_events_since(None, limit)` (`:101-130`) still returns the **oldest** events ascending — so `rustwatch tail` shows events from months ago, which looks like a broken daemon.
- **Scaling path:** Retention window + prune job; move raw events out of the hot DB or into a rolling time-partitioned table.

### Memory index — unbounded duplicate growth

- **Current capacity:** Unbounded, and grows *faster than real usage* because of the non-idempotent `chunk_id` (see the perf section above).
- **Limit:** Compounds the full-table-scan problem. Doubling chunk count doubles every search's latency, linearly, forever.
- **Scaling path:** Deterministic `chunk_id` (immediate ~N× reduction), then an ANN index.

### Screenshots — full window PNG per focus change, never pruned

- **Files:** `crates/rustwatch-capture/src/platform/macos.rs:205-217`; written to `~/.rustwatch/screenshots/YYYY-MM-DD/` by `macos.rs:302-305`.
- **Current capacity:** A full-resolution PNG (~1-5 MB) per application switch. 100 switches/day is ~300 MB/day.
- **Limit:** No retention, no compression, no downscaling, no disk-space guard. `screenshots` is counted in `stats()` (`db.rs:237-239`) but nothing manages it. This will fill a disk faster than anything else in the system.
- **Scaling path:** Downscale + JPEG, retention window, disk-usage ceiling that disables capture rather than failing.

### Single-file SQLite ceiling

- **Limit:** One `rustwatch.db` written by the daemon and read by three other processes, with no `busy_timeout` and no single-writer enforcement beyond an in-process `Mutex`. This does not survive the addition of any second writer.
- **Scaling path:** Writer-owner daemon with a read-only connection per consumer; move analytics off the hot file.

---

## Dependencies at Risk

### `keytap 0.4` — keyboard tap

- **File:** `crates/rustwatch-capture/Cargo.toml:20`; used at `crates/rustwatch-capture/src/platform/macos.rs:105-107`
- **Risk:** A `0.x` version (no semver stability guarantee) implementing a macOS event tap, which is inherently tied to TCC behavior and to `CGEventTap` placement. It requires Input Monitoring permission and breaks in non-obvious ways when permissions change mid-session — the daemon keeps running while silently capturing nothing (`macos.rs:71-74` only warns if the *iterator* errors, and permission denial surfaces as an `Err` from `Tap::new()` which is logged once and then the thread exits permanently).
- **Impact:** Losing capture silently on a macOS upgrade, with the daemon still reporting `running: true`.
- **Migration plan:** Treat `rustwatch-capture` behind the `PlatformCapture` trait as the replacement boundary — the abstraction at `crates/rustwatch-capture/src/platform/mod.rs:1-9` already exists for this. Add a heartbeat/event-count watchdog so "tap died" becomes visible as an error state instead of silence.

### `active-win-pos-rs 0.11` and `xcap 0.9` — window titles and screen capture

- **Files:** `crates/rustwatch-capture/Cargo.toml:19,21`; `macos.rs:232-240`, `:298-346`
- **Risk:** Both depend on Screen Recording TCC permission and on macOS window-server internals. `current_app_context()` (`:232-240`) returns `None` on any error (`:233` — `.ok()?`), and `run_focus_loop` (`:187-189`) treats `None` as "skip this tick" — so a TCC failure looks exactly like an idle screen. Combined with the broken `permissions` command, there is no way for a user to tell the difference.
- **Migration plan:** Same `PlatformCapture` boundary. Make the permission state observable rather than inferred from silence.

### `lancedb 0.17` + `arrow-array 53` + `surrealdb 2` — entirely unverified

- **Files:** `crates/rustwatch-memory-backends/Cargo.toml:10-11,17,22`
- **Risk:** None of these three appear in `Cargo.lock`; the crate has never been compiled. `arrow-array 53` is a hand-pinned major almost certainly incompatible with `lancedb 0.17`'s own arrow requirement, and the `RecordBatch`/`FixedSizeListArray` construction at `crates/rustwatch-memory-backends/src/lance.rs:110-130` uses arrow-53-era APIs. `surrealdb 2` is pinned with `default-features = false, features = ["kv-mem"]`, and the code at `surreal.rs:27-29` uses `.create(...).content(...)` with a typed `Option<surrealdb::sql::Thing>` return — a signature that has changed across surrealdb 2.x minors.
- **Impact:** 258 lines that have never seen a compiler. Treat as unverified, not as working.
- **Migration plan:** Fix the workspace membership first (see Tech Debt), then `cargo check -p rustwatch-memory-backends` and let the resolver pick arrow.

### `tokio` with `features = ["full"]` workspace-wide

- **File:** `Cargo.toml:34`
- **Risk:** Pulls in the filesystem, process, signal, and net drivers into every crate including leaf libraries like `rustwatch-core`, inflating binary size and build time for all three binaries.
- **Migration plan:** Narrow to the features each target needs.

### `reqwest` with `rustls-tls` and no certificate policy

- **File:** `Cargo.toml:31`
- **Risk:** Fine as configured (avoiding OpenSSL), but the API keys from `OPENAI_API_KEY`/`ANTHROPIC_API_KEY` (`crates/rustwatch-analyze/src/classifier.rs:76`, `:126`) travel over this connection with no request timeout configured (`reqwest::Client::new()` at `:79` and `:128` — default is **no** timeout). A hung OpenAI endpoint hangs `rustwatch analyze` indefinitely, with the progress spinner spinning (`crates/rustwatch-cli/src/commands.rs:189-198`) and no way out.
- **Migration plan:** `reqwest::Client::builder().timeout(Duration::from_secs(60)).build()`.

---

## Missing Critical Features

### No clean shutdown path for the daemon

- **Problem:** `crates/rustwatch-daemon/src/main.rs` has no signal handling. `SIGTERM` (sent by `commands::stop` at `crates/rustwatch-cli/src/commands.rs:69`) and `SIGINT` both kill the process with the default disposition.
- **Blocks:** (a) the active `SegmentGrouper` segment is never flushed — `flush()` at `main.rs:79` only runs when every channel sender is dropped, which never happens while running; (b) the pid file and socket are orphaned, deadlocking `rustwatch start`; (c) `rustwatch analyze` cannot run while the daemon holds the DB; (d) upgrades require a manual `rm -f` of the data dir.
- **Fix:** `tokio::signal::unix::{signal, SignalKind}` for SIGTERM/SIGINT → cancel the writer token, `grouper.flush()`, insert the segment, remove pid + socket, exit 0.

### Accessibility integration is a stub

- **Files:** `crates/rustwatch-capture/src/platform/macos.rs:247-250`
  ```rust
  fn read_focused_text_snapshot() -> Option<String> {
      // Best-effort placeholder: full AX integration can be expanded later.
      None
  }
  ```
- **Blocks:** The `TextFieldSnapshot` variant (`crates/rustwatch-core/src/events.rs:34-36`) is never produced, the `SegmentGrouper` branch that consumes it (`crates/rustwatch-core/src/segment.rs:45-53`) is dead, and the README's Accessibility permission grant ("focused text fields") does nothing. The capture path relies entirely on synthetic keystroke text, which is the source of the Shift/capslock corruption and the 16 KiB eviction problem.

### No scheduling — analysis is entirely manual

- **Files:** `config.analyze.batch_interval_minutes` (`crates/rustwatch-core/src/config.rs:36`) is declared and never read. `analyze_pending` (`crates/rustwatch-analyze/src/classifier.rs:21`) only runs when a human types `rustwatch analyze`.
- **Blocks:** The product cannot build a memory of a day without manual intervention every batch. Nothing ever calls `MemoryEngine::ingest_segments` from the daemon (`crates/rustwatch-memory/src/lib.rs:31` is invoked only from the CLI at `commands.rs:273` and the rebuild path), so the memory index is stale by default.

### No vision/LLM analysis despite the config promising it

- **Files:** `config.analyze.vision_model` (`config.rs:35`) and `config.privacy.send_screenshots_to_llm` (`config.rs:58`) are never read. `build_prompt` (`crates/rustwatch-analyze/src/classifier.rs:166-180`) sends only text; screenshots are captured, stored, and never analyzed.
- **Blocks:** Window screenshots — often the richest signal available — are dead weight on disk. Users setting `send_screenshots_to_llm = false` get a false guarantee, and users setting it `true` (the default) get nothing.

### No way to bound or observe disk usage

- **Problem:** Four unbounded stores (`events`, `screenshots/`, `memory.db` with duplicate chunks, `export-*.json` which are never cleaned up — `crates/rustwatch-cli/src/commands.rs:179-184`), no pruning, and `Store::stats()` (`db.rs:227-241`) reports rows but not bytes.
- **Blocks:** A user cannot answer "how much is this storing?" or "how do I delete it?" without reading source.

### No authentication, and no single-writer design

- **Problem:** Covered above (socket auth, `busy_timeout`, no transactions). The absence of any `BEGIN`/`COMMIT` anywhere in the repo (`rg 'transaction'` returns nothing) means `SqliteMemoryStore::upsert` (`crates/rustwatch-memory/src/sqlite_store.rs:62-82`) writes `memory_chunks` and `memory_fts` as two independent statements, and `GraphStore::upsert_activity` (`crates/rustwatch-memory/src/graph.rs:35-70`) writes N nodes and M edges unguarded. A crash mid-upsert leaves the vector table and the FTS table permanently out of sync, with no reconciliation path.

---

## Test Coverage Gaps

**There are zero tests.** `rg '#\[test\]|#\[cfg\(test\)]|mod tests'` over all `*.rs` returns nothing. No `tests/` directories, no `benches/`, no `.github/` CI, no `clippy.toml`, no `rustfmt.toml`, no `.config/nextest.toml`. `docs/TESTING_PLAN.md` is a complete four-layer plan that has not been started — and `tempfile`/`proptest`/`wiremock` (its proposed dev-deps) are absent from every `Cargo.toml`.

This is the root concern: the bugs above are individually small and individually fixable, but nothing prevents any of them from being reintroduced.

### rustwatch-core — **Priority: High**

- **What's not tested:** Everything. Most critical gaps:
  - `SegmentGrouper` (`crates/rustwatch-core/src/segment.rs`) — focus transitions, buffer eviction at `MAX_BUFFER_CHARS`, `TextFieldSnapshot` replacement rule, `flush()`. This module holds two live panics and the flush-data-loss bug.
  - `append_text` / `truncate` with multi-byte input (`segment.rs:106-118`) — the exact panic repro.
  - `Store` round-trips (`crates/rustwatch-core/src/db.rs`) against a real temp SQLite file: `insert_event` → `list_events_since`, `insert_segment` → `list_segments_between`, `insert_activity` → `list_activities_for_date`, `get_segment`.
  - `parse_ts` fallback behavior (`db.rs:252-256`) — pin whether "now" substitution is intended.
  - `list_unanalyzed_segments` (`db.rs:157-165`) — the `LIKE` false-positive case.
  - IPC frame encode/decode (`crates/rustwatch-core/src/ipc.rs:51-86`) including an oversized length prefix.
  - `expand_tilde` (`crates/rustwatch-core/src/paths.rs:58-72`) and `DataPaths` resolution — would have caught the `~/.rustwatch` vs ProjectDirs mismatch.
  - `Config` TOML round-trip, and behavior on a partial config missing a field.
- **Risk:** Any refactor of `db.rs` or `segment.rs` is unverifiable. The three panics would all be caught by a single test feeding `"é".repeat(5000)` through each.
- **Priority:** High

### rustwatch-analyze — **Priority: High**

- **What's not tested:** `Redactor::scrub` truncation (`crates/rustwatch-analyze/src/redact.rs:25-34`) with multi-byte input at exactly `max_chars` — the panic. `is_excluded_app` substring matching. `build_prompt` output. Both HTTP classifier bodies (`crates/rustwatch-analyze/src/classifier.rs:86-164`) — request shape, `response_format`, header values, response parsing, and the `.unwrap_or("{}")` fallbacks at `:112` and `:160` that silently swallow malformed LLM responses into an empty result. `analyze_pending`'s segment-to-activity mapping (`:48-64`).
- **Note:** `build_prompt` and the response-parsing are currently private free functions inline in the `classify` methods, so they are not reachable from a test without a refactor — `docs/TESTING_PLAN.md` Phase 1 calls this out.
- **Risk:** Prompt/response contract drift against the live OpenAI and Anthropic APIs is invisible until a user's analysis silently returns zero activities.
- **Priority:** High

### rustwatch-memory — **Priority: High**

- **What's not tested:** `bytes_to_f32` ↔ `Vec<f32>` round-trip and `cosine` on known vectors including the zero-vector and dimension-mismatch cases (`crates/rustwatch-memory/src/sqlite_store.rs:124-149`). `SqliteMemoryStore::search` ranking correctness. Idempotency of `ingest_segments` — a test ingesting the same segment twice and asserting one row would have caught the duplicate-chunk bug. `hash_embedding` determinism and 384-dim L2 normalization (`crates/rustwatch-memory/src/embedder.rs:46-61`), plus a test that pins `DefaultHasher` stability. `GraphRag::merge` keyword boost, dedup-by-`chunk_id`, and sort order (`crates/rustwatch-memory/src/rag.rs:6-31`). `slug()` collisions between apps and topics (`crates/rustwatch-memory/src/graph.rs:112-123`). `GraphStore::expand_around_apps` `hops` semantics (`:74-103`).
- **Risk:** The memory layer is the product's differentiator and has no verified behavior. Search quality cannot be assessed or preserved across changes.
- **Priority:** High

### rustwatch-capture — **Priority: Medium**

- **What's not tested:** The pure functions are testable and untested: `key_to_text`, `active_modifiers`, `scope_label`, `hash_content` (`crates/rustwatch-capture/src/platform/macos.rs:252-359`). The `meta_held` latch state machine (`:120-160`) — a test asserting `active_modifiers` after a Meta-down/other-key/no-Meta-up sequence would have caught it. `capture_to_disk`'s "reports success without writing" path (`:298-346`). The stub platform's `UnsupportedPlatform` errors (`crates/rustwatch-capture/src/platform/stub.rs`).
- **Risk:** Low testability for the real tap, but the pure functions are exactly where the bugs live, and they need no macOS hardware to test.
- **Priority:** Medium

### rustwatch-daemon — **Priority: Medium**

- **What's not tested:** `run_daemon` is a single 164-line `async fn` in `main.rs` with no library split, so **none of it is reachable from a test**. The writer task's `try_lock`-drop behavior, the signal/shutdown path that doesn't exist, and the IPC handler dispatch (`main.rs:102-157`) have no coverage.
- **Note:** `docs/TESTING_PLAN.md` Phase 4 already identifies the lib/bin split as a prerequisite.
- **Risk:** The daemon owns all the silent-failure paths. It is the least-testable crate and the one where failures are quietest.
- **Priority:** Medium

### rustwatch-cli / rustwatch-mcp — **Priority: Medium**

- **What's not tested:** The `&hit.text[..120]` panic (`crates/rustwatch-cli/src/commands.rs:241`) needs a single test with a multi-byte hit. `stop`'s pid parsing and `install`'s binary-path resolution (`commands.rs:23-27`, `:66-70`). The MCP line loop (`crates/rustwatch-mcp/src/main.rs:25-64`): malformed line recovery, unknown-method error shape, and the serial-await behavior. `tool()` schema construction (`:68-74`) declares `"required": []` for every tool, so `search_activity_memory` is advertised as taking no arguments — a client that trusts the schema will send no query and get a full-dump search.
- **Risk:** The MCP server is the integration surface for AI agents; a protocol violation there is invisible until a client refuses to connect.
- **Priority:** Medium

---

## Summary of Highest-Impact Findings

Ordered by (severity × likelihood of user-visible failure):

1. **Keystroke capture ignores the exclude list** (`macos.rs:71`, `:191`) — passwords typed into 1Password are captured and stored in plaintext. Security, high.
2. **Four panics on multi-byte input** — `commands.rs:241`, `redact.rs:31`, `segment.rs:110` — the `segment.rs` one kills the daemon writer silently. Correctness, high likelihood.
3. **Vector search is O(total_chunks) with a full-table scan** (`sqlite_store.rs:86-115`), compounded by non-idempotent ingest (`lib.rs:40`) producing duplicates on every run. Performance, degrades daily.
4. **Zero tests, zero CI** across all seven crates, with a written plan (`docs/TESTING_PLAN.md`) not started. This is what makes items 1-3 fixable-but-unfixable-in-perpetuity.
5. **Unauthenticated daemon socket with no frame-size cap** (`ipc.rs:63-78`) — any local user can dump all keystrokes or OOM the daemon with 4 bytes.
6. **No daemon shutdown path** — active session data lost on every exit, pid file orphaned, `rustwatch start` permanently wedged.
7. **`rustwatch-memory-backends` cannot be built** and has never been compiled — 258 lines of unverifiable code shipped behind a documented build command.

---

*Concerns audit: 2026-10-02*
