---
last_mapped_commit: c8ba2a9e65b063c9dbc60fc393d554bb99e9ca7a
last_mapped_at: 2026-10-02
---
# Technology Stack

**Analysis Date:** 2026-10-02

## Languages

**Primary:**

- Rust (edition 2021, workspace version `0.1.0`) — entire codebase, `crates/*/src/**/*.rs`, ~5,900 LOC across 8 packages

**Secondary:**

- SQL — one embedded migration, `crates/rustwatch-core/migrations/V1__initial.sql`, plus inline DDL in `crates/rustwatch-memory/src/sqlite_store.rs:32-52` and `crates/rustwatch-memory/src/graph.rs:17-28`
- HTML — one inline template string, `crates/rustwatch-analyze/src/chart.rs:56` (no templates crate, no asset pipeline)
- plist (XML) — launchd service definition, `deploy/macos/com.rustwatch.plist`

No build scripts (`build.rs`), no proc macros, no `unsafe` blocks in workspace crates (`libc::kill` in `crates/rustwatch-cli/src/commands.rs:69` is the only `unsafe`).

## Runtime

**Environment:**

- rustc 1.97.1 (`8bab26f4f 2026-07-14`), cargo 1.97.1 (`c980f4866 2026-06-30`) — verified on this machine
- No `rust-toolchain.toml` or `rust-toolchain` file exists — the toolchain is **unpinned**, so any stable Rust ≥ 1.70 compiles it. Add `rust-toolchain.toml` before relying on a specific compiler.
- macOS for capture; all other crates build and run anywhere `rusqlite` compiles (bundled C build requires a C compiler — Xcode CLT on macOS).

**Package Manager:**

- Cargo (workspace, `resolver = "2"`, declared in `Cargo.toml:2`)
- Lockfile: `Cargo.lock` present (162 KB) and committed
- 7 workspace members: `rustwatch-core`, `rustwatch-capture`, `rustwatch-daemon`, `rustwatch-cli`, `rustwatch-analyze`, `rustwatch-memory`, `rustwatch-mcp` (`Cargo.toml:3-11`)
- `crates/rustwatch-memory-backends` is **not** a workspace member and is **not** in `workspace.exclude`. `cargo metadata --manifest-path crates/rustwatch-memory-backends/Cargo.toml` fails with `current package believes it's in a workspace when it's not`. Its deps (`lancedb`, `surrealdb`, `arrow-array`, `arrow-schema`, `futures`) are absent from `Cargo.lock` and have never been resolved or built. The `--manifest-path` build command in `README.md` does not work as written.

## Frameworks

**Core:**

- tokio 1.53.1 (`features = ["full"]`) — async runtime for all three binaries; `#[tokio::main]` in `crates/rustwatch-cli/src/main.rs:74`, `crates/rustwatch-daemon/src/main.rs:18`, `crates/rustwatch-mcp/src/main.rs:9`; Unix socket server/client in `crates/rustwatch-core/src/ipc.rs`
- No web framework — no HTTP server, no Axum/Actix. The only HTTP client is `reqwest`.

**Testing:**

- None installed. No `[dev-dependencies]` in any `Cargo.toml`, no `tests/` directory, no `#[cfg(test)]` module anywhere in `crates/`.
- `docs/TESTING_PLAN.md` is an unimplemented plan: it specifies `proptest`, `tempfile`, `wiremock`, `assert_cmd`, `predicates`, `cargo-nextest` (`.config/nextest.toml`), and `.github/workflows/test.yml` — none of which exist yet.

**Build/Dev:**

- `refinery` 0.8.16 with the `rusqlite` feature — build-time embedded migrations via `embed_migrations!("migrations")` at `crates/rustwatch-core/src/db.rs:10`
- `cargo` only; no `just`, `make`, `nix`, or `xtask`. Build commands live in `README.md`.
- `[profile.release]` sets `lto = true`, `codegen-units = 1` (`Cargo.toml:57-59`) — expect slow release builds.

## Key Dependencies

**Critical:**

- `rusqlite` 0.32.1 with `features = ["bundled"]` — SQLite statically compiled in, so no system `libsqlite3` is required. Used by `rustwatch-core/src/db.rs`, `rustwatch-memory/src/sqlite_store.rs`, `rustwatch-memory/src/graph.rs`
- `reqwest` 0.12.28 with `features = ["json", "rustls-tls"], default-features = false` — pure-Rust TLS, no OpenSSL. Only consumer is `crates/rustwatch-analyze/src/classifier.rs`
- `serde` 1 + `serde_json` 1 — every crate's wire/storage format (`CaptureEventKind` is `#[serde(tag = "type")]`, `DaemonCommand` is `#[serde(tag = "cmd")]`)
- `chrono` 0.4 with `serde` — all timestamps are `DateTime<Utc>`, persisted as RFC 3339 strings (`db.rs:252-256`)

**macOS capture (target-gated, `[target.'cfg(target_os = "macos")'.dependencies]` in `crates/rustwatch-capture/Cargo.toml`):**

- `keytap` 0.4 — CGEventTap keyboard capture, requires Input Monitoring permission (`macos.rs:105-107`)
- `active-win-pos-rs` 0.11 — active window app name/title/PID (`macos.rs:229-238`)
- `xcap` 0.9 — window and monitor PNG capture, requires Screen Recording permission (`macos.rs:315-356`)
- `arboard` 3 — clipboard read via NSPasteboard (`macos.rs:241-244`)

**UI / terminal:**

- `clap` 4 with `derive` — subcommand CLI in `crates/rustwatch-cli/src/main.rs:10-72`
- `ratatui` 0.29 + `crossterm` 0.28 (0.29 also in the lock tree) — TUI dashboard, `crates/rustwatch-cli/src/tui.rs`
- `comfy-table` 7, `console` 0.15, `owo-colors` 4, `indicatif` 0.17 — table/color/spinner output
- `tracing` 0.1 + `tracing-subscriber` 0.3 (`env-filter`) + `tracing-indicatif` 0.3 — logging and progress

**Errors / config / misc:**

- `anyhow` 1 (binary + integration boundaries) and `thiserror` 2 (`crates/rustwatch-core/src/error.rs` defines the single `Error` enum)
- `toml` 0.8 — config file parsing; `serde` derive without `#[serde(default)]`, so a partial `config.toml` fails to deserialize
- `directories` 5 — `ProjectDirs::from("com", "chophe", "rustwatch")` for the data dir (`crates/rustwatch-core/src/paths.rs:52`)
- `uuid` 1 with `v4` + `serde` — event/segment/activity/chunk IDs
- `sha2` 0.10 — clipboard content hashing (`macos.rs:358-363`)
- `regex` 1 — redaction patterns (`crates/rustwatch-analyze/src/redact.rs`), declared only in `rustwatch-analyze`
- `libc` 0.2 — `SIGTERM` on `rustwatch stop` (`commands.rs:68-70`)
- `async-trait` 0.1 — `ActivityClassifier` trait (`classifier.rs:8-11`)

**Optional features (off by default):**

- `fastembed` 4.9.1 behind `rustwatch-memory`'s `fastembed` feature — real local embeddings via ONNX Runtime (`ort` 2.0.0-rc.9 is in `Cargo.lock`). Enables `BGE-small-en-v1.5` 384-dim vectors. Build: `cargo build --release -p rustwatch-memory --features fastembed`
- `rmcp` 0.3.2 (`server`, `transport-io`) behind `rustwatch-mcp`'s `rmcp` feature — **declared but never referenced**; there is no `cfg(feature = "rmcp")` in `crates/rustwatch-mcp/src/main.rs`, so the hand-rolled JSON-RPC loop is always what runs
- `lancedb` 0.17 + `arrow-array`/`arrow-schema` 53 + `surrealdb` 2 (`kv-mem`) + `futures` 0.3 in `crates/rustwatch-memory-backends` — written but unbuildable per the workspace issue above

## Configuration

**Environment:**

- File config only: `~/.rustwatch/config.toml`, written on first run by `load_or_create_config` (`crates/rustwatch-core/src/paths.rs:81-92`). Typed by `Config` in `crates/rustwatch-core/src/config.rs` with sections `data`, `capture`, `analyze`, `memory`, `ui`, `privacy`
- Environment variables: `OPENAI_API_KEY` (`classifier.rs:76`), `ANTHROPIC_API_KEY` (`classifier.rs:126`), `HOME` (`paths.rs:62`, `commands.rs:19`), `RUST_LOG` via `EnvFilter::from_default_env()` in all three binaries
- Config fields that are declared but never read at runtime: `analyze.vision_model`, `analyze.batch_interval_minutes`, `memory.vector_backend`, `memory.graph_backend`, `memory.surreal_engine`, `data.lance_path`, `data.surreal_path`, `ui.tui_enabled`, `ui.progress_bars`, `privacy.send_screenshots_to_llm`. Editing them has no runtime effect today.

**Build:**

- `Cargo.toml` (workspace deps + release profile), per-crate `Cargo.toml`, `Cargo.lock`
- No `.cargo/config.toml`, no `rustfmt.toml`, no `clippy.toml`, no lint groups configured. No formatter config means default `rustfmt`; note several lines exceed 100 cols (`crates/rustwatch-core/src/db.rs:50`, `crates/rustwatch-memory/src/rag.rs:4`), so the tree is not rustfmt-clean.
- No `.github/` directory — no CI workflow, no release automation.

## Platform Requirements

**Development:**

- Stable Rust toolchain (verified 1.97.1), a C compiler for the bundled SQLite build (Xcode CLT on macOS)
- macOS for any capture work; `crates/rustwatch-capture/src/platform/mod.rs:1-11` compiles a `stub.rs` elsewhere that returns `Error::UnsupportedPlatform` from every method

**Production:**

- macOS TCC permissions: Input Monitoring (keyboard), Accessibility (focused text), Screen Recording (window titles + screenshots). `PlatformCapture::permissions()` (`macos.rs:45-55`) is a hardcoded placeholder returning all-false, so `rustwatch permissions` cannot actually detect granted state.
- macOS 10.15+ in practice: SQLite ships on 10.15+, but earlier systems need the bundled build.
- Deployment: launchd agent. `rustwatch install` renders `deploy/macos/com.rustwatch.plist` into `~/Library/LaunchAgents/com.rustwatch.plist` with `{{RUSTWATCHD_PATH}}` substituted (`commands.rs:18-36`), `RunAtLoad` + `KeepAlive`. Logs land in `/tmp/rustwatchd.out.log` and `/tmp/rustwatchd.err.log`.
- Binaries must sit in the same `target/<profile>/` directory — `rustwatch start` resolves `rustwatchd` as a sibling of the current executable (`commands.rs:44-48`).
- Auxiliary repo artifact: `.crush/` (local Crush agent logs and `crush.db`) is untracked-by-convention tooling output, not part of the build. Not covered by `.gitignore`.

---

*Stack analysis: 2026-10-02*
