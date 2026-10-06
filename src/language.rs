use std::{fmt, path::Path, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    JavaScript,
    Lua,
}

impl Language {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "py" => Some(Self::Python),
            "js" | "mjs" | "cjs" => Some(Self::JavaScript),
            "lua" => Some(Self::Lua),
            _ => None,
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Python => "py",
            Self::JavaScript => "js",
            Self::Lua => "lua",
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::Lua => "lua",
        })
    }
}

impl FromStr for Language {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "python" | "py" => Ok(Self::Python),
            "javascript" | "js" | "node" => Ok(Self::JavaScript),
            "lua" => Ok(Self::Lua),
            _ => Err(format!(
                "unsupported language `{value}` (expected python, javascript, or lua)"
            )),
        }
    }
}
