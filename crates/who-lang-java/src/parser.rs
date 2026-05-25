use anyhow::{Context, Result};
use tree_sitter::{Node, Parser, Tree};
use who_core::index::Index;
use who_core::lang::{LanguageParser, ParsedFile};
use who_core::refs::{RefKind, Reference};
use who_core::symbol::{Import, SourceRange, Symbol, SymbolKind, Visibility};

pub struct JavaParser {
    _private: (),
}

impl JavaParser {
    pub fn new() -> Self {
        Self { _private: () }
    }

    fn create_parser() -> Result<Parser> {
        let mut parser = Parser::new();
        let language = tree_sitter_java::LANGUAGE;
        parser
            .set_language(&language.into())
            .context("failed to set Java grammar")?;
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
        package: &str,
        class_stack: &[String],
        symbols: &mut Vec<Symbol>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "class_declaration" | "enum_declaration" => {
                    self.extract_class(child, source, file_id, package, class_stack, symbols);
                }
                "interface_declaration" => {
                    self.extract_interface(child, source, file_id, package, class_stack, symbols);
                }
                _ => {
                    self.extract_symbols(child, source, file_id, package, class_stack, symbols);
                }
            }
        }
    }

    fn extract_class(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        package: &str,
        class_stack: &[String],
        symbols: &mut Vec<Symbol>,
    ) {
        let class_name = match node.child_by_field_name("name") {
            Some(n) => node_text(n, source).to_string(),
            None => return,
        };

        let mut new_stack = class_stack.to_vec();
        new_stack.push(class_name);

        let body = match node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        };

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "method_declaration" => {
                    if let Some(sym) =
                        self.extract_method(child, source, file_id, package, &new_stack)
                    {
                        symbols.push(sym);
                    }
                }
                "constructor_declaration" => {
                    if let Some(sym) =
                        self.extract_constructor(child, source, file_id, package, &new_stack)
                    {
                        symbols.push(sym);
                    }
                }
                "class_declaration" | "enum_declaration" => {
                    self.extract_class(child, source, file_id, package, &new_stack, symbols);
                }
                "interface_declaration" => {
                    self.extract_interface(child, source, file_id, package, &new_stack, symbols);
                }
                _ => {}
            }
        }
    }

    fn extract_interface(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        package: &str,
        class_stack: &[String],
        symbols: &mut Vec<Symbol>,
    ) {
        let iface_name = match node.child_by_field_name("name") {
            Some(n) => node_text(n, source).to_string(),
            None => return,
        };

        let mut new_stack = class_stack.to_vec();
        new_stack.push(iface_name);

        let body = match node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        };

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "method_declaration" => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let method_name = node_text(name_node, source).to_string();
                        let qualified_name =
                            build_qualified_name(package, &new_stack, &method_name);
                        let visibility = extract_visibility(child, source);
                        let signature = extract_signature(child, source);

                        let has_body = child.child_by_field_name("body").is_some();
                        let kind = if has_body {
                            SymbolKind::Method
                        } else {
                            SymbolKind::TraitMethodDecl
                        };

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
                "class_declaration" | "enum_declaration" => {
                    self.extract_class(child, source, file_id, package, &new_stack, symbols);
                }
                "interface_declaration" => {
                    self.extract_interface(child, source, file_id, package, &new_stack, symbols);
                }
                _ => {}
            }
        }
    }

    fn extract_method(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        package: &str,
        class_stack: &[String],
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source).to_string();
        let qualified_name = build_qualified_name(package, class_stack, &name);
        let visibility = extract_visibility(node, source);
        let signature = extract_signature(node, source);

        let is_static = has_modifier(node, source, "static");
        let kind = if is_static {
            SymbolKind::ClassMethod
        } else {
            SymbolKind::Method
        };

        Some(Symbol {
            id: 0,
            file_id,
            name,
            qualified_name,
            kind,
            range: node_range(node),
            signature: Some(signature),
            visibility,
        })
    }

    fn extract_constructor(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        package: &str,
        class_stack: &[String],
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source).to_string();
        let qualified_name = build_qualified_name(package, class_stack, &name);
        let visibility = extract_visibility(node, source);
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

    fn extract_imports(&self, node: Node, source: &[u8], file_id: i64, imports: &mut Vec<Import>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "import_declaration" {
                self.extract_import(child, source, file_id, imports);
            }
        }
    }

    fn extract_import(&self, node: Node, source: &[u8], file_id: i64, imports: &mut Vec<Import>) {
        let text = node_text(node, source).to_string();
        let path = text
            .trim_start_matches("import")
            .trim_start_matches("static")
            .trim()
            .trim_end_matches(';')
            .trim()
            .to_string();

        if path.ends_with(".*") {
            return;
        }

        let local_name = path.rsplit('.').next().unwrap_or(&path).to_string();

        imports.push(Import {
            id: 0,
            file_id,
            local_name,
            qualified_target: path,
            alias: None,
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
            match child.kind() {
                "method_invocation" => {
                    let call_text = node_text(child, source).to_string();
                    let enclosing = find_enclosing_symbol(child, symbols);

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
                "object_creation_expression" => {
                    let call_text = node_text(child, source).to_string();
                    let enclosing = find_enclosing_symbol(child, symbols);

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
                _ => {}
            }
            self.extract_calls(child, source, file_id, symbols, refs);
        }
    }
}

impl Default for JavaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for JavaParser {
    fn language_id(&self) -> &str {
        "java"
    }

    fn file_extensions(&self) -> &[&str] {
        &["java"]
    }

    fn parse_file(&self, index: &Index, file_id: i64, source: &[u8]) -> Result<ParsedFile> {
        let tree = Self::parse_source(source)?;
        let root = tree.root_node();

        let package = extract_package_name(root, source);

        let mut symbols = Vec::new();
        self.extract_symbols(root, source, file_id, &package, &[], &mut symbols);

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
        if child.kind() == "package_declaration" {
            let mut inner = child.walk();
            for gc in child.children(&mut inner) {
                if gc.kind() == "scoped_identifier" || gc.kind() == "identifier" {
                    return node_text(gc, source).to_string();
                }
            }
        }
    }
    String::new()
}

fn build_qualified_name(package: &str, class_stack: &[String], method_name: &str) -> String {
    let mut parts = Vec::new();
    if !package.is_empty() {
        parts.push(package.to_string());
    }
    for class in class_stack {
        parts.push(class.clone());
    }
    parts.push(method_name.to_string());
    parts.join(".")
}

fn extract_visibility(node: Node, source: &[u8]) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "modifiers" {
            let text = node_text(child, source);
            if text.contains("public") {
                return Visibility::Public;
            } else if text.contains("protected") {
                return Visibility::PubCrate;
            } else if text.contains("private") {
                return Visibility::Private;
            }
        }
    }
    Visibility::Private
}

fn has_modifier(node: Node, source: &[u8], modifier: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "modifiers" {
            let text = node_text(child, source);
            return text.contains(modifier);
        }
    }
    false
}

fn extract_signature(node: Node, source: &[u8]) -> String {
    let start = node.start_byte();
    let text = &source[start..];
    if let Some(brace_pos) = text.iter().position(|&b| b == b'{') {
        let sig = std::str::from_utf8(&text[..brace_pos]).unwrap_or("").trim();
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

fn find_enclosing_symbol<'a>(node: Node, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    let line = node.start_position().row as u32 + 1;
    symbols
        .iter()
        .filter(|s| s.contains_line(line))
        .min_by_key(|s| s.range.end_line - s.range.start_line)
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
    fn parse_simple_class() {
        let source =
            b"package com.example;\n\npublic class App {\n    public void run() {\n    }\n}\n";
        let tree = JavaParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = JavaParser::new();
        let pkg = extract_package_name(root, source);
        assert_eq!(pkg, "com.example");
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &[], &mut symbols);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "run");
        assert_eq!(symbols[0].qualified_name, "com.example.App.run");
        assert_eq!(symbols[0].kind, SymbolKind::Method);
        assert_eq!(symbols[0].visibility, Visibility::Public);
    }

    #[test]
    fn parse_static_method() {
        let source = b"package util;\n\npublic class StringUtils {\n    public static String capitalize(String s) {\n        return s;\n    }\n}\n";
        let tree = JavaParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = JavaParser::new();
        let pkg = extract_package_name(root, source);
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &[], &mut symbols);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "capitalize");
        assert_eq!(symbols[0].kind, SymbolKind::ClassMethod);
    }

    #[test]
    fn parse_constructor() {
        let source = b"package com.example;\n\npublic class App {\n    public App(String name) {\n    }\n}\n";
        let tree = JavaParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = JavaParser::new();
        let pkg = extract_package_name(root, source);
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &[], &mut symbols);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "App");
        assert_eq!(symbols[0].qualified_name, "com.example.App.App");
    }

    #[test]
    fn parse_interface_methods() {
        let source = b"package svc;\n\npublic interface Greeting {\n    String greet(String name);\n    default String greetAll(String[] names) {\n        return \"\";\n    }\n}\n";
        let tree = JavaParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = JavaParser::new();
        let pkg = extract_package_name(root, source);
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &[], &mut symbols);
        assert_eq!(symbols.len(), 2);
        let decl = symbols.iter().find(|s| s.name == "greet").unwrap();
        assert_eq!(decl.kind, SymbolKind::TraitMethodDecl);
        assert_eq!(decl.qualified_name, "svc.Greeting.greet");
        let default = symbols.iter().find(|s| s.name == "greetAll").unwrap();
        assert_eq!(default.kind, SymbolKind::Method);
    }

    #[test]
    fn parse_imports() {
        let source = b"package com.example;\n\nimport java.util.List;\nimport java.util.ArrayList;\nimport com.example.utils.StringUtils;\n\npublic class App {\n}\n";
        let tree = JavaParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = JavaParser::new();
        let mut imports = Vec::new();
        parser.extract_imports(root, source, 1, &mut imports);
        assert_eq!(imports.len(), 3);
        assert!(imports
            .iter()
            .any(|i| i.local_name == "List" && i.qualified_target == "java.util.List"));
        assert!(imports.iter().any(|i| i.local_name == "StringUtils"
            && i.qualified_target == "com.example.utils.StringUtils"));
    }

    #[test]
    fn parse_wildcard_import_skipped() {
        let source = b"package com.example;\n\nimport java.util.*;\n\npublic class App {\n}\n";
        let tree = JavaParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = JavaParser::new();
        let mut imports = Vec::new();
        parser.extract_imports(root, source, 1, &mut imports);
        assert!(imports.is_empty());
    }

    #[test]
    fn parse_method_calls() {
        let source = b"package com.example;\n\npublic class App {\n    public void run() {\n        process();\n        System.out.println(\"hi\");\n    }\n    private void process() {\n    }\n}\n";
        let tree = JavaParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = JavaParser::new();
        let pkg = extract_package_name(root, source);
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &[], &mut symbols);
        for (i, s) in symbols.iter_mut().enumerate() {
            s.id = (i + 1) as i64;
        }
        let mut refs = Vec::new();
        parser.extract_calls(root, source, 1, &symbols, &mut refs);
        assert!(refs.len() >= 2);
    }

    #[test]
    fn parse_private_visibility() {
        let source =
            b"package com.example;\n\npublic class App {\n    private void secret() {\n    }\n}\n";
        let tree = JavaParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = JavaParser::new();
        let pkg = extract_package_name(root, source);
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &[], &mut symbols);
        assert_eq!(symbols[0].visibility, Visibility::Private);
    }

    #[test]
    fn parse_no_package() {
        let source =
            b"public class Main {\n    public static void main(String[] args) {\n    }\n}\n";
        let tree = JavaParser::parse_source(source).unwrap();
        let root = tree.root_node();
        let parser = JavaParser::new();
        let pkg = extract_package_name(root, source);
        assert_eq!(pkg, "");
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, &pkg, &[], &mut symbols);
        assert_eq!(symbols[0].qualified_name, "Main.main");
    }
}
