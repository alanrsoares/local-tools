# local-tools

Fast, single-purpose Rust binaries that automate **Alan's** day-to-day
developer workflows. Primary target is Alan's machine (macOS Apple Silicon);
the workspace is std-only and builds/tests clean on Linux too, and CI runs
both.

The repo is a **Cargo workspace**: a thin, shared `local-common` crate plus one
crate per tool. Tools are intentionally small, dependency-light, and replace
ad-hoc shell functions with something versioned, testable, and shareable.

## Layout

```
local-tools/
├── Cargo.toml              # workspace manifest (members, lint policy, shared deps)
├── crates/
│    ├── local-common/       # shared plumbing (paths, terminal helpers, …)
│    └── <tool>/             # one crate per tool, added as we build them
└── README.md
```

## Included Tools

| Tool | Purpose | Key Commands |
| :--- | :--- | :--- |
| **`scaffold`** | Instant offline project scaffolder (TS/Bun/Biome, Rust, Go, Python/uv) | `scaffold ts -n my-app`<br>`scaffold rust`<br>`scaffold --list` |
| **`portkill`** | Sub-millisecond port inspector and process killer | `portkill`<br>`portkill 3000 8080`<br>`portkill -f node` |
| **`jwt`** | Zero-dependency JWT inspector, claim extractor & claim humanizer | `jwt <TOKEN>`<br>`pbpaste \| jwt`<br>`jwt -c exp,sub <TOKEN>` |
| **`devclean`** | Multi-ecosystem build artifact scanner & disk space reclaimer | `devclean`<br>`devclean ~/dev --clean`<br>`devclean -t rust,node` |
| **`fanout`** | Concurrent quality gate & task matrix runner with topological DAG, Git change detection & live TUI | `fanout`<br>`fanout lint typecheck test --bail`<br>`fanout --since main`<br>`fanout --filter '@renkonos/*' --topological` |
| **`webdriver`** | Zero-dependency browser automation, persistent sessions & screenshot engine. Terse one-line-per-step output (`ok` / `warn` / `err`), `text=` and `role=` locators, buffered console errors, streaming `--repl` | `webdriver --session my-app http://localhost:3000 wait-for-hydration click 'role=button:Save' console`<br>`webdriver https://app.dev viewport 1440 900 screenshot out.png --full-page`<br>`webdriver --repl < script.wd`<br>`webdriver --list-sessions` |

## Conventions

* **Edition 2021, `unsafe_code = "forbid"`, `clippy::all = warn`** — enforced
  workspace-wide via `[workspace.lints]`; every crate opts in with
  `[lints] workspace = true`.
* **Zero external dependencies** (std only across crates). Builds and tests fully offline
  with instant compilation speed.
* **Per-tool homes are predictable**: `~/.config/local-tools/<tool>/` for
  config, `~/.local/share/local-tools/<tool>/` for data — XDG-style on purpose
  to match the rest of this machine's `~/.config/` dotfiles.
* **`Cargo.lock` is committed** — this is a binary-producing workspace.

## Adding a new tool

1. `mkdir crates/<tool> && cd crates/<tool>`
2. Write a `Cargo.toml` inheriting common fields:
   ```toml
   [package]
   name = "<tool>"
   version.workspace = true
   edition.workspace = true
   authors.workspace = true
   license.workspace = true

   [[bin]]
   name = "<tool>"
   path = "src/main.rs"

   [dependencies]
   local-common = { path = "../local-common" }

   [lints]
   workspace = true
   ```
3. Add `"crates/<tool>"` to the `members` array in the root `Cargo.toml`.
4. `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`

## Status

- ✅ Workspace scaffolding + CI-ready lint policy + `.gitignore`.
- ✅ `local-common` — per-tool path resolution and terminal-aware colour helpers. Test-covered.
- ✅ `scaffold` — offline project generator (TypeScript, Rust, Go, Python). Test-covered.
- ✅ `portkill` — fast port inspector & process killer. Test-covered.
- ✅ `jwt` — zero-dependency JWT decoder & timestamp humanizer. Test-covered.
- ✅ `devclean` — multi-ecosystem project artifact scanner & reclaimer. Test-covered.
- ✅ `fanout` — concurrent quality gate & task matrix runner with interactive TUI. Test-covered.
- ✅ `webdriver` — zero-dependency browser automation, DSL runner & screenshot engine. Test-covered.
