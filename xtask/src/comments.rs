mod css;
mod cursor;
mod git;
mod markup;
mod rust;
mod script;
mod shell;
mod toml;
mod yaml;

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Rust,
    Toml,
    Yaml,
    Css,
    Scss,
    Markup,
    Script,
    Jsx,
    Shell,
    PowerShell,
    GitPatterns,
}

impl Language {
    pub fn from_path(path: &Path) -> Option<Self> {
        let file_name = path.file_name()?.to_str()?.to_ascii_lowercase();
        if matches!(file_name.as_str(), ".gitignore" | ".gitattributes") {
            return Some(Self::GitPatterns);
        }
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "rs" => Some(Self::Rust),
            "toml" => Some(Self::Toml),
            "yml" | "yaml" => Some(Self::Yaml),
            "css" => Some(Self::Css),
            "scss" => Some(Self::Scss),
            "html" | "svg" => Some(Self::Markup),
            "js" | "mjs" | "cjs" | "ts" | "mts" | "cts" => Some(Self::Script),
            "jsx" | "tsx" => Some(Self::Jsx),
            "sh" => Some(Self::Shell),
            "ps1" => Some(Self::PowerShell),
            _ => None,
        }
    }

    pub fn comment_lines(self, source: &str) -> Vec<usize> {
        match self {
            Self::Rust => rust::comment_lines(source),
            Self::Toml => toml::comment_lines(source),
            Self::Yaml => yaml::comment_lines(source),
            Self::Css => css::comment_lines(source),
            Self::Scss => css::scss_comment_lines(source),
            Self::Markup => markup::comment_lines(source),
            Self::Script => script::comment_lines(source),
            Self::Jsx => script::jsx_comment_lines(source),
            Self::Shell => shell::posix_comment_lines(source),
            Self::PowerShell => shell::powershell_comment_lines(source),
            Self::GitPatterns => git::comment_lines(source),
        }
    }
}
