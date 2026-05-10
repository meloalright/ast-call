use anyhow::Result;

use who_core::error::WhoError;
use who_core::index::Index;
use who_core::resolve::resolve_target;
use who_core::symbol::SymbolKind;
use who_core::target::Target;

use crate::output::OutputOpts;

pub fn run(target_str: &str, opts: &OutputOpts) -> Result<()> {
    let target: Target = target_str
        .parse()
        .map_err(|e: String| WhoError::ParseError(e))?;

    let index_path = crate::cmd_callers::find_index_path()?;
    let index = Index::open(&index_path)?;

    let resolved = resolve_target(&index, &target)?;
    let sym = &resolved.symbol;

    let impls = index.find_symbols_by_name(&sym.name)?;
    let impls: Vec<_> = impls
        .into_iter()
        .filter(|s| {
            s.id != sym.id && matches!(s.kind, SymbolKind::TraitMethod | SymbolKind::Method)
        })
        .collect();

    if opts.json {
        let output = serde_json::json!({
            "command": "impl",
            "target": {
                "input": target_str,
                "symbol": sym.name,
                "qualified_name": sym.qualified_name,
                "kind": format!("{:?}", sym.kind).to_lowercase(),
            },
            "implementations": impls.iter().map(|s| {
                let file = index.find_file_by_id(s.file_id).ok().flatten();
                serde_json::json!({
                    "name": s.name,
                    "qualified_name": s.qualified_name,
                    "kind": format!("{:?}", s.kind).to_lowercase(),
                    "file": file.as_ref().map(|f| f.path.as_str()).unwrap_or("?"),
                    "line": s.range.start_line,
                    "column": s.range.start_col,
                })
            }).collect::<Vec<_>>(),
            "total": impls.len(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Trait method:");
        println!("  {}", sym.qualified_name);
        println!(
            "  {}:{}:{}",
            resolved.file_path, sym.range.start_line, sym.range.start_col
        );
        println!();

        if impls.is_empty() {
            println!("No implementations found.");
        } else {
            println!("Implementations:");
            for s in &impls {
                let file = index.find_file_by_id(s.file_id).ok().flatten();
                println!(
                    "  {}:{}:{}\t{}",
                    file.as_ref().map(|f| f.path.as_str()).unwrap_or("?"),
                    s.range.start_line,
                    s.range.start_col,
                    s.qualified_name
                );
            }
            println!();
            println!("{} implementations found.", impls.len());
        }
    }

    Ok(())
}
