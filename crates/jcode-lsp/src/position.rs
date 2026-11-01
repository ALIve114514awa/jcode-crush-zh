use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Encoding {
    Utf8,
    Utf16,
    Utf32,
}

impl Encoding {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("utf-16") {
            "utf-8" => Ok(Self::Utf8),
            "utf-16" => Ok(Self::Utf16),
            "utf-32" => Ok(Self::Utf32),
            other => bail!("Unsupported LSP position encoding: {other}"),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf16 => "utf-16",
            Self::Utf32 => "utf-32",
        }
    }

    fn width(self, ch: char) -> usize {
        match self {
            Self::Utf8 => ch.len_utf8(),
            Self::Utf16 => ch.len_utf16(),
            Self::Utf32 => 1,
        }
    }

    /// Tool coordinates count Unicode scalars, starting at one.
    pub(crate) fn position(self, text: &str, line: usize, column: usize) -> Result<Value> {
        if line == 0 || column == 0 {
            bail!("line and column must be at least 1");
        }
        let source = text
            .split('\n')
            .nth(line - 1)
            .context("line is outside the file")?;
        let source = source.strip_suffix('\r').unwrap_or(source);
        if column - 1 > source.chars().count() {
            bail!("column is outside the line");
        }
        let character: usize = source
            .chars()
            .take(column - 1)
            .map(|ch| self.width(ch))
            .sum();
        Ok(json!({"line": line - 1, "character": character}))
    }

    pub(crate) fn end(self, text: &str) -> Value {
        let line = text.bytes().filter(|byte| *byte == b'\n').count();
        let tail = text.rsplit('\n').next().unwrap_or("");
        let character: usize = tail.chars().map(|ch| self.width(ch)).sum();
        json!({"line": line, "character": character})
    }
}

/// Canonical Windows paths have a verbatim prefix which is not a file URI.
pub(crate) fn uri(path: &Path) -> Result<String> {
    #[cfg(windows)]
    let normalized = {
        let source = path.to_string_lossy();
        if let Some(rest) = source.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{rest}"))
        } else if let Some(rest) = source.strip_prefix(r"\\?\") {
            PathBuf::from(rest)
        } else {
            path.to_path_buf()
        }
    };
    #[cfg(not(windows))]
    let normalized: PathBuf = path.to_path_buf();
    Url::from_file_path(normalized)
        .map(|url| url.to_string())
        .map_err(|_| anyhow::anyhow!("Cannot convert absolute file path to an LSP URI"))
}

/// Equivalent file URIs must address the same document cache. For example,
/// TypeScript returns a lower-case, percent-escaped Windows drive letter.
pub(crate) fn document_key(uri: &str) -> String {
    let normalized = Url::parse(uri)
        .ok()
        .filter(|url| url.scheme() == "file")
        .and_then(|url| url.to_file_path().ok())
        .and_then(|path| self::uri(&path).ok())
        .unwrap_or_else(|| uri.into());
    #[cfg(windows)]
    {
        normalized.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        normalized
    }
}

pub(crate) fn language_id(extension: &str) -> &str {
    match extension {
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "rs" => "rust",
        "py" => "python",
        "sh" => "shellscript",
        "cs" => "csharp",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_columns_are_converted_for_each_encoding() {
        let text = "中😀symbol\r\nlast\r\n";
        assert_eq!(
            Encoding::Utf16.position(text, 1, 3).unwrap(),
            json!({"line":0,"character":3})
        );
        assert_eq!(
            Encoding::Utf8.position(text, 1, 3).unwrap(),
            json!({"line":0,"character":7})
        );
        assert_eq!(
            Encoding::Utf32.position(text, 1, 3).unwrap(),
            json!({"line":0,"character":2})
        );
        assert_eq!(
            Encoding::Utf16.position(text, 3, 1).unwrap(),
            json!({"line":2,"character":0})
        );
        assert!(Encoding::Utf16.position(text, 0, 1).is_err());
        assert!(Encoding::Utf16.position(text, 2, 6).is_err());
        assert!(Encoding::Utf16.position(text, 4, 1).is_err());
    }

    #[test]
    fn file_uri_round_trips_spaces_chinese_and_emoji() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("中文 空格😀.ts");
        std::fs::write(&path, "").unwrap();
        let canonical = path.canonicalize().unwrap();
        let url = Url::parse(&uri(&canonical).unwrap()).unwrap();
        assert_eq!(
            url.to_file_path().unwrap().canonicalize().unwrap(),
            canonical
        );
        assert!(url.as_str().contains("%20"));
        assert!(!url.as_str().contains('\\'));
    }

    #[cfg(windows)]
    #[test]
    fn equivalent_windows_file_uris_share_a_document_key() {
        assert_eq!(
            document_key("file:///C:/Project/A%20B.ts"),
            document_key("file:///c%3A/Project/A%20B.ts")
        );
    }
}
