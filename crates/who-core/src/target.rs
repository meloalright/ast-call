use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    FileLine { file: String, line: u32 },
    FileSymbol { file: String, symbol: String },
}

impl Target {
    pub fn file(&self) -> &str {
        match self {
            Self::FileLine { file, .. } | Self::FileSymbol { file, .. } => file,
        }
    }

    pub fn line(&self) -> Option<u32> {
        match self {
            Self::FileLine { line, .. } => Some(*line),
            Self::FileSymbol { .. } => None,
        }
    }
}

impl FromStr for Target {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err("empty target".to_string());
        }

        // file#symbol
        if let Some((file, symbol)) = s.split_once('#') {
            if file.is_empty() || symbol.is_empty() {
                return Err("invalid file#symbol target".to_string());
            }
            return Ok(Target::FileSymbol {
                file: file.to_string(),
                symbol: symbol.to_string(),
            });
        }

        // file:line
        if let Some((file, line_str)) = s.rsplit_once(':') {
            if let Ok(line) = line_str.parse::<u32>() {
                if !file.is_empty() {
                    return Ok(Target::FileLine {
                        file: file.to_string(),
                        line,
                    });
                }
            }
        }

        Err(format!(
            "invalid target '{s}': use file:line or file#symbol"
        ))
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileLine { file, line } => write!(f, "{file}:{line}"),
            Self::FileSymbol { file, symbol } => write!(f, "{file}#{symbol}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file_line() {
        let t: Target = "src/render.rs:42".parse().unwrap();
        assert_eq!(
            t,
            Target::FileLine {
                file: "src/render.rs".to_string(),
                line: 42
            }
        );
    }

    #[test]
    fn parse_file_symbol() {
        let t: Target = "src/render.rs#render_text".parse().unwrap();
        assert_eq!(
            t,
            Target::FileSymbol {
                file: "src/render.rs".to_string(),
                symbol: "render_text".to_string()
            }
        );
    }

    #[test]
    fn reject_plain_symbol() {
        let result: Result<Target, _> = "render_text".parse();
        assert!(result.is_err());
    }

    #[test]
    fn reject_empty() {
        let result: Result<Target, _> = "".parse();
        assert!(result.is_err());
    }
}
