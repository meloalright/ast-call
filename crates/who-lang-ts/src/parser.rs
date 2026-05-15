use anyhow::{Context, Result};
use tree_sitter::{Node, Parser, Tree};
use who_core::index::Index;
use who_core::lang::{LanguageParser, ParsedFile};
use who_core::refs::{RefKind, Reference};
use who_core::symbol::{Import, SourceRange, Symbol, SymbolKind, Visibility};

pub struct TypeScriptParser {
    _private: (),
}

impl TypeScriptParser {
    pub fn new() -> Self {
        Self { _private: () }
    }

    fn create_parser() -> Result<Parser> {
        let mut parser = Parser::new();
        let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT;
        parser
            .set_language(&language.into())
            .context("failed to set TypeScript grammar")?;
        Ok(parser)
    }

    fn create_tsx_parser() -> Result<Parser> {
        let mut parser = Parser::new();
        let language = tree_sitter_typescript::LANGUAGE_TSX;
        parser
            .set_language(&language.into())
            .context("failed to set TSX grammar")?;
        Ok(parser)
    }

    fn parse_source(source: &[u8], tsx: bool) -> Result<Tree> {
        let mut parser = if tsx {
            Self::create_tsx_parser()?
        } else {
            Self::create_parser()?
        };
        parser
            .parse(source, None)
            .context("tree-sitter parse returned None")
    }

    fn extract_symbols(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        class_name: Option<&str>,
        symbols: &mut Vec<Symbol>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_declaration" => {
                    if let Some(sym) = self.extract_function(child, source, file_id) {
                        symbols.push(sym);
                    }
                }
                "export_statement" => {
                    self.extract_symbols(child, source, file_id, class_name, symbols);
                }
                "class_declaration" => {
                    self.extract_class(child, source, file_id, symbols);
                }
                "abstract_class_declaration" => {
                    self.extract_class(child, source, file_id, symbols);
                }
                "interface_declaration" => {
                    self.extract_interface(child, source, file_id, symbols);
                }
                "lexical_declaration" => {
                    self.extract_arrow_functions(child, source, file_id, symbols);
                }
                "method_definition" => {
                    if let Some(sym) =
                        self.extract_method(child, source, file_id, class_name.unwrap_or(""))
                    {
                        symbols.push(sym);
                    }
                }
                _ => {
                    self.extract_symbols(child, source, file_id, class_name, symbols);
                }
            }
        }
    }

    fn extract_function(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source).to_string();

        let visibility = if is_exported(node) {
            Visibility::Public
        } else {
            Visibility::Private
        };

        let signature = extract_signature(node, source);

        Some(Symbol {
            id: 0,
            file_id,
            name: name.clone(),
            qualified_name: name,
            kind: SymbolKind::Function,
            range: node_range(node),
            signature: Some(signature),
            visibility,
        })
    }

    fn extract_class(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        symbols: &mut Vec<Symbol>,
    ) {
        let class_name = match node.child_by_field_name("name") {
            Some(n) => node_text(n, source).to_string(),
            None => return,
        };

        let body = match node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        };

        self.extract_symbols(body, source, file_id, Some(&class_name), symbols);
    }

    fn extract_method(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        class_name: &str,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source).to_string();

        let qualified_name = if class_name.is_empty() {
            name.clone()
        } else {
            format!("{class_name}.{name}")
        };

        let visibility = method_visibility(node, source);
        let signature = extract_signature(node, source);

        Some(Symbol {
            id: 0,
            file_id,
            name,
            qualified_name,
            kind: SymbolKind::ClassMethod,
            range: node_range(node),
            signature: Some(signature),
            visibility,
        })
    }

    fn extract_interface(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        symbols: &mut Vec<Symbol>,
    ) {
        let iface_name = match node.child_by_field_name("name") {
            Some(n) => node_text(n, source).to_string(),
            None => return,
        };

        let body = match node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        };

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "method_signature" {
                let method_name = match child.child_by_field_name("name") {
                    Some(n) => node_text(n, source).to_string(),
                    None => continue,
                };
                let qualified_name = format!("{iface_name}.{method_name}");
                let sig_text = node_text(child, source).to_string();
                symbols.push(Symbol {
                    id: 0,
                    file_id,
                    name: method_name,
                    qualified_name,
                    kind: SymbolKind::TraitMethodDecl,
                    range: node_range(child),
                    signature: Some(sig_text),
                    visibility: Visibility::Public,
                });
            }
        }
    }

    fn extract_arrow_functions(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        symbols: &mut Vec<Symbol>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "variable_declarator" {
                continue;
            }
            let name_node = match child.child_by_field_name("name") {
                Some(n) => n,
                None => continue,
            };
            let value_node = match child.child_by_field_name("value") {
                Some(n) => n,
                None => continue,
            };
            if value_node.kind() != "arrow_function" {
                continue;
            }
            let name = node_text(name_node, source).to_string();
            let visibility = if is_exported(node) {
                Visibility::Public
            } else {
                Visibility::Private
            };
            let signature = extract_signature(node, source);
            symbols.push(Symbol {
                id: 0,
                file_id,
                name: name.clone(),
                qualified_name: name,
                kind: SymbolKind::ArrowFunction,
                range: node_range(value_node),
                signature: Some(signature),
                visibility,
            });
        }
    }

    fn extract_imports(&self, node: Node, source: &[u8], file_id: i64, imports: &mut Vec<Import>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "import_statement" => {
                    self.extract_import_statement(child, source, file_id, imports);
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
        let source_node = match node.child_by_field_name("source") {
            Some(n) => n,
            None => return,
        };
        let raw_path = node_text(source_node, source);
        let import_path = raw_path.trim_matches(|c| c == '\'' || c == '"').to_string();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "import_clause" => {
                    self.extract_import_clause(child, source, file_id, &import_path, imports);
                }
                "import_specifier" => {
                    self.extract_import_specifier(child, source, file_id, &import_path, imports);
                }
                "named_imports" => {
                    self.extract_named_imports(child, source, file_id, &import_path, imports);
                }
                "identifier" | "type_identifier" => {
                    let local_name = node_text(child, source).to_string();
                    if local_name != "import" && local_name != "from" && local_name != "type" {
                        imports.push(Import {
                            id: 0,
                            file_id,
                            local_name,
                            qualified_target: import_path.clone(),
                            alias: None,
                            start_line: node.start_position().row as u32 + 1,
                            start_col: node.start_position().column as u32 + 1,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_import_clause(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        import_path: &str,
        imports: &mut Vec<Import>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    let local_name = node_text(child, source).to_string();
                    imports.push(Import {
                        id: 0,
                        file_id,
                        local_name,
                        qualified_target: import_path.to_string(),
                        alias: None,
                        start_line: child.start_position().row as u32 + 1,
                        start_col: child.start_position().column as u32 + 1,
                    });
                }
                "named_imports" => {
                    self.extract_named_imports(child, source, file_id, import_path, imports);
                }
                "namespace_import" => {
                    let mut ns_cursor = child.walk();
                    for ns_child in child.children(&mut ns_cursor) {
                        if ns_child.kind() == "identifier" {
                            let local_name = node_text(ns_child, source).to_string();
                            imports.push(Import {
                                id: 0,
                                file_id,
                                local_name,
                                qualified_target: import_path.to_string(),
                                alias: Some("*".to_string()),
                                start_line: child.start_position().row as u32 + 1,
                                start_col: child.start_position().column as u32 + 1,
                            });
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_named_imports(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        import_path: &str,
        imports: &mut Vec<Import>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "import_specifier" {
                self.extract_import_specifier(child, source, file_id, import_path, imports);
            }
        }
    }

    fn extract_import_specifier(
        &self,
        node: Node,
        source: &[u8],
        file_id: i64,
        import_path: &str,
        imports: &mut Vec<Import>,
    ) {
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => return,
        };
        let original_name = node_text(name_node, source).to_string();

        let alias_node = node.child_by_field_name("alias");
        let (local_name, alias) = if let Some(a) = alias_node {
            let alias_text = node_text(a, source).to_string();
            (alias_text.clone(), Some(alias_text))
        } else {
            (original_name.clone(), None)
        };

        let qualified = format!("{import_path}.{original_name}");

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
                        start_line: callee.start_position().row as u32 + 1,
                        start_col: callee.start_position().column as u32 + 1,
                        end_line: callee.end_position().row as u32 + 1,
                        end_col: callee.end_position().column as u32 + 1,
                        text: truncate_utf8(&call_text, 120),
                        confidence: 0.0,
                    });
                }
            }
            self.extract_calls(child, source, file_id, symbols, refs);
        }
    }
}

impl Default for TypeScriptParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageParser for TypeScriptParser {
    fn language_id(&self) -> &str {
        "typescript"
    }

    fn file_extensions(&self) -> &[&str] {
        &["ts", "tsx"]
    }

    fn parse_file(&self, index: &Index, file_id: i64, source: &[u8]) -> Result<ParsedFile> {
        let file_entry = index.find_file_by_id(file_id)?;
        let is_tsx = file_entry
            .as_ref()
            .map(|f| f.path.ends_with(".tsx"))
            .unwrap_or(false);

        let tree = Self::parse_source(source, is_tsx)?;
        let root = tree.root_node();

        let mut symbols = Vec::new();
        self.extract_symbols(root, source, file_id, None, &mut symbols);

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

fn is_exported(node: Node) -> bool {
    node.parent()
        .map(|p| p.kind() == "export_statement")
        .unwrap_or(false)
}

fn method_visibility(node: Node, source: &[u8]) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "accessibility_modifier" {
            return match node_text(child, source) {
                "private" => Visibility::Private,
                "protected" => Visibility::PubCrate,
                _ => Visibility::Public,
            };
        }
    }
    Visibility::Public
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

fn extract_signature(node: Node, source: &[u8]) -> String {
    let start = node.start_byte();
    let text = &source[start..];
    if let Some(brace_pos) = text.iter().position(|&b| b == b'{') {
        let sig = std::str::from_utf8(&text[..brace_pos]).unwrap_or("").trim();
        sig.to_string()
    } else if let Some(arrow_pos) = text
        .windows(2)
        .position(|w| w == b"=>")
    {
        let sig = std::str::from_utf8(&text[..arrow_pos + 2])
            .unwrap_or("")
            .trim();
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
        let source = b"function hello() {\n}\n";
        let tree = TypeScriptParser::parse_source(source, false).unwrap();
        let root = tree.root_node();
        let parser = TypeScriptParser::new();
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, None, &mut symbols);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "hello");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
    }

    #[test]
    fn parse_exported_function() {
        let source = b"export function greet(name: string): string {\n  return name;\n}\n";
        let tree = TypeScriptParser::parse_source(source, false).unwrap();
        let root = tree.root_node();
        let parser = TypeScriptParser::new();
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, None, &mut symbols);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "greet");
        assert_eq!(symbols[0].visibility, Visibility::Public);
    }

    #[test]
    fn parse_class_methods() {
        let source = b"class Server {\n  start() {\n  }\n  private stop() {\n  }\n}\n";
        let tree = TypeScriptParser::parse_source(source, false).unwrap();
        let root = tree.root_node();
        let parser = TypeScriptParser::new();
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, None, &mut symbols);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].qualified_name, "Server.start");
        assert_eq!(symbols[0].visibility, Visibility::Public);
        assert_eq!(symbols[1].qualified_name, "Server.stop");
        assert_eq!(symbols[1].visibility, Visibility::Private);
    }

    #[test]
    fn parse_interface_methods() {
        let source = b"interface Speaker {\n  speak(): string;\n  greet(name: string): void;\n}\n";
        let tree = TypeScriptParser::parse_source(source, false).unwrap();
        let root = tree.root_node();
        let parser = TypeScriptParser::new();
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, None, &mut symbols);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].kind, SymbolKind::TraitMethodDecl);
        assert_eq!(symbols[0].qualified_name, "Speaker.speak");
        assert_eq!(symbols[1].qualified_name, "Speaker.greet");
    }

    #[test]
    fn parse_arrow_function() {
        let source = b"const add = (a: number, b: number): number => {\n  return a + b;\n}\n";
        let tree = TypeScriptParser::parse_source(source, false).unwrap();
        let root = tree.root_node();
        let parser = TypeScriptParser::new();
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, None, &mut symbols);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "add");
        assert_eq!(symbols[0].kind, SymbolKind::ArrowFunction);
    }

    #[test]
    fn parse_imports() {
        let source = b"import { readFile, writeFile as write } from 'fs';\nimport path from 'path';\n";
        let tree = TypeScriptParser::parse_source(source, false).unwrap();
        let root = tree.root_node();
        let parser = TypeScriptParser::new();
        let mut imports = Vec::new();
        parser.extract_imports(root, source, 1, &mut imports);
        assert!(imports.iter().any(|i| i.local_name == "readFile"));
        assert!(imports.iter().any(|i| i.local_name == "write"
            && i.qualified_target == "fs.writeFile"));
        assert!(imports.iter().any(|i| i.local_name == "path"));
    }

    #[test]
    fn parse_namespace_import() {
        let source = b"import * as utils from './utils';\n";
        let tree = TypeScriptParser::parse_source(source, false).unwrap();
        let root = tree.root_node();
        let parser = TypeScriptParser::new();
        let mut imports = Vec::new();
        parser.extract_imports(root, source, 1, &mut imports);
        assert!(imports
            .iter()
            .any(|i| i.local_name == "utils" && i.alias == Some("*".to_string())));
    }

    #[test]
    fn parse_calls() {
        let source = b"import { greet } from './greet';\n\nfunction main() {\n  greet('world');\n  console.log('done');\n}\n";
        let tree = TypeScriptParser::parse_source(source, false).unwrap();
        let root = tree.root_node();
        let parser = TypeScriptParser::new();
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, None, &mut symbols);
        for s in &mut symbols {
            s.id = 1;
        }
        let mut refs = Vec::new();
        parser.extract_calls(root, source, 1, &symbols, &mut refs);
        assert!(refs.len() >= 2);
    }

    #[test]
    fn parse_tsx() {
        let source = b"function App(): JSX.Element {\n  return <div>Hello</div>;\n}\n";
        let tree = TypeScriptParser::parse_source(source, true).unwrap();
        let root = tree.root_node();
        let parser = TypeScriptParser::new();
        let mut symbols = Vec::new();
        parser.extract_symbols(root, source, 1, None, &mut symbols);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "App");
    }
}
