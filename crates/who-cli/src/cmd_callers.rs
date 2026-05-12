use anyhow::Result;

use who_core::error::WhoError;
use who_core::index::Index;
use who_core::resolve::{find_callers, resolve_target};
use who_core::target::Target;

use crate::output::{self, OutputOpts};

pub fn run(target_str: &str, opts: &OutputOpts) -> Result<()> {
    let target: Target = target_str
        .parse()
        .map_err(|e: String| WhoError::ParseError(e))?;

    let index_path = find_index_path()?;
    let index = Index::open(&index_path)?;

    let resolved = resolve_target(&index, &target)?;
    let callers = find_callers(&index, resolved.symbol.id)?;

    if opts.json {
        output::format_callers_json(target_str, &resolved.symbol, &resolved.file_path, &callers);
    } else {
        output::format_callers_human(target_str, &resolved.symbol, &resolved.file_path, &callers);
    }

    Ok(())
}

pub fn find_index_path() -> Result<std::path::PathBuf> {
    let cwd = std::env::current_dir()?;
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join(".whocall/index.sqlite");
        if candidate.exists() {
            return Ok(candidate);
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return Err(WhoError::IndexMissing(cwd.to_string_lossy().to_string()).into()),
        }
    }
}
