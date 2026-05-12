# whocall — Architecture

## Overview

`whocall` is a semantic code intelligence tool for humans and AI agents. It parses source code via tree-sitter, builds a symbol/call index in SQLite, and answers the question "who calls this function?"

```
Source Files
     |
  tree-sitter AST Parse
     |
  Symbol + Import + Call Extraction
     |
  SQLite Index  (.who-ast/index.sqlite)
     |
  Target Resolution + Query
     |
  Output (human / JSON)
```

---

## CLI

```
whocall     Find callers of a symbol — and build the index
```

### Target Formats

Two ways to specify a target:

```
file:line          src/render.rs:42
file#symbol        src/render.rs#render_text
```

### Usage

```bash
whocall index .                          # build the index
whocall src/render.rs:3                  # who calls the function at this line?
whocall src/render.rs:3 --json           # structured output for AI agents
whocall src/render.rs#render_text        # who calls render_text in this file?
```

---

## Workspace Layout

```
who-ast/
├── Cargo.toml                          # workspace root
├── crates/
│   ├── who-core/                       # data model, index, resolution engine
│   │   └── src/
│   │       ├── target.rs               # CLI target parser (file:line, file#symbol, etc.)
│   │       ├── symbol.rs               # Symbol, Import, FileEntry, SourceRange
│   │       ├── refs.rs                 # Reference, RefKind
│   │       ├── calls.rs                # CallEdge, Resolution
│   │       ├── confidence.rs           # internal scoring builder + labels
│   │       ├── resolve.rs              # target → symbol resolution, caller lookup
│   │       ├── index.rs                # SQLite schema, reads, writes
│   │       ├── lang.rs                 # LanguageParser trait, detect_language()
│   │       └── error.rs               # WhoError, ExitCode
│   │
│   ├── who-cli/                        # binary crate (whocall)
│   │   └── src/
│   │       ├── bin_whocall.rs          # `whocall` — caller queries + index
│   │       ├── cmd_index.rs            # index subcommand
│   │       ├── cmd_callers.rs          # caller resolution
│   │       └── output.rs              # human + JSON formatters
│   │
│   ├── who-lang-rust/                  # Rust language support
│   │   └── src/
│   │       ├── lib.rs
│   │       └── parser.rs              # tree-sitter Rust extraction
│   │
│   ├── who-lang-python/                # Python language support
│   │   └── src/
│   │       ├── lib.rs
│   │       └── parser.rs              # tree-sitter Python extraction
│   │
│   └── who-lang-go/                    # Go language support
│       └── src/
│           ├── lib.rs
│           └── parser.rs              # tree-sitter Go extraction
│
├── samples/
│   ├── rust-project/                   # Rust sample codebase for demos
│   ├── python-project/                 # Python sample codebase for demos
│   └── go-project/                     # Go sample codebase for demos
│
├── npm/
│   └── whocall-cli/                    # @whocall/cli npm package
│       ├── package.json
│       └── install.js                 # postinstall binary downloader
│
└── .github/workflows/
    ├── ci.yml                          # build, test, clippy, fmt
    ├── showcase-rust.yml               # Rust sample demos + edge cases
    ├── showcase-python.yml             # Python sample demos + edge cases
    ├── showcase-go.yml                 # Go sample demos + edge cases
    ├── release.yml                     # build binaries + update Homebrew tap
    └── npm.yml                         # publish @whocall/cli to npm
```

### Crate Dependency Graph

```
who-cli
├── who-core
│   ├── rusqlite          (SQLite storage)
│   ├── serde / serde_json (serialization)
│   ├── ignore            (gitignore-aware file walking)
│   ├── thiserror         (error types)
│   └── anyhow            (error propagation)
├── who-lang-rust
│   ├── who-core
│   ├── tree-sitter       (AST parsing framework)
│   └── tree-sitter-rust  (Rust grammar)
├── who-lang-python
│   ├── who-core
│   ├── tree-sitter       (AST parsing framework)
│   └── tree-sitter-python (Python grammar)
├── who-lang-go
│   ├── who-core
│   ├── tree-sitter       (AST parsing framework)
│   └── tree-sitter-go    (Go grammar)
└── clap                  (CLI argument parsing)
```

---

## Core Data Model

Five entities stored in SQLite:

```
┌──────────┐     ┌──────────┐     ┌──────────┐
│  files   │◄────│ symbols  │◄────│  imports │
│          │     │          │     │          │
│ path     │     │ name     │     │ local_   │
│ language │     │ qual_name│     │   name   │
│ hash     │     │ kind     │     │ qual_    │
│ mtime    │     │ range    │     │  target  │
└──────────┘     │ signature│     │ alias    │
                 │ visibility     └──────────┘
                 └─────┬────┘
                       │
              ┌────────┴────────┐
              ▼                 ▼
        ┌──────────┐     ┌──────────┐
        │   refs   │◄────│  calls   │
        │          │     │          │
        │ target_  │     │ caller_  │
        │  sym_id  │     │  sym_id  │
        │ source_  │     │ callee_  │
        │  file_id │     │  sym_id  │
        │ kind     │     │ ref_id   │
        │ text     │     │ confid.  │
        │ confid.  │     │ resolut. │
        └──────────┘     └──────────┘
```

---

## Call Resolution

After indexing, `resolve_all_calls()` runs a second pass over every file's unresolved call refs. Three strategies are tried in order:

```
 Strategy              Confidence   When
 ──────────────────────────────────────────────────────────
 1. Import match       0.75         callee name matches an import's local_name,
                                    and the import's qualified_target resolves to
                                    exactly one symbol
 2. Same-file match    0.60         callee name matches a symbol defined in the
                                    same file
 3. Global unique      0.45         callee name matches exactly one symbol across
                                    the entire index
```

If global lookup finds multiple candidates, an `Ambiguous` call edge is stored with all candidate IDs and confidence 0.25.

---

## Release & Installation

### Release Pipeline (`.github/workflows/release.yml`)

Triggered on GitHub release publish:

```
 release published
     │
     ├─ Build 4 targets in parallel:
     │   ├─ aarch64-apple-darwin   (macOS ARM, native)
     │   ├─ x86_64-apple-darwin    (macOS Intel, native)
     │   ├─ x86_64-unknown-linux-gnu (Linux x86_64, native)
     │   └─ aarch64-unknown-linux-gnu (Linux ARM, cross)
     │
     ├─ Package whocall as who-<target>.tar.gz
     │
     ├─ Upload to GitHub release assets
     │
     └─ Homebrew job: generate whocall.rb → push to meloalright/homebrew-tap
```

### npm Publish Pipeline (`.github/workflows/npm.yml`)

Triggered on the same release event, after release assets are available:

```
 release published
     │
     ├─ Wait for release assets (≥4 binaries uploaded)
     │
     ├─ Update package.json version to match release tag
     │
     └─ Publish @whocall/cli
```

The npm package is a thin wrapper — no native code bundled. On `npm install`, a `postinstall` script downloads the correct prebuilt binary from GitHub releases.

### Installation

```sh
# npm (recommended)
npm install -g @whocall/cli

# Homebrew (macOS / Linux)
brew tap meloalright/tap
brew install whocall

# From source
cargo install --path crates/who-cli
```

---

## Design Principles

1. **Semantic-first** — resolve to meaning, not syntax shape
2. **Incremental by default** — hash-based re-indexing avoids reparsing unchanged files
3. **AI-agent native** — structured JSON output for easy integration
4. **Multi-language architecture** — pluggable `LanguageParser` trait per language
5. **Unix-like UX** — minimal, composable, scriptable CLI commands
