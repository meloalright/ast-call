use anyhow::{Context, Result};
use ast_call_core::index::Index;
use ast_call_core::lang::{LanguageParser, ParsedFile};
use ast_call_core::refs::{RefKind, Reference};
use ast_call_core::symbol::{Import, SourceRange, Symbol, SymbolKind, Visibility};
use tree_sitter::{Node, Parser, Tree};

pub struct RustParser {
    _private: (),
}

impl RustParser {
    pub fn new() -> Self {
        Self { _private: () }
    }

    fn create_parser() -> Result<Parser> {
        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE;
        parser
            .set_language(&language.into())
            .context("failed to set Rust grammar")?;
        Ok(parser)
    }

    fn parse_source(source: &[u8]) -> Result<Tree> {
        let mut parser = Self::create_parser()?;
        parser
            .parse(source, None)
            .context("tree-sitter parse returned None")
    }

    fn extract_symbols(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        module_path: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_item" => {
                    if let Some(sym) = self.extract_function(child, source, file_id, module_path) {
                        symbols.push(sym);
                    }
                }
                "impl_item" => {
                    self.extract_impl_methods(child, source, file_id, module_path, symbols);
                }
                "trait_item" => {
                    self.extract_trait_methods(child, source, file_id, module_path, symbols);
                }
                "mod_item" => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let mod_name = node_text(name_node, source);
                        let new_path = if module_path.is_empty() {
                            mod_name.to_string()
                        } else {
                            format!("{module_path}::{mod_name}")
                        };
                        if let Some(body) = child.child_by_field_name("body") {
                            self.extract_symbols(body, source, file_id, &new_path, symbols);
                        }
                    }
                }
                _ => {
                    self.extract_symbols(child, source, file_id, module_path, symbols);
                }
            }
        }
    }

    fn extract_function(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        module_path: &str,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source).to_string();
        let qualified_name = if module_path.is_empty() {
            format!("crate::{name}")
        } else {
            format!("crate::{module_path}::{name}")
        };

        let visibility = extract_visibility(node, source);
        let signature = extract_signature(node, source);

        Some(Symbol {
            id: 0,
            file_id,
            name,
            qualified_name,
            kind: SymbolKind::Function,
            range: node_range(node),
            signature: Some(signature),
            visibility,
        })
    }

    fn extract_impl_methods(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        module_path: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let type_name = node
            .child_by_field_name("type")
            .map(|n| node_text(n, source).to_string())
            .unwrap_or_default();

        let trait_name = node
            .child_by_field_name("trait")
            .map(|n| node_text(n, source).to_string());

        let body = match node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        };

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "function_item" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let method_name = node_text(name_node, source).to_string();
                    let qualified_name = if module_path.is_empty() {
                        format!("crate::{type_name}::{method_name}")
                    } else {
                        format!("crate::{module_path}::{type_name}::{method_name}")
                    };

                    let kind = if trait_name.is_some() {
                        SymbolKind::TraitMethod
                    } else {
                        SymbolKind::Method
                    };

                    let visibility = extract_visibility(child, source);
                    let signature = extract_signature(child, source);

                    symbols.push(Symbol {
                        id: 0,
                        file_id,
                        name: method_name,
                        qualified_name,
                        kind,
                        range: node_range(child),
                        signature: Some(signature),
                        visibility,
                    });
                }
            }
        }
    }

    fn extract_trait_methods(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        module_path: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let trait_name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source).to_string())
            .unwrap_or_default();

        let body = match node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        };

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "function_item" || child.kind() == "function_signature_item" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let method_name = node_text(name_node, source).to_string();
                    let qualified_name = if module_path.is_empty() {
                        format!("crate::{trait_name}::{method_name}")
                    } else {
                        format!("crate::{module_path}::{trait_name}::{method_name}")
                    };

                    let signature = extract_signature(child, source);

                    symbols.push(Symbol {
                        id: 0,
                        file_id,
                        name: method_name,
                        qualified_name,
                        kind: SymbolKind::TraitMethodDecl,
                        range: node_range(child),
                        signature: Some(signature),
                        visibility: Visibility::Public,
                    });
                }
            }
        }
    }

    fn extract_imports(&self, node: Node, source: &[u8], file_id: i64, imports: &mut Vec<Import>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "use_declaration" {
                self.extract_use(child, source, file_id, imports);
            } else {
                self.extract_imports(child, source, file_id, imports);
            }
        }
    }

    fn extract_use(&self, node: Node, source: &[u8], file_id: i64, imports: &mut Vec<Import>) {
        if let Some(arg) = node.child_by_field_name("argument") {
            self.extract_use_path(arg, source, file_id, "", imports);
        }
    }

    fn extract_use_path(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        path_prefix: &str,
        imports: &mut Vec<Import>,
    ) {
        match node.kind() {
            "use_as_clause" => {
                let path_node = node.child_by_field_name("path").unwrap_or(node);
                let alias_node = node.child_by_field_name("alias");
                let raw = node_text(path_node, source);
                let qualified = prepend_prefix(path_prefix, raw);
                let local_name = alias_node
                    .map(|n| node_text(n, source).to_string())
                    .unwrap_or_else(|| {
                        qualified
                            .rsplit("::")
                            .next()
                            .unwrap_or(&qualified)
                            .to_string()
                    });
                let alias = alias_node.map(|n| node_text(n, source).to_string());
                imports.push(Import {
                    id: 0,
                    file_id,
                    local_name,
                    qualified_target: qualified,
                    alias,
                    start_line: node.start_position().row as u32 + 1,
                    start_col: node.start_position().column as u32 + 1,
                });
            }
            "scoped_use_list" => {
                // Extract prefix path from the scoped_use_list's path child
                let prefix = node
                    .child_by_field_name("path")
                    .map(|p| {
                        let raw = node_text(p, source);
                        prepend_prefix(path_prefix, raw)
                    })
                    .unwrap_or_else(|| path_prefix.to_string());
                if let Some(list) = node.child_by_field_name("list") {
                    let mut cursor = list.walk();
                    for child in list.children(&mut cursor) {
                        self.extract_use_path(child, source, file_id, &prefix, imports);
                    }
                }
            }
            "use_list" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_use_path(child, source, file_id, path_prefix, imports);
                }
            }
            "scoped_identifier" | "identifier" => {
                let raw = node_text(node, source);
                let qualified = prepend_prefix(path_prefix, raw);
                let local_name = qualified
                    .rsplit("::")
                    .next()
                    .unwrap_or(&qualified)
                    .to_string();
                imports.push(Import {
                    id: 0,
                    file_id,
                    local_name,
                    qualified_target: qualified,
                    alias: None,
                    start_line: node.start_position().row as u32 + 1,
                    start_col: node.start_position().column as u32 + 1,
                });
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_use_path(child, source, file_id, path_prefix, imports);
                }
            }
        }
    }

    fn extract_calls(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        symbols: &[Symbol],
        refs: &mut Vec<Reference>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "call_expression" {
                if let Some(callee) = child.child_by_field_name("function") {
                    let call_text = node_text(child, source).to_string();
                    let callee_name = node_text(callee, source).to_string();
                    let enclosing = symbols
                        .iter()
                        .filter(|s| {
                            let line = child.start_position().row as u32 + 1;
                            s.contains_line(line)
                        })
                        .min_by_key(|s| s.range.end_line - s.range.start_line);

                    refs.push(Reference {
                        id: 0,
                        target_symbol_id: 0,
                        source_file_id: file_id,
                        source_symbol_id: enclosing.map(|s| s.id),
                        kind: RefKind::Call,
                        start_line: child.start_position().row as u32 + 1,
                        start_col: child.start_position().column as u32 + 1,
                        end_line: child.end_position().row as u32 + 1,
                        end_col: child.end_position().column as u32 + 1,
                        text: truncate_utf8(&call_text, 120),
                        confidence: 0.0,
                    });
                    let _ = callee_name; // used during resolution phase
                }
            }
            self.extract_calls(child, source, file_id, symbols, refs);
        }
    }
}

impl Default for RustParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for RustParser {
    fn language_id(&self) -> &str {
        "rust"
    }

    fn file_extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn parse_file(&self, index: &Index, file_id: i64, source: &[u8]) -> Result<ParsedFile> {
        let tree = Self::parse_source(source)?;
        let root = tree.root_node();

        let file_entry = index.find_file_by_id(file_id)?;
        let module_path = file_entry
            .as_ref()
            .map(|f| derive_module_path(&f.path))
            .unwrap_or_default();

        let mut symbols = Vec::new();
        self.extract_symbols(root, source, file_id, &module_path, &mut symbols);

        let mut stored_symbols = Vec::new();
        for mut sym in symbols {
            let id = index.insert_symbol(&sym)?;
            sym.id = id;
            stored_symbols.push(sym);
        }

        let mut imports = Vec::new();
        self.extract_imports(root, source, file_id, &mut imports);
        for imp in &imports {
            index.insert_import(imp)?;
        }

        let mut refs = Vec::new();
        self.extract_calls(root, source, file_id, &stored_symbols, &mut refs);
        for r in &refs {
            index.insert_ref(r)?;
        }

        Ok(ParsedFile {
            file_id,
            symbols_count: stored_symbols.len(),
            imports_count: imports.len(),
            calls_count: refs.len(),
        })
    }

    fn resolve_calls(&self, index: &Index, file_id: i64) -> Result<usize> {
        let imports = index.imports_in_file(file_id)?;
        let symbols = index.symbols_in_file(file_id)?;
        let refs = index.refs_to_symbol(0); // TODO: get refs by source file
        let _ = (imports, symbols, refs);

        // Phase 4 implementation: resolve call edges using imports
        // For now, return 0 resolved calls
        Ok(0)
    }
}

fn node_text<'a>(node: Node<'a>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn node_range(node: Node) -> SourceRange {
    SourceRange {
        start_line: node.start_position().row as u32 + 1,
        start_col: node.start_position().column as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        end_col: node.end_position().column as u32 + 1,
    }
}

fn extract_visibility(node: Node, source: &[u8]) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = node_text(child, source);
            return match text {
                "pub" => Visibility::Public,
                "pub(crate)" => Visibility::PubCrate,
                "pub(super)" => Visibility::PubSuper,
                _ if text.starts_with("pub") => Visibility::Public,
                _ => Visibility::Private,
            };
        }
    }
    Visibility::Private
}

fn prepend_prefix(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}::{name}")
    }
}

/// Derive the Rust module path from a file's relative path.
/// e.g. "src/text/render.rs" → "text::render"
///      "src/main.rs"        → ""
///      "src/text/mod.rs"    → "text"
fn derive_module_path(file_path: &str) -> String {
    let mut path = file_path;
    // Strip common source prefixes
    for prefix in &["src/", "lib/", "tests/"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            path = rest;
            break;
        }
    }
    // Strip .rs extension
    let path = path.strip_suffix(".rs").unwrap_or(path);
    // Strip /mod suffix
    let path = path.strip_suffix("/mod").unwrap_or(path);
    // Crate root files have no module path
    if path == "main" || path == "lib" || path == "mod" {
        return String::new();
    }
    path.replace('/', "::")
}

fn extract_signature(node: Node, source: &[u8]) -> String {
    let start = node.start_byte();
    let text = &source[start..];
    if let Some(body_pos) = text.iter().position(|&b| b == b'{') {
        let sig = std::str::from_utf8(&text[..body_pos]).unwrap_or("").trim();
        sig.to_string()
    } else if let Some(semi_pos) = text.iter().position(|&b| b == b';') {
        let sig = std::str::from_utf8(&text[..semi_pos]).unwrap_or("").trim();
        sig.to_string()
    } else {
        let end = node.end_byte().min(start + 200);
        std::str::from_utf8(&source[start..end])
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string()
    }
}

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.saturating_sub(3);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ascii_short() {
        assert_eq!(truncate_utf8("hello", 120), "hello");
    }

    #[test]
    fn truncate_ascii_long() {
        let long = "a".repeat(200);
        let result = truncate_utf8(&long, 120);
        assert!(result.len() <= 120);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_chinese_no_panic() {
        let chinese = "你好世界".repeat(30);
        let result = truncate_utf8(&chinese, 120);
        assert!(result.len() <= 120);
        assert!(result.ends_with("..."));
        // Must not split a 3-byte Chinese character
        assert!(result.is_char_boundary(result.len()));
    }

    #[test]
    fn truncate_mixed_scripts() {
        let mixed = "Hello 世界 こんにちは мир 🌍 ".repeat(10);
        let result = truncate_utf8(&mixed, 120);
        assert!(result.len() <= 120);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_emoji_boundary() {
        // 🌍 is 4 bytes — ensure we don't slice in the middle
        let s = "x".repeat(116) + "🌍🌍";
        let result = truncate_utf8(&s, 120);
        assert!(result.len() <= 120);
        assert!(result.ends_with("..."));
    }
}
