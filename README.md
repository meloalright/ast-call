# whocall + whoimpl

Semantic code intelligence for humans and AI agents.

`whocall` answers questions like **"who calls this function?"**

`whoimpl` answers questions like **"who implements this trait?"**

## Install

via homebrew

```shell
brew install meloalright/tap/whocall
brew install meloalright/tap/whoimpl
```

via shell

```shell
curl -fsSL https://raw.githubusercontent.com/meloalright/who-ast/master/install-whocall.sh | sh
curl -fsSL https://raw.githubusercontent.com/meloalright/who-ast/master/install-whoimpl.sh | sh
```

via npm

```sh
npm install -g @whocall/cli
npm install -g @whoimpl/cli
```

## Usage

```
$ whocall src/render.rs#render_text

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

## Supported Languages

- [x] Rust
- [x] Python
- [ ] TypeScript/JavaScript
- [ ] Go
- [ ] Java


## License

MIT
