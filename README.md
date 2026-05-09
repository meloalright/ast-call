# who-ast

Semantic code intelligence for humans and AI agents.

`who-ast` parses source code via tree-sitter, builds a symbol/call index in SQLite, and answers questions like **"who calls this function?"** and **"who implements this trait?"**

## Install

```sh
# npm (recommended)
npm install -g @whocall/cli
npm install -g @whoimpl/cli

# Shell (macOS / Linux)
curl -fsSL https://raw.githubusercontent.com/meloalright/who-ast/master/install-whocall.sh | sh
curl -fsSL https://raw.githubusercontent.com/meloalright/who-ast/master/install-whoimpl.sh | sh

# Homebrew (macOS / Linux)
brew tap meloalright/tap
brew install whocall
brew install whoimpl

# From source
cargo install --path crates/who-cli
```

## Usage

```bash
# Build the index
whocall index .

# Who calls the function at this line?
whocall src/render.rs:3

# Who calls render_text in this file?
whocall src/render.rs#render_text

# JSON output for AI agents
whocall src/render.rs:3 --json

# Who implements the trait method at this line?
whoimpl src/base_trait.rs:4

# Who implements render in this file?
whoimpl src/base_trait.rs#render
```

Two target formats: `file:line` and `file#symbol`.

## Supported Languages

| Language | Crate | Grammar |
|----------|-------|---------|
| Rust | `who-lang-rust` | tree-sitter-rust |
| Python | `who-lang-python` | tree-sitter-python |

## How It Works

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

Call resolution uses three strategies with confidence scoring:

| Strategy | Confidence | When |
|----------|-----------|------|
| Import match | 0.75 | Callee matches an import that resolves to one symbol |
| Same-file match | 0.60 | Callee matches a symbol in the same file |
| Global unique | 0.45 | Callee matches exactly one symbol in the index |

## Example Output

```
$ whocall src/render.rs:3

Target:
  render_text
  src/render.rs:3:1
  pub fn render_text(ctx: &mut RenderCtx, text: &str)

Callers:
  src/main.rs:10:5       main
  src/main.rs:53:5       paint
  src/main.rs:55:5       paint
  src/main.rs:59:5       draw_editor
  src/main.rs:60:5       draw_editor
  src/main.rs:66:5       format_line
  src/traits.rs:48:5     render_all

7 callers found.
Confidence: high 0.75
```

## Architecture

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for workspace layout, data model, and design principles.

## License

MIT
