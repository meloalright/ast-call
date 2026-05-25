use anyhow::Result;
use std::path::Path;

use crate::index::Index;

pub struct ParsedFile {
    pub file_id: i64,
    pub symbols_count: usize,
    pub imports_count: usize,
    pub calls_count: usize,
}

pub trait LanguageParser: Send + Sync {
    fn language_id(&self) -> &str;

    fn file_extensions(&self) -> &[&str];

    fn can_parse(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| self.file_extensions().contains(&ext))
            .unwrap_or(false)
    }

    fn parse_file(&self, index: &Index, file_id: i64, source: &[u8]) -> Result<ParsedFile>;

    fn resolve_calls(&self, index: &Index, file_id: i64) -> Result<usize>;
}

pub fn detect_language(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()? {
        "rs" => Some("rust"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" => Some("javascript"),
        "py" => Some("python"),
        "go" => Some("go"),
        "java" => Some("java"),
        "lua" => Some("lua"),
        _ => None,
    }
}
