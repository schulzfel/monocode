//! The tool reference in `docs/native/mcp.md` is the catalog, in two
//! tables: the tools every relay lists, then those only a Mono's relay lists.
//! One row per published tool, in `tools/list` order, with its scope (and,
//! in brackets, the scope some calls need too), hints, whether the person
//! confirms it, and the first sentence of its description. When this fails,
//! paste the printed table between the doc's markers.
use monocode_control::{Descriptor, catalog};
use std::fmt::Write as _;

const DOC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/native/mcp.md");
const START: &str = "<!-- tools:start -->";
const END: &str = "<!-- tools:end -->";
const MONO_START: &str = "<!-- mono-tools:start -->";
const MONO_END: &str = "<!-- mono-tools:end -->";

/// The relay's own tool, after the catalog.
const APP_LAUNCH: &str = "| `app_launch` | (relay) | idempotent | no | Start MonoCode when it is not running, and wait up to 30 seconds until it can be controlled. |";

fn first_sentence(text: &str) -> &str {
    text.find(". ").map_or(text, |end| &text[..=end])
}

fn hints(descriptor: &Descriptor) -> String {
    let hints = descriptor.hints;
    let names: Vec<&str> = [
        (hints.read_only, "read-only"),
        (hints.destructive, "destructive"),
        (hints.idempotent, "idempotent"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    if names.is_empty() {
        "(none)".into()
    } else {
        names.join(", ")
    }
}

fn scope(descriptor: &Descriptor) -> String {
    descriptor
        .also
        .iter()
        .fold(format!("`{}`", descriptor.scope.id()), |mut cell, scope| {
            let _ = write!(cell, " (+ `{}`)", scope.id());
            cell
        })
}

/// The table of the catalog's tools a relay lists only to a Mono
/// (`mono`), or to everyone, then the relay's own tool.
fn expected(mono: bool) -> String {
    let mut rows = vec![
        "| Tool | Scope | Hints | Confirmed | Purpose |".to_owned(),
        "| --- | --- | --- | --- | --- |".to_owned(),
    ];
    for descriptor in catalog().into_iter().filter(|d| d.mono_only == mono) {
        rows.push(format!(
            "| `{}` | {} | {} | {} | {} |",
            descriptor.name,
            scope(&descriptor),
            hints(&descriptor),
            if descriptor.confirmed { "yes" } else { "no" },
            first_sentence(descriptor.description).replace('|', "\\|"),
        ));
    }
    if !mono {
        rows.push(APP_LAUNCH.to_owned());
    }
    rows.join("\n")
}

#[test]
fn the_documented_tool_tables_match_the_catalog() {
    let doc = std::fs::read_to_string(DOC).unwrap();
    // The doc marks where each table starts and ends.
    for (start, end, mono, which) in [
        (START, END, false, "tool table"),
        (MONO_START, MONO_END, true, "table of tools for Monos"),
    ] {
        let from = doc.find(start).unwrap() + start.len();
        let documented = doc[from..doc.find(end).unwrap()].trim();
        let expected = expected(mono);
        assert!(
            documented == expected,
            "docs/native/mcp.md's {which} drifted from the catalog. Replace it with:\n\n{expected}\n"
        );
    }
}
