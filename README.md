# local-tools

Fast, single-purpose Rust binaries that automate **Alan R. Soares'** day-to-day
developer workflows on this machine (macOS Apple Silicon).

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

## Conventions

* **Edition 2021, `unsafe_code = "forbid"`, `clippy::all = warn`** — enforced
  workspace-wide via `[workspace.lints]`; every crate opts in with
  `[lints] workspace = true`.
* **`local-common` is dependency-free** (std only). It builds with no network,
  which keeps every tool's first `cargo build` fast and offline-friendly.
* **Per-tool homes are predictable**: `~/.config/local-tools/<tool>/` for
  config, `~/.local/share/local-tools/<tool>/` for data — XDG-style on purpose
  to match the rest of this machine's `~/.config/` dotfiles.
* **`Cargo.lock` is committed** — this is a binary-producing workspace.

## Adding a new tool

```bash
cargo new crates/<tool-name> --bin --vcs none \
     -d local-common --features-local-common  # pseudo; see below
cargo add local-common --features-local-common # pseudo
```

In practice:

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
   local-common = { path = "../local-common", version = "0.1.0" }
   [lints]
   workspace = true
   ```
3. Add `crates/<tool>` to the `members` array in the root `Cargo.toml`.
4. `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`

## Status

- ✅ Workspace scaffolding + CI-ready lint policy + `.gitignore`.
- ✅ `local-common` — per-tool path resolution (`tool_config_dir` /
  `tool_data_dir`) and terminal-aware colour helpers (`Colour`,
  `color_enabled_for`). Test-covered.
- ⏳ First tool — _pending your pick_ (see conversation / `TODO: first tool`).
