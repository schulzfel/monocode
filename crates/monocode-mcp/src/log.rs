//! Diagnostics on standard error; standard output is the MCP channel.
use std::io::Write;

/// Writes one diagnostic line to standard error.
pub(crate) fn line(message: &str) {
    let _ = writeln!(std::io::stderr().lock(), "monocode mcp: {message}");
}
