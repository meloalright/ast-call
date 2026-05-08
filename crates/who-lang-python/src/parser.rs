use anyhow::{Context, Result};
use tree_sitter::{Node, Parser, Tree};
use who_core::index::Index;
use who_core::lang::{LanguageParser, ParsedFile};
use who_core::refs::{RefKind, Reference};
use who_core::symbol::{Import, SourceRange, Symbol, SymbolKind, Visibility};

pub struct PythonParser {
    _private: (),
}

impl PythonParser {
    pub fn new() -> Self {
        Self { _private: () }
    }

    fn create_parser() -> Result<Parser> {
        let mut parser = Parser::new();
        let language = tree_sitter_python::LANGUAGE;
        parser
            .set_language(&language.into())
            .context("failed to set Python grammar")?;
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
                "function_definition" => {
                    if let Some(sym) = self.extract_function(child, source, file_id, module_path) {
                        symbols.push(sym);
                    }
                }
                "class_definition" => {
                    self.extract_class(child, source, file_id, module_path, symbols);
                }
                "decorated_definition" => {
                    if let Some(def) = child.child_by_field_name("definition") {
                        match def.kind() {
                            "function_definition" => {
                                if let Some(sym) =
                                    self.extract_function(def, source, file_id, module_path)
                                {
                                    symbols.push(sym);
                                }
                            }
                            "class_definition" => {
                                self.extract_class(def, source, file_id, module_path, symbols);
                            }
                            _ => {}
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
            name.clone()
        } else {
            format!("{module_path}.{name}")
        };

        let signature = extract_signature(node, source);

        Some(Symbol {
            id: 0,
            file_id,
            name,
            qualified_name,
            kind: SymbolKind::Function,
            range: node_range(node),
            signature: Some(signature),
            visibility: Visibility::Public,
        })
    }

    fn extract_class(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        module_path: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let class_name = match node.child_by_field_name("name") {
            Some(n) => node_text(n, source).to_string(),
            None => return,
        };

        let class_path = if module_path.is_empty() {
            class_name.clone()
        } else {
            format!("{module_path}.{class_name}")
        };

        let body = match node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        };

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            let def = match child.kind() {
                "function_definition" => child,
                "decorated_definition" => match child.child_by_field_name("definition") {
                    Some(d) if d.kind() == "function_definition" => d,
                    _ => continue,
                },
                _ => continue,
            };

            if let Some(name_node) = def.child_by_field_name("name") {
                let method_name = node_text(name_node, source).to_string();
                let qualified_name = format!("{class_path}.{method_name}");

                let signature = extract_signature(def, source);

                symbols.push(Symbol {
                    id: 0,
                    file_id,
                    name: method_name,
                    qualified_name,
                    kind: SymbolKind::Method,
                    range: node_range(def),
                    signature: Some(signature),
                    visibility: Visibility::Public,
                });
            }
        }
    }

    fn extract_imports(&self, node: Node, source: &[u8], file_id: i64, imports: &mut Vec<Import>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "import_statement" => {
                    self.extract_import_statement(child, source, file_id, imports);
                }
                "import_from_statement" => {
                    self.extract_import_from(child, source, file_id, imports);
                }
                _ => {
                    self.extract_imports(child, source, file_id, imports);
                }
            }
        }
    }

    fn extract_import_statement(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        imports: &mut Vec<Import>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "dotted_name" => {
                    let qualified = node_text(child, source).to_string();
                    let local_name = qualified
                        .rsplit('.')
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
                "aliased_import" => {
                    let name_node = child.child_by_field_name("name");
                    let alias_node = child.child_by_field_name("alias");
                    if let Some(name_n) = name_node {
                        let qualified = node_text(name_n, source).to_string();
                        let alias = alias_node.map(|a| node_text(a, source).to_string());
                        let local_name = alias.clone().unwrap_or_else(|| {
                            qualified
                                .rsplit('.')
                                .next()
                                .unwrap_or(&qualified)
                                .to_string()
                        });
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
                }
                _ => {}
            }
        }
    }

    fn extract_import_from(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        imports: &mut Vec<Import>,
    ) {
        let module = node
            .child_by_field_name("module_name")
            .map(|n| node_text(n, source).to_string())
            .unwrap_or_default();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "dotted_name" | "identifier"
                    if child.start_byte()
                        > node
                            .child_by_field_name("module_name")
                            .map(|n| n.end_byte())
                            .unwrap_or(0) =>
                {
                    let name = node_text(child, source).to_string();
                    let qualified = if module.is_empty() {
                        name.clone()
                    } else {
                        format!("{module}.{name}")
                    };
                    imports.push(Import {
                        id: 0,
                        file_id,
                        local_name: name,
                        qualified_target: qualified,
                        alias: None,
                        start_line: child.start_position().row as u32 + 1,
                        start_col: child.start_position().column as u32 + 1,
                    });
                }
                "aliased_import" => {
                    let name_node = child.child_by_field_name("name");
                    let alias_node = child.child_by_field_name("alias");
                    if let Some(name_n) = name_node {
                        let name = node_text(name_n, source).to_string();
                        let qualified = if module.is_empty() {
                            name.clone()
                        } else {
                            format!("{module}.{name}")
                        };
                        let alias = alias_node.map(|a| node_text(a, source).to_string());
                        let local_name = alias.clone().unwrap_or(name);
                        imports.push(Import {
                            id: 0,
                            file_id,
                            local_name,
                            qualified_target: qualified,
                            alias,
                            start_line: child.start_position().row as u32 + 1,
                            start_col: child.start_position().column as u32 + 1,
                        });
                    }
                }
                _ => {}
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
            if child.kind() == "call" {
                if let Some(callee) = child.child_by_field_name("function") {
                    let call_text = node_text(child, source).to_string();
                    let _callee_name = node_text(callee, source).to_string();
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
                }
            }
            self.extract_calls(child, source, file_id, symbols, refs);
        }
    }
}

impl Default for PythonParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for PythonParser {
    fn language_id(&self) -> &str {
        "python"
    }

    fn file_extensions(&self) -> &[&str] {
        &["py"]
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
        let refs = index.refs_to_symbol(0);
        let _ = (imports, symbols, refs);
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

fn derive_module_path(file_path: &str) -> String {
    let mut path = file_path;
    for prefix in &["src/", "lib/", "tests/"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            path = rest;
            break;
        }
    }
    let path = path.strip_suffix(".py").unwrap_or(path);
    if path == "__init__" || path == "__main__" || path == "main" {
        return String::new();
    }
    path.replace('/', ".")
}

fn extract_signature(node: Node, source: &[u8]) -> String {
    let start = node.start_byte();
    let text = &source[start..];
    if let Some(colon_pos) = text.iter().position(|&b| b == b':') {
        let sig = std::str::from_utf8(&text[..colon_pos]).unwrap_or("").trim();
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
    fn parse_simple_function() {
        let source = b"def hello():\n    pass\n";
        let tree = PythonParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = PythonParser::new();
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, "", &mut symbols);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "hello");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
    }

    #[test]
    fn parse_class_methods() {
        let source =
            b"class Foo:\n    def bar(self):\n        pass\n    def baz(self):\n        pass\n";
        let tree = PythonParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = PythonParser::new();
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, "", &mut symbols);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "bar");
        assert_eq!(symbols[0].qualified_name, "Foo.bar");
        assert_eq!(symbols[1].name, "baz");
    }

    #[test]
    fn parse_imports() {
        let source =
            b"import os\nfrom pathlib import Path\nfrom collections import OrderedDict as OD\n";
        let tree = PythonParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = PythonParser::new();
        let mut imports = Vec::new();
        parser.extract_imports(root, source, 1, &mut imports);
        assert!(imports.iter().any(|i| i.local_name == "os"));
        assert!(imports
            .iter()
            .any(|i| i.local_name == "Path" && i.qualified_target == "pathlib.Path"));
        assert!(imports
            .iter()
            .any(|i| i.local_name == "OD" && i.qualified_target == "collections.OrderedDict"));
    }

    #[test]
    fn parse_calls() {
        let source = b"def main():\n    print('hello')\n    os.path.join('a', 'b')\n";
        let tree = PythonParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = PythonParser::new();
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, "", &mut symbols);
        for s in &mut symbols {
            s.id = 1;
        }
        let mut refs = Vec::new();
        parser.extract_calls(root, source, 1, &symbols, &mut refs);
        assert!(refs.len() >= 2);
    }

    #[test]
    fn module_path_derivation() {
        assert_eq!(derive_module_path("src/utils/helpers.py"), "utils.helpers");
        assert_eq!(derive_module_path("src/__init__.py"), "");
        assert_eq!(derive_module_path("lib/core.py"), "core");
    }
}
