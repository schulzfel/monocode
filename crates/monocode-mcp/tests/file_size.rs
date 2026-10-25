//! Every Rust file in this crate stays at or under 500 lines.
use std::path::{Path, PathBuf};

const MAX_LINES: usize = 500;

/// Helpers live in a `cfg(test)` module, so test-only lints apply.
#[cfg(test)]
mod support {
    use super::*;

    pub(super) fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
}
use support::rust_files;

#[test]
fn no_rust_file_is_over_500_lines() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in ["src", "tests"] {
        rust_files(&root.join(dir), &mut files);
    }
    assert!(files.len() > 5, "the walk found the sources: {files:?}");
    let long: Vec<_> = files
        .iter()
        .map(|path| (path, std::fs::read_to_string(path).unwrap().lines().count()))
        .filter(|(_, lines)| *lines > MAX_LINES)
        .collect();
    assert!(long.is_empty(), "files over {MAX_LINES} lines: {long:?}");
}
