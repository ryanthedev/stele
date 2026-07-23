//! Loads and validates the document file — the barricade's outside edge:
//! the file path is the first untrusted input the binary touches.

use std::fmt;
use std::path::Path;

/// Why loading `path` failed. `Display` renders a clean, user-facing
/// message — never a raw `io::Error` debug dump.
#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    InvalidUtf8,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "could not read file: {e}"),
            LoadError::InvalidUtf8 => write!(f, "file is not valid UTF-8"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Reads `path` as UTF-8 text. `fs::read`'s underlying `read_to_end`
/// already retries on `ErrorKind::Interrupted` (EINTR), so no additional
/// retry loop is needed here.
pub fn load_document(path: &Path) -> Result<String, LoadError> {
    let bytes = std::fs::read(path).map_err(LoadError::Io)?;
    String::from_utf8(bytes).map_err(|_| LoadError::InvalidUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dw_5_4_missing_file_produces_clean_error() {
        let err = load_document(Path::new("/nonexistent/does-not-exist.md")).unwrap_err();
        assert!(matches!(err, LoadError::Io(_)));
        let message = err.to_string();
        assert!(message.starts_with("could not read file:"));
        // The message should be readable prose, not a raw Debug dump.
        assert!(!message.contains("Os {"));
    }

    #[test]
    fn test_dw_5_4_invalid_utf8_produces_clean_error() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("stele-invalid-utf8-{}.md", std::process::id()));
        std::fs::write(&path, [0x48, 0x65, 0x6c, 0x6c, 0x6f, 0xff, 0xfe]).unwrap();
        let err = load_document(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(matches!(err, LoadError::InvalidUtf8));
        assert_eq!(err.to_string(), "file is not valid UTF-8");
    }

    #[test]
    fn test_valid_utf8_file_loads_successfully() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("stele-valid-{}.md", std::process::id()));
        std::fs::write(&path, "# Hello\n\nWorld.\n").unwrap();
        let content = load_document(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(content, "# Hello\n\nWorld.\n");
    }
}
