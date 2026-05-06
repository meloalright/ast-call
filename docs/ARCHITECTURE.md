# ast-call Architecture

## Overview

ast-call is a location-first semantic caller analysis CLI. It parses source code into ASTs via tree-sitter, builds a symbol/call index in SQLite, and answers queries like "who calls the function at this line?"

```
Source Files
     ↓
  tree-sitter AST Parse
     ↓
  Symbol + Import + Call Extraction
     ↓
  SQLite Index  (.ast-call/index.sqlite)
     ↓
  Target Resolution + Query
     ↓
  Output (human / JSON / NDJSON / quickfix)
```

---

## Workspace Layout

```
ast-call/
├── Cargo.toml                          # workspace root
├── crates/
│   ├── ast-call-core/                  # data model, index, resolution engine
│   │   └── src/
│   │       ├── target.rs               # CLI target parser (file:line, file#symbol, etc.)
│   │       ├── symbol.rs               # Symbol, Import, FileEntry, SourceRange
│   │       ├── refs.rs                 # Reference, RefKind
│   │       ├── calls.rs               # CallEdge, Resolution
│   │       ├── confidence.rs           # scoring builder + labels
│   │       ├── resolve.rs              # target → symbol resolution, caller lookup
│   │       ├── index.rs               # SQLite schema, reads, writes
│   │       ├── lang.rs                # LanguageParser trait, detect_language()
│   │       └── error.rs              # AstCallError, ExitCode
│   │
│   ├── ast-call-cli/                   # binary crate
│   │   └── src/
│   │       ├── main.rs                 # clap CLI, command dispatch
│   │       ├── cmd_index.rs            # `ast-call index .`
│   │       ├── cmd_callers.rs          # `ast-call [callers] <target>`
│   │       ├── cmd_def.rs              # `ast-call def <target>`
│   │       ├── cmd_refs.rs             # `ast-call refs <target>`
│   │       ├── cmd_impl.rs             # `ast-call impl <target>`
│   │       ├── cmd_impact.rs           # `ast-call impact <target>`
│   │       └── output.rs              # human, JSON, quickfix formatters
│   │
│   └── ast-call-lang-rust/             # Rust language support
│       └── src/
│           ├── lib.rs
│           └── parser.rs              # tree-sitter Rust extraction
│
├── samples/
│   └── rust-project/                   # sample codebase for demos
│
└── .github/workflows/
    ├── ci.yml                          # build, test, clippy, fmt
    └── showcase.yml                    # index + query the sample project
```

### Crate Dependency Graph

```
ast-call-cli
├── ast-call-core
│   ├── rusqlite          (SQLite storage)
│   ├── serde / serde_json (serialization)
│   ├── ignore            (gitignore-aware file walking)
│   ├── thiserror         (error types)
│   └── anyhow            (error propagation)
├── ast-call-lang-rust
│   ├── ast-call-core
│   ├── tree-sitter       (AST parsing framework)
│   └── tree-sitter-rust  (Rust grammar)
└── clap                  (CLI argument parsing)
```

---

## Core Data Model

Five entities stored in SQLite, mirroring the spec's semantic layers:

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

### Entity Summary

| Entity   | What it stores                            | Key fields                                      |
|----------|-------------------------------------------|-------------------------------------------------|
| `files`  | Each indexed source file                  | `path`, `language`, `hash` (for incremental)    |
| `symbols`| Functions, methods, trait decls            | `qualified_name`, `kind`, `range`, `signature`  |
| `imports`| `use` statements and aliases              | `local_name`, `qualified_target`, `alias`       |
| `refs`   | All reference sites (calls, imports, etc.) | `target_symbol_id`, `kind`, `text`, `confidence`|
| `calls`  | Directed caller→callee edges              | `caller_symbol_id`, `callee_symbol_id`, `resolution` |

### SQLite Indexes

```
idx_symbols_file_id          symbols(file_id)
idx_symbols_name             symbols(name)
idx_symbols_qualified_name   symbols(qualified_name)
idx_imports_file_id          imports(file_id)
idx_refs_target_symbol_id    refs(target_symbol_id)
idx_refs_source_file_id      refs(source_file_id)
idx_calls_caller_symbol_id   calls(caller_symbol_id)
idx_calls_callee_symbol_id   calls(callee_symbol_id)
```

---

## Data Flow

### Indexing Pipeline (`ast-call index .`)

```
 Walk files (ignore::WalkBuilder, respects .gitignore)
     │
     ▼
 For each .rs file:
     │
     ├─ Check hash → skip if unchanged (incremental)
     │
     ├─ Delete old data for file (if re-indexing)
     │
     ├─ Insert into `files` table
     │
     └─ parser.parse_file(index, file_id, source)
            │
            ├─ 1. tree-sitter parse → AST
            │
            ├─ 2. extract_symbols()
            │     Walk AST for function_item, impl_item, trait_item, mod_item
            │     Build qualified names: crate::module::Type::method
            │     → INSERT INTO symbols
            │
            ├─ 3. extract_imports()
            │     Walk AST for use_declaration
            │     Handle scoped lists, aliases, path prefixes
            │     → INSERT INTO imports
            │
            └─ 4. extract_calls()
                  Walk AST for call_expression nodes
                  Find enclosing symbol for each call site
                  → INSERT INTO refs (target_symbol_id = 0, unresolved)

 All within a single SQLite transaction.
 Write metadata.json with file/symbol/import/call counts.
```

### Query Pipeline (`ast-call <target>`)

```
 Parse target string
     │  "src/render.rs:42"  →  Target::FileLine { file, line }
     │  "src/app.rs:88:12"  →  Target::FileLineColumn { file, line, col }
     │  "src/render.rs#fn"  →  Target::FileSymbol { file, symbol }
     │  "crate::mod::fn"    →  Target::QualifiedSymbol { path }
     │  "render_text"       →  Target::PlainSymbol { name }
     │
     ▼
 Find .ast-call/index.sqlite (walk up from cwd)
     │
     ▼
 resolve_target(index, target) → ResolvedTarget { symbol, file_path }
     │
     │  FileLine:         find file → get symbols in file → smallest enclosing
     │  FileSymbol:       find file → filter symbols by name
     │  QualifiedSymbol:  query symbols by qualified_name
     │  PlainSymbol:      query symbols by name (ambiguous if > 1)
     │
     ▼
 Command-specific query:
     │
     ├─ callers:  find_callers(symbol.id)
     │            → SELECT from calls WHERE callee_symbol_id = ?
     │
     ├─ def:      return resolved symbol location
     │
     ├─ refs:     refs_to_symbol(symbol.id)
     │            → SELECT from refs WHERE target_symbol_id = ?
     │
     ├─ impl:     find_symbols_by_name(symbol.name)
     │            → filter by kind = TraitMethod | Method, exclude self
     │
     └─ impact:   BFS caller chain to --depth N
                  → repeated find_callers() with visited set
     │
     ▼
 Format output (output.rs)
     ├─ Human:    tabular, grouped by section
     ├─ JSON:     structured with target, callers[], summary
     ├─ NDJSON:   one JSON object per line
     └─ Quickfix: file:line:col: description (Vim-compatible)
```

---

## Tree-Sitter Parsing (Rust)

The Rust parser (`ast-call-lang-rust/src/parser.rs`) traverses the tree-sitter AST in a single pass, extracting three things:

### Symbol Extraction

| AST Node Kind             | Symbol Kind        | Qualified Name Pattern                       |
|---------------------------|--------------------|----------------------------------------------|
| `function_item`           | `Function`         | `crate::[mod_path::]name`                    |
| `function_item` in `impl` | `Method`          | `crate::[mod_path::]Type::name`              |
| `function_item` in trait `impl` | `TraitMethod` | `crate::[mod_path::]Type::name`            |
| `function_signature_item` in `trait` | `TraitMethodDecl` | `crate::[mod_path::]Trait::name`   |

Module nesting is tracked by recursing into `mod_item` bodies, accumulating the module path.

### Import Extraction

Handles all Rust `use` forms:

```rust
use crate::text::render_text;              // simple
use crate::text::render_text as rt;        // alias
use crate::text::{render_text, layout};    // scoped list
use crate::text::*;                        // glob (stored as-is)
```

Extracted via recursive walk of `use_declaration` → `scoped_use_list` → `use_as_clause` / `scoped_identifier` nodes.

### Call Site Extraction

Every `call_expression` node produces a `Reference` with:
- `kind = RefKind::Call`
- `text` = the call expression text (truncated to 120 chars)
- `source_symbol_id` = enclosing function (smallest symbol containing the call's line)
- `target_symbol_id = 0` (unresolved — Phase 4)

---

## Confidence Scoring

Each reference and call edge carries a confidence score (0.0–1.0) computed by additive/subtractive signals:

```
 Signal                        Score
 ─────────────────────────────────────
 Exact source location         +0.40
 Exact qualified symbol match  +0.35
 Import resolves to target     +0.25
 Receiver type known           +0.25
 Same module                   +0.10
 Exact local name match        +0.10
 Multiple candidates           −0.20
 Dynamic dispatch              −0.25
 Macro-generated code          −0.30
 Dynamic import/eval           −0.40

 Label thresholds:
   0.85–1.00  high
   0.60–0.84  medium
   0.30–0.59  low
   0.00–0.29  unresolved
```

---

## Error Handling & Exit Codes

| Code | Meaning               | When                                         |
|-----:|-----------------------|----------------------------------------------|
|    0 | Success               | Query returned results                       |
|    1 | No match              | Target resolved but no callers/refs found    |
|    2 | Ambiguous target      | Plain symbol matches multiple definitions    |
|    3 | Parse error           | Invalid target string                        |
|    4 | Index missing         | No `.ast-call/index.sqlite` found            |
|    5 | Unsupported language  | File extension not recognized                |
|    6 | Target outside symbol | Line doesn't fall within any function        |
|   10 | Internal error        | I/O, SQLite, or unexpected failure           |

---

## Implementation Status

### Implemented (Phases 1–3, 5–6 partial)

- Target parsing: all 5 formats
- Rust symbol extraction: functions, methods, trait methods, trait decls
- Import extraction: simple, aliased, scoped lists
- Call site discovery: all `call_expression` nodes
- SQLite index: schema, CRUD, incremental hash-based re-indexing
- All 6 CLI commands: `index`, `callers`, `def`, `refs`, `impl`, `impact`
- Output: human, JSON, NDJSON, quickfix
- Confidence model: builder + labels
- Exit codes

### Not Yet Implemented

| Area                         | Status | Notes                                          |
|------------------------------|--------|-------------------------------------------------|
| Import-aware call resolution | Stub   | Phase 4: connect calls to symbols via `use` paths |
| Explain mode (`--why`)       | Stub   | Phase 6: populate `why` vectors in JSON output  |
| Watch mode (`--watch`)       | Parsed | Flag accepted but no file watcher               |
| TypeScript parser            | —      | Language detected, no parser crate               |
| Python parser                | —      | Language detected, no parser crate               |
| Go parser                    | —      | Language detected, no parser crate               |
| Lua parser                   | —      | Language detected, no parser crate               |
| Type-aware resolution        | —      | Phase: Level 3 heuristic method resolution      |
| LSP-assisted mode            | —      | Phase: Level 4 optional integration             |
| Mermaid graph output         | —      | `--graph mermaid` not yet wired                  |

---

## Key Design Decisions

**Location over name.** The primary input is `file:line`, not a symbol name. This eliminates ambiguity — a source location identifies exactly one function in one file.

**SQLite as the index.** A single `.ast-call/index.sqlite` file is the entire index. No daemon, no server. Queries are fast (WAL mode), incremental updates are simple (hash comparison), and the tool composes well with other CLI tools.

**Unresolved-first indexing.** Call sites are stored immediately with `target_symbol_id = 0`. Resolution is a separate pass. This keeps indexing fast and lets the tool return partial results even before resolution is complete.

**LanguageParser trait.** Each language is a separate crate implementing `LanguageParser`. The core library has no tree-sitter dependency — only language crates do. Adding a new language means adding a new crate without touching core.

**Confidence as a first-class concept.** Every result carries a confidence score. AI coding agents can filter by confidence threshold. Humans see a label (high/medium/low). The scoring model is explicit and tunable.
