use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};

use who_core::index::Index;
use who_core::lang::{detect_language, LanguageParser};
use who_core::resolve::resolve_all_calls;
use who_lang_rust::RustParser;

pub struct IndexOpts {
    pub path: String,
    pub lang: Option<String>,
    pub clean: bool,
    pub no_gitignore: bool,
    #[allow(dead_code)]
    pub include: Option<String>,
    pub exclude: Option<String>,
}

pub fn run(opts: IndexOpts) -> Result<()> {
    let start = Instant::now();
    let root = Path::new(&opts.path)
        .canonicalize()
        .context("invalid path")?;
    let index_dir = root.join(".who-ast");
    let index_path = index_dir.join("index.sqlite");

    let index = if opts.clean && index_path.exists() {
        std::fs::remove_file(&index_path)?;
        Index::create(&index_path)?
    } else {
        Index::create(&index_path)?
    };

    let parsers: Vec<Box<dyn LanguageParser>> = vec![Box::new(RustParser::new())];

    let mut walker = ignore::WalkBuilder::new(&root);
    walker.hidden(true);

    if opts.no_gitignore {
        walker.git_ignore(false);
    }

    let exclude_patterns: Vec<&str> = opts
        .exclude
        .as_deref()
        .map(|s| s.split(',').collect())
        .unwrap_or_default();

    let mut total_files = 0u64;
    let mut total_symbols = 0u64;
    let mut total_imports = 0u64;
    let mut total_calls = 0u64;

    index.transaction()?;

    for entry in walker.build() {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        if let Some(ref lang_filter) = opts.lang {
            if detect_language(path) != Some(lang_filter.as_str()) {
                continue;
            }
        }

        let should_exclude = exclude_patterns
            .iter()
            .any(|pat| path.to_str().map(|s| s.contains(pat)).unwrap_or(false));
        if should_exclude {
            continue;
        }

        let lang = match detect_language(path) {
            Some(l) => l,
            None => continue,
        };

        let parser = match parsers.iter().find(|p| p.language_id() == lang) {
            Some(p) => p,
            None => continue,
        };

        let rel_path = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let metadata = std::fs::metadata(path)?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let source = std::fs::read(path)?;
        let hash = format!("{:x}", simple_hash(&source));

        if let Some(existing) = index.find_file(&rel_path)? {
            if existing.hash == hash {
                continue;
            }
            index.delete_file_data(existing.id)?;
        }

        let file_id = index.insert_file(&rel_path, lang, mtime, &hash)?;

        match parser.parse_file(&index, file_id, &source) {
            Ok(result) => {
                total_files += 1;
                total_symbols += result.symbols_count as u64;
                total_imports += result.imports_count as u64;
                total_calls += result.calls_count as u64;
            }
            Err(e) => {
                eprintln!("warning: failed to parse {rel_path}: {e}");
            }
        }
    }

    let resolved = resolve_all_calls(&index)?;

    index.commit()?;

    let elapsed = start.elapsed();
    eprintln!(
        "Indexed {total_files} files in {:.2}s",
        elapsed.as_secs_f64()
    );
    eprintln!("  {total_symbols} symbols, {total_imports} imports, {total_calls} call sites");
    eprintln!("  {resolved} calls resolved");

    let metadata = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "root": root.to_string_lossy(),
        "files": total_files,
        "symbols": total_symbols,
        "imports": total_imports,
        "calls": total_calls,
        "elapsed_ms": elapsed.as_millis(),
    });

    let metadata_path = index_dir.join("metadata.json");
    std::fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)?;

    Ok(())
}

fn simple_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 5381;
    for &byte in data {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}
