use anyhow::{Context, Result};
use tree_sitter::{Node, Parser, Tree};
use who_core::index::Index;
use who_core::lang::{LanguageParser, ParsedFile};
use who_core::refs::{RefKind, Reference};
use who_core::symbol::{Import, SourceRange, Symbol, SymbolKind, Visibility};

pub struct GoParser {
    _private: (),
}

impl GoParser {
    pub fn new() -> Self {
        Self { _private: () }
    }

    fn create_parser() -> Result<Parser> {
        let mut parser = Parser::new();
        let language = tree_sitter_go::LANGUAGE;
        parser
            .set_language(&language.into())
            .context("failed to set Go grammar")?;
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
        pkg: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_declaration" => {
                    if let Some(sym) = self.extract_function(child, source, file_id, pkg) {
                        symbols.push(sym);
                    }
                }
                "method_declaration" => {
                    if let Some(sym) = self.extract_method(child, source, file_id, pkg) {
                        symbols.push(sym);
                    }
                }
                "type_declaration" => {
                    self.extract_interface_methods(child, source, file_id, pkg, symbols);
                    self.extract_symbols(child, source, file_id, pkg, symbols);
                }
                _ => {
                    self.extract_symbols(child, source, file_id, pkg, symbols);
                }
            }
        }
    }

    fn extract_function(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        pkg: &str,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source).to_string();
        let qualified_name = if pkg.is_empty() {
            name.clone()
        } else {
            format!("{pkg}.{name}")
        };

        let visibility = if name.starts_with(|c: char| c.is_uppercase()) {
            Visibility::Public
        } else {
            Visibility::Private
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
            visibility,
        })
    }

    fn extract_method(&self, node: Node, source: &[u8], file_id: i64, pkg: &str) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source).to_string();

        let receiver_type = node
            .child_by_field_name("receiver")
            .and_then(|r| extract_receiver_type(r, source))
            .unwrap_or_default();

        let qualified_name = if pkg.is_empty() {
            if receiver_type.is_empty() {
                name.clone()
            } else {
                format!("{receiver_type}.{name}")
            }
        } else if receiver_type.is_empty() {
            format!("{pkg}.{name}")
        } else {
            format!("{pkg}.{receiver_type}.{name}")
        };

        let visibility = if name.starts_with(|c: char| c.is_uppercase()) {
            Visibility::Public
        } else {
            Visibility::Private
        };

        let signature = extract_signature(node, source);

        Some(Symbol {
            id: 0,
            file_id,
            name,
            qualified_name,
            kind: SymbolKind::Method,
            range: node_range(node),
            signature: Some(signature),
            visibility,
        })
    }

    fn extract_interface_methods(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        pkg: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "type_spec" {
                continue;
            }
            let iface_name = match child.child_by_field_name("name") {
                Some(n) => node_text(n, source).to_string(),
                None => continue,
            };
            let type_node = match child.child_by_field_name("type") {
                Some(n) if n.kind() == "interface_type" => n,
                _ => continue,
            };
            let mut inner = type_node.walk();
            for member in type_node.children(&mut inner) {
                if member.kind() != "method_elem" {
                    continue;
                }
                let mut mc = member.walk();
                for mchild in member.children(&mut mc) {
                    if mchild.kind() == "field_identifier" {
                        let method_name = node_text(mchild, source).to_string();
                        let qualified_name = if pkg.is_empty() {
                            format!("{iface_name}.{method_name}")
                        } else {
                            format!("{pkg}.{iface_name}.{method_name}")
                        };
                        let sig_text = node_text(member, source).to_string();
                        symbols.push(Symbol {
                            id: 0,
                            file_id,
                            name: method_name,
                            qualified_name,
                            kind: SymbolKind::TraitMethodDecl,
                            range: node_range(member),
                            signature: Some(sig_text),
                            visibility: Visibility::Public,
                        });
                    }
                }
            }
        }
    }

    fn extract_imports(&self, node: Node, source: &[u8], file_id: i64, imports: &mut Vec<Import>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "import_declaration" => {
                    self.extract_import_decl(child, source, file_id, imports);
                }
                _ => {
                    self.extract_imports(child, source, file_id, imports);
                }
            }
        }
    }

    fn extract_import_decl(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        imports: &mut Vec<Import>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "import_spec" => {
                    self.extract_import_spec(child, source, file_id, imports);
                }
                "import_spec_list" => {
                    let mut inner = child.walk();
                    for spec in child.children(&mut inner) {
                        if spec.kind() == "import_spec" {
                            self.extract_import_spec(spec, source, file_id, imports);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_import_spec(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        imports: &mut Vec<Import>,
    ) {
        let path_node = match node.child_by_field_name("path") {
            Some(n) => n,
            None => return,
        };
        let raw_path = node_text(path_node, source);
        let import_path = raw_path.trim_matches('"').to_string();

        let alias_node = node.child_by_field_name("name");
        let alias = alias_node.map(|n| node_text(n, source).to_string());

        let local_name = alias.clone().unwrap_or_else(|| {
            import_path
                .rsplit('/')
                .next()
                .unwrap_or(&import_path)
                .to_string()
        });

        if local_name == "." || local_name == "_" {
            return;
        }

        imports.push(Import {
            id: 0,
            file_id,
            local_name,
            qualified_target: import_path,
            alias,
            start_line: node.start_position().row as u32 + 1,
            start_col: node.start_position().column as u32 + 1,
        });
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

impl Default for GoParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for GoParser {
    fn language_id(&self) -> &str {
        "go"
    }

    fn file_extensions(&self) -> &[&str] {
        &["go"]
    }

    fn parse_file(&self, index: &Index, file_id: i64, source: &[u8]) -> Result<ParsedFile> {
        let tree = Self::parse_source(source)?;
        let root = tree.root_node();

        let pkg = extract_package_name(root, source);

        let mut symbols = Vec::new();
        self.extract_symbols(root, source, file_id, &pkg, &mut symbols);

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

fn extract_package_name(root: Node, source: &[u8]) -> String {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "package_clause" {
            let mut inner = child.walk();
            for gc in child.children(&mut inner) {
                if gc.kind() == "package_identifier" {
                    return node_text(gc, source).to_string();
                }
            }
        }
    }
    String::new()
}

fn extract_receiver_type(receiver: Node, source: &[u8]) -> Option<String> {
    let mut cursor = receiver.walk();
    for child in receiver.children(&mut cursor) {
        if child.kind() == "parameter_declaration" {
            if let Some(type_node) = child.child_by_field_name("type") {
                let text = node_text(type_node, source);
                let name = text.trim_start_matches('*');
                return Some(name.to_string());
            }
        }
    }
    None
}

fn extract_signature(node: Node, source: &[u8]) -> String {
    let start = node.start_byte();
    let text = &source[start..];
    if let Some(brace_pos) = text.iter().position(|&b| b == b'{') {
        let sig = std::str::from_utf8(&text[..brace_pos]).unwrap_or("").trim();
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
        let source = b"package main\n\nfunc hello() {\n}\n";
        let tree = GoParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = GoParser::new();
        let pkg = extract_package_name(root, source);
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &mut symbols);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "hello");
        assert_eq!(symbols[0].qualified_name, "main.hello");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
    }

    #[test]
    fn parse_method() {
        let source = b"package main\n\nfunc (s *Server) Start() {\n}\n";
        let tree = GoParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = GoParser::new();
        let pkg = extract_package_name(root, source);
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &mut symbols);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Start");
        assert_eq!(symbols[0].qualified_name, "main.Server.Start");
        assert_eq!(symbols[0].kind, SymbolKind::Method);
    }

    #[test]
    fn parse_exported_visibility() {
        let source = b"package pkg\n\nfunc Exported() {\n}\n\nfunc unexported() {\n}\n";
        let tree = GoParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = GoParser::new();
        let pkg = extract_package_name(root, source);
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &mut symbols);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].visibility, Visibility::Public);
        assert_eq!(symbols[1].visibility, Visibility::Private);
    }

    #[test]
    fn parse_imports() {
        let source = b"package main\n\nimport (\n\t\"fmt\"\n\t\"net/http\"\n)\n";
        let tree = GoParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = GoParser::new();
        let mut imports = Vec::new();
        parser.extract_imports(root, source, 1, &mut imports);
        assert!(imports.iter().any(|i| i.local_name == "fmt"));
        assert!(imports
            .iter()
            .any(|i| i.local_name == "http" && i.qualified_target == "net/http"));
    }

    #[test]
    fn parse_aliased_import() {
        let source = b"package main\n\nimport (\n\tpb \"google/protobuf\"\n)\n";
        let tree = GoParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = GoParser::new();
        let mut imports = Vec::new();
        parser.extract_imports(root, source, 1, &mut imports);
        assert!(imports
            .iter()
            .any(|i| i.local_name == "pb" && i.qualified_target == "google/protobuf"));
    }

    #[test]
    fn parse_calls() {
        let source =
            b"package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"hello\")\n}\n";
        let tree = GoParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = GoParser::new();
        let pkg = extract_package_name(root, source);
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &mut symbols);
        for s in &mut symbols {
            s.id = 1;
        }
        let mut refs = Vec::new();
        parser.extract_calls(root, source, 1, &symbols, &mut refs);
        assert!(!refs.is_empty());
    }

    #[test]
    fn parse_interface_methods() {
        let source = b"package pkg\n\ntype Speaker interface {\n\tSpeak() string\n}\n\nfunc (d Dog) Speak() string {\n\treturn \"Woof\"\n}\n\nfunc (c Cat) Speak() string {\n\treturn \"Meow\"\n}\n";
        let tree = GoParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = GoParser::new();
        let pkg = extract_package_name(root, source);
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &mut symbols);
        let decl = symbols
            .iter()
            .find(|s| s.kind == SymbolKind::TraitMethodDecl);
        assert!(decl.is_some());
        assert_eq!(decl.unwrap().qualified_name, "pkg.Speaker.Speak");
        let methods: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
        assert_eq!(methods.len(), 2);
    }
}
