//! Validation of `relative_path` received from peers (PROTOCOL.md section 9.3).
//!
//! The receiver must never trust a path from the network. Everything that could escape the
//! target folder or that Windows cannot store is rejected.

use thiserror::Error;

/// Longest allowed single path segment, in UTF-8 bytes.
pub const MAX_SEGMENT: usize = 255;
/// Longest allowed whole path, in UTF-8 bytes.
pub const MAX_PATH: usize = 4096;

/// Why a path was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PathError {
    /// The path is empty.
    #[error("empty path")]
    Empty,
    /// The path starts with `/`.
    #[error("absolute path")]
    Absolute,
    /// A segment is empty, `.` or `..`.
    #[error("empty, '.' or '..' segment")]
    BadSegment,
    /// A character that is not allowed in a name (`\ : < > " | ? *` or a control character).
    #[error("forbidden character {0:?}")]
    ForbiddenChar(char),
    /// A segment ends with a dot or a space.
    #[error("segment ends with '.' or ' '")]
    TrailingDotOrSpace,
    /// A segment is a reserved Windows device name.
    #[error("reserved Windows name")]
    ReservedName,
    /// A segment or the whole path is too long.
    #[error("path too long")]
    TooLong,
}

const FORBIDDEN: &[char] = &['\\', ':', '<', '>', '"', '|', '?', '*'];

fn is_reserved(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        // Compare bytes: `split_at` would panic inside a multi-byte character.
        s if s.len() == 4 => {
            let b = s.as_bytes();
            (b.starts_with(b"COM") || b.starts_with(b"LPT")) && matches!(b[3], b'1'..=b'9')
        }
        _ => false,
    }
}

/// Check `path` and return its segments.
pub fn validate_relative_path(path: &str) -> Result<Vec<String>, PathError> {
    if path.is_empty() {
        return Err(PathError::Empty);
    }
    if path.len() > MAX_PATH {
        return Err(PathError::TooLong);
    }
    if path.starts_with('/') {
        return Err(PathError::Absolute);
    }
    if let Some(c) = path
        .chars()
        .find(|c| c.is_control() || FORBIDDEN.contains(c))
    {
        return Err(PathError::ForbiddenChar(c));
    }
    let mut out = Vec::new();
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(PathError::BadSegment);
        }
        if seg.len() > MAX_SEGMENT {
            return Err(PathError::TooLong);
        }
        if seg.ends_with('.') || seg.ends_with(' ') {
            return Err(PathError::TrailingDotOrSpace);
        }
        if is_reserved(seg) {
            return Err(PathError::ReservedName);
        }
        out.push(seg.to_string());
    }
    Ok(out)
}

/// Name used when `name` already exists: `a.txt` becomes `a (1).txt`, `a (2).txt`, ...
///
/// The extension is the part after the last dot, unless the dot is the first character.
pub fn with_counter(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({}){}", &name[..i], n, &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

/// First free name, using `exists` to probe. Names compare case-insensitively on Windows, so
/// callers should make `exists` case-insensitive there.
pub fn unique_name(name: &str, exists: impl Fn(&str) -> bool) -> String {
    if !exists(name) {
        return name.to_string();
    }
    let mut n = 1;
    loop {
        let candidate = with_counter(name, n);
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Paths every implementation must accept (used by tests and test vectors).
pub const EXAMPLE_VALID_PATHS: &[&str] = &[
    "a",
    "a.txt",
    "dir/a.txt",
    "photos/2026/img 01.jpg",
    "d\u{e9}j\u{e0}/vu.txt",
    "\u{65e5}\u{672c}\u{8a9e}/\u{30d5}\u{30a1}\u{30a4}\u{30eb}.md",
    ".hidden",
    "dir/.gitignore",
    "con_x",
    "COM10",
    "LPT0",
    "a b/c d",
    "a-b_c(1)/d[2].txt",
];

/// Paths every implementation must reject.
pub const EXAMPLE_INVALID_PATHS: &[&str] = &[
    "",
    "/",
    "/etc/passwd",
    "a//b",
    "a/",
    ".",
    "..",
    "../x",
    "a/../b",
    "a/./b",
    "a/..",
    "a\\b",
    "C:",
    "C:/x",
    "a:b",
    "a<b",
    "a>b",
    "a\"b",
    "a|b",
    "a?b",
    "a*b",
    "a\u{0}b",
    "a\nb",
    "a\u{7f}b",
    "name.",
    "name ",
    "dir./x",
    "CON",
    "con",
    "nul.txt",
    "Aux.tar.gz",
    "dir/PRN",
    "COM1",
    "com9.txt",
    "LPT1",
    "lpt5.log",
    "CON .txt",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_paths() {
        for p in EXAMPLE_VALID_PATHS {
            assert!(validate_relative_path(p).is_ok(), "should accept {p:?}");
        }
    }

    #[test]
    fn rejects_invalid_paths() {
        for p in EXAMPLE_INVALID_PATHS {
            assert!(validate_relative_path(p).is_err(), "should reject {p:?}");
        }
    }

    #[test]
    fn returns_segments() {
        assert_eq!(
            validate_relative_path("a/b/c.txt").unwrap(),
            vec!["a", "b", "c.txt"]
        );
    }

    #[test]
    fn length_limits() {
        assert!(validate_relative_path(&"a".repeat(MAX_SEGMENT)).is_ok());
        assert_eq!(
            validate_relative_path(&"a".repeat(MAX_SEGMENT + 1)),
            Err(PathError::TooLong)
        );
        let long = vec!["a".repeat(200); 21].join("/");
        assert_eq!(validate_relative_path(&long), Err(PathError::TooLong));
        // 255 multi-byte characters exceed 255 bytes.
        assert_eq!(
            validate_relative_path(&"\u{e9}".repeat(200)),
            Err(PathError::TooLong)
        );
    }

    #[test]
    fn specific_errors() {
        assert_eq!(validate_relative_path(""), Err(PathError::Empty));
        assert_eq!(validate_relative_path("/a"), Err(PathError::Absolute));
        assert_eq!(validate_relative_path("a/../b"), Err(PathError::BadSegment));
        assert_eq!(
            validate_relative_path("a:b"),
            Err(PathError::ForbiddenChar(':'))
        );
        assert_eq!(
            validate_relative_path("a."),
            Err(PathError::TrailingDotOrSpace)
        );
        assert_eq!(validate_relative_path("nul"), Err(PathError::ReservedName));
    }

    #[test]
    fn conflict_names() {
        assert_eq!(with_counter("a.txt", 1), "a (1).txt");
        assert_eq!(with_counter("archive.tar.gz", 2), "archive.tar (2).gz");
        assert_eq!(with_counter(".bashrc", 1), ".bashrc (1)");
        assert_eq!(with_counter("noext", 3), "noext (3)");
    }

    #[test]
    fn unique_name_skips_existing() {
        let taken = ["a.txt", "a (1).txt", "a (2).txt"];
        assert_eq!(unique_name("a.txt", |n| taken.contains(&n)), "a (3).txt");
        assert_eq!(unique_name("b.txt", |n| taken.contains(&n)), "b.txt");
    }
}
