use crate::calls::{CallEdge, Resolution};
use crate::index::Index;
use crate::symbol::Symbol;
use crate::target::Target;
use anyhow::{Context, Result};

pub struct ResolvedTarget {
    pub symbol: Symbol,
    pub file_path: String,
}

pub fn resolve_target(index: &Index, target: &Target) -> Result<ResolvedTarget> {
    match target {
        Target::FileLine { file, line } => resolve_file_line(index, file, *line),
        Target::FileSymbol { file, symbol } => resolve_file_symbol(index, file, symbol),
    }
}

fn resolve_file_line(index: &Index, file: &str, line: u32) -> Result<ResolvedTarget> {
    let file_entry = index
        .find_file(file)?
        .with_context(|| format!("file not in index: {file}"))?;

    let symbols = index.symbols_in_file(file_entry.id)?;
    let enclosing = symbols
        .into_iter()
        .filter(|s| s.contains_line(line))
        .min_by_key(|s| s.range.end_line - s.range.start_line);

    match enclosing {
        Some(symbol) => Ok(ResolvedTarget {
            file_path: file_entry.path.clone(),
            symbol,
        }),
        None => anyhow::bail!(crate::error::WhoError::TargetOutsideSymbol),
    }
}

fn resolve_file_symbol(index: &Index, file: &str, symbol_name: &str) -> Result<ResolvedTarget> {
    let file_entry = index
        .find_file(file)?
        .with_context(|| format!("file not in index: {file}"))?;

    let symbols = index.symbols_in_file(file_entry.id)?;
    let matches: Vec<_> = symbols
        .into_iter()
        .filter(|s| s.name == symbol_name)
        .collect();

    match matches.len() {
        0 => anyhow::bail!(crate::error::WhoError::NoMatch),
        1 => Ok(ResolvedTarget {
            file_path: file_entry.path.clone(),
            symbol: matches.into_iter().next().unwrap(),
        }),
        n => anyhow::bail!(crate::error::WhoError::AmbiguousTarget(n)),
    }
}

pub fn find_callers(index: &Index, target_symbol_id: i64) -> Result<Vec<CallerResult>> {
    let edges = index.call_edges_to(target_symbol_id)?;
    let mut results = Vec::new();

    for edge in edges {
        let caller_sym = index.find_symbol_by_id(edge.caller_symbol_id)?;
        let reference = index.find_ref_by_id(edge.ref_id)?;
        if let (Some(sym), Some(r)) = (caller_sym, reference) {
            let file = index.find_file_by_id(sym.file_id)?;
            results.push(CallerResult {
                caller_symbol: sym,
                call_edge: edge,
                call_text: r.text,
                file_path: file.map(|f| f.path).unwrap_or_default(),
                line: r.start_line,
                column: r.start_col,
            });
        }
    }

    Ok(results)
}

pub struct CallerResult {
    pub caller_symbol: Symbol,
    pub call_edge: CallEdge,
    pub call_text: String,
    pub file_path: String,
    pub line: u32,
    pub column: u32,
}

/// Run import-aware call resolution across all indexed files.
///
/// For each unresolved call ref, tries to match the callee name against
/// imports, local symbols, and global unique names. Creates call edges
/// in the `calls` table for each resolved call.
pub fn resolve_all_calls(index: &Index) -> Result<usize> {
    let file_ids = index.all_file_ids()?;
    let mut total_resolved = 0;

    for file_id in file_ids {
        total_resolved += resolve_file_calls(index, file_id)?;
    }

    Ok(total_resolved)
}

fn resolve_file_calls(index: &Index, file_id: i64) -> Result<usize> {
    let call_refs = index.call_refs_in_source_file(file_id)?;
    if call_refs.is_empty() {
        return Ok(0);
    }

    let imports = index.imports_in_file(file_id)?;
    let local_symbols = index.symbols_in_file(file_id)?;
    let mut resolved = 0;

    for r in &call_refs {
        let callee_name = extract_callee_name(&r.text);
        if callee_name.is_empty() {
            continue;
        }

        // Strategy 1: match against imports
        if let Some(imp) = imports.iter().find(|i| i.local_name == callee_name) {
            // Try exact qualified_target, then with crate:: prefix
            let target_paths = [
                imp.qualified_target.clone(),
                format!("crate::{}", imp.qualified_target),
            ];
            let mut found = false;
            for path in &target_paths {
                let candidates = index.find_symbols_by_qualified_name(path)?;
                if candidates.len() == 1 {
                    let target = &candidates[0];
                    let confidence = 0.75;
                    index.update_ref_target(r.id, target.id, confidence)?;
                    if let Some(caller_id) = r.source_symbol_id {
                        index.insert_call(&CallEdge {
                            id: 0,
                            caller_symbol_id: caller_id,
                            callee_symbol_id: Some(target.id),
                            callee_name: Some(callee_name.to_string()),
                            candidate_symbol_ids: vec![],
                            ref_id: r.id,
                            confidence,
                            resolution: Resolution::Resolved,
                        })?;
                    }
                    resolved += 1;
                    found = true;
                    break;
                }
            }
            if found {
                continue;
            }
        }

        // Strategy 1b: package-qualified call (Go's pkg.Func() pattern)
        // e.g. "utils.Process()" → prefix "utils", find import with local_name "utils",
        // then look up symbol with qualified_name "utils.Process"
        if let Some(prefix) = extract_call_prefix(&r.text) {
            if let Some(imp) = imports.iter().find(|i| i.local_name == prefix) {
                let pkg_name = imp
                    .qualified_target
                    .rsplit('/')
                    .next()
                    .unwrap_or(&imp.qualified_target);
                let qname = format!("{pkg_name}.{callee_name}");
                let candidates = index.find_symbols_by_qualified_name(&qname)?;
                if candidates.len() == 1 {
                    let target = &candidates[0];
                    let confidence = 0.80;
                    index.update_ref_target(r.id, target.id, confidence)?;
                    if let Some(caller_id) = r.source_symbol_id {
                        index.insert_call(&CallEdge {
                            id: 0,
                            caller_symbol_id: caller_id,
                            callee_symbol_id: Some(target.id),
                            callee_name: Some(callee_name.to_string()),
                            candidate_symbol_ids: vec![],
                            ref_id: r.id,
                            confidence,
                            resolution: Resolution::Resolved,
                        })?;
                    }
                    resolved += 1;
                    continue;
                }
            }
        }

        // Strategy 2: match symbol in same file
        if let Some(sym) = local_symbols.iter().find(|s| s.name == callee_name) {
            let confidence = 0.60;
            index.update_ref_target(r.id, sym.id, confidence)?;
            if let Some(caller_id) = r.source_symbol_id {
                index.insert_call(&CallEdge {
                    id: 0,
                    caller_symbol_id: caller_id,
                    callee_symbol_id: Some(sym.id),
                    callee_name: Some(callee_name.to_string()),
                    candidate_symbol_ids: vec![],
                    ref_id: r.id,
                    confidence,
                    resolution: Resolution::Resolved,
                })?;
            }
            resolved += 1;
            continue;
        }

        // Strategy 3: global unique name match
        let global = index.find_symbols_by_name(callee_name)?;
        if global.len() == 1 {
            let target = &global[0];
            let confidence = 0.45;
            index.update_ref_target(r.id, target.id, confidence)?;
            if let Some(caller_id) = r.source_symbol_id {
                index.insert_call(&CallEdge {
                    id: 0,
                    caller_symbol_id: caller_id,
                    callee_symbol_id: Some(target.id),
                    callee_name: Some(callee_name.to_string()),
                    candidate_symbol_ids: vec![],
                    ref_id: r.id,
                    confidence,
                    resolution: Resolution::Resolved,
                })?;
            }
            resolved += 1;
        } else if global.len() > 1 {
            // Ambiguous — store candidates but don't resolve
            if let Some(caller_id) = r.source_symbol_id {
                let candidate_ids: Vec<i64> = global.iter().map(|s| s.id).collect();
                index.insert_call(&CallEdge {
                    id: 0,
                    caller_symbol_id: caller_id,
                    callee_symbol_id: None,
                    callee_name: Some(callee_name.to_string()),
                    candidate_symbol_ids: candidate_ids,
                    ref_id: r.id,
                    confidence: 0.25,
                    resolution: Resolution::Ambiguous,
                })?;
            }
        }
    }

    Ok(resolved)
}

/// Extract the bare callee function name from a call expression text.
/// e.g. "render_text(ctx, text)" → "render_text"
///      "ctx.render_text(text)" → "render_text"
///      "Foo::new()"            → "new"
fn extract_callee_name(call_text: &str) -> &str {
    let before_paren = match call_text.find('(') {
        Some(pos) => call_text[..pos].trim(),
        None => call_text.trim(),
    };
    // Method call: take after last '.'
    if let Some(dot_pos) = before_paren.rfind('.') {
        return &before_paren[dot_pos + 1..];
    }
    // Qualified path: take after last '::'
    if let Some(colon_pos) = before_paren.rfind("::") {
        return &before_paren[colon_pos + 2..];
    }
    before_paren
}

/// Extract the package/receiver prefix from a call expression.
/// e.g. "pkg.FormatOutput()" → Some("pkg")
///      "fmt.Println()"      → Some("fmt")
///      "obj.field.Method()"  → None (multi-dot = receiver chain, not package)
///      "render_text()"       → None
///      "Foo::new()"          → None
fn extract_call_prefix(call_text: &str) -> Option<&str> {
    let before_paren = match call_text.find('(') {
        Some(pos) => call_text[..pos].trim(),
        None => return None,
    };
    // Only single-dot patterns like `pkg.Func` — skip `a.b.c` chains
    let dot_pos = before_paren.find('.')?;
    if before_paren[dot_pos + 1..].contains('.') {
        return None;
    }
    if before_paren.contains("::") {
        return None;
    }
    let prefix = &before_paren[..dot_pos];
    if prefix.is_empty() {
        return None;
    }
    Some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_callee_simple() {
        assert_eq!(extract_callee_name("render_text(ctx, text)"), "render_text");
    }

    #[test]
    fn test_extract_callee_method() {
        assert_eq!(extract_callee_name("ctx.render_text(text)"), "render_text");
    }

    #[test]
    fn test_extract_callee_qualified() {
        assert_eq!(extract_callee_name("Foo::new()"), "new");
    }

    #[test]
    fn test_extract_callee_nested() {
        assert_eq!(extract_callee_name("self.inner.flush()"), "flush");
    }

    #[test]
    fn test_extract_prefix_package_call() {
        assert_eq!(extract_call_prefix("pkg.FormatOutput()"), Some("pkg"));
        assert_eq!(extract_call_prefix("fmt.Println(\"hi\")"), Some("fmt"));
    }

    #[test]
    fn test_extract_prefix_no_dot() {
        assert_eq!(extract_call_prefix("render_text()"), None);
    }

    #[test]
    fn test_extract_prefix_multi_dot() {
        assert_eq!(extract_call_prefix("obj.field.Method()"), None);
    }

    #[test]
    fn test_extract_prefix_rust_path() {
        assert_eq!(extract_call_prefix("Foo::new()"), None);
    }
}
