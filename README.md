


# whocall + whoimpl

[![CI](https://github.com/meloalright/who-ast/actions/workflows/ci.yml/badge.svg)](https://github.com/meloalright/who-ast/actions/workflows/ci.yml)

code analysis tools applicable to both human and AI agents.

<img height="307" alt="whocall" src="https://github.com/user-attachments/assets/ad1cb275-5a56-4ada-9f2a-8e169fbdd2f9" />

`whocall` answers questions like **"who calls this function?"**

<img height="307" alt="whoimpl" src="https://github.com/user-attachments/assets/86c5cf4f-ff76-46f4-9027-4a051fea59bf" />

`whoimpl` answers questions like **"who implements this trait?"**

## Install

* via homebrew

```sh
brew install meloalright/tap/whocall
```
```sh
brew install meloalright/tap/whoimpl
```

* via npm

```sh
npm install -g @whocall/cli
```
```sh
npm install -g @whoimpl/cli
```

## Usage

* whocall

```sh
$ whocall src/text/render.rs#render_text
Target:
  render_text
  src/text/render.rs:3:1
  pub fn render_text(ctx: &mut RenderCtx, text: &str)

Callers:
  src/main.rs:12:5	main
  src/main.rs:55:5	paint
  src/main.rs:57:5	paint
  src/main.rs:61:5	draw_editor
  src/main.rs:62:5	draw_editor
  src/main.rs:68:5	format_line
  src/text/highlight.rs:6:5	highlight_keyword
  src/text/highlight.rs:12:9	highlight_search
  src/text/render.rs:18:9	render_block
  src/text/unicode.rs:5:5	render_chinese
  src/text/unicode.rs:9:5	render_mixed
  src/traits.rs:48:5	render_all

12 callers found.
Confidence: medium 0.74
```

* whoimpl

```sh
$ whoimpl src/base_trait.rs#render
Trait method:
  crate::base_trait::Renderer::render
  src/base_trait.rs:4:5

Implementations:
  src/traits.rs:8:5	crate::traits::TextRenderer::render
  src/traits.rs:34:5	crate::traits::GpuRenderer::render
```

## Supported Languages

- [x] Rust
- [x] Python
- [ ] TypeScript/JavaScript
- [ ] Go
- [ ] Java
