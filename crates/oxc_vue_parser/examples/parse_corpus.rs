//! Parse every `.vue` file under a directory and verify structural
//! invariants: no panics, every byte of the template covered by
//! contiguous sibling spans, and SFC blocks extracted.
//!
//! Usage: `cargo run -p oxc_vue_parser --example parse_corpus -- <dir>`

use std::path::PathBuf;

use oxc_vue_parser::{ast::Node, parse_sfc, parse_template};

fn main() {
    let root = std::env::args().nth(1).expect("usage: parse_corpus <dir>");
    let mut files = Vec::new();
    collect(&PathBuf::from(root), &mut files);
    files.sort();

    let mut parsed = 0u32;
    let mut templates = 0u32;
    let mut elements = 0u64;
    let mut directives = 0u64;
    let mut violations = 0u32;

    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else { continue };
        let sfc = parse_sfc(&source);
        parsed += 1;
        for block in &sfc.blocks {
            if block.name != "template" {
                continue;
            }
            templates += 1;
            // Standalone re-parse of the extracted block content: base_offset
            // 0 keeps spans relative to `block.content` so they can be
            // checked against `block.content.len()` below.
            let nodes = parse_template(block.content, 0);
            if let Err(message) =
                check_contiguous(&nodes, 0, u32::try_from(block.content.len()).unwrap())
            {
                violations += 1;
                eprintln!("SPAN VIOLATION {}: {message}", path.display());
            }
            let (element_count, directive_count) = count(&nodes);
            elements += element_count;
            directives += directive_count;
        }
    }

    println!(
        "files={parsed} templates={templates} elements={elements} directives={directives} span_violations={violations}"
    );
    assert_eq!(violations, 0, "span coverage violations found");
}

/// Sibling nodes must tile `[start, end)` with no gaps or overlaps.
fn check_contiguous(nodes: &[Node], start: u32, end: u32) -> Result<(), String> {
    let mut cursor = start;
    for node in nodes {
        let span = node.span();
        if span.start != cursor {
            return Err(format!("gap: expected {cursor}, node starts at {}", span.start));
        }
        cursor = span.end;
        if let Node::Element(element) = node {
            // Children sit strictly inside the element; verify contiguity
            // among themselves (their exact start is after the open tag).
            if let (Some(first), Some(last)) = (element.children.first(), element.children.last()) {
                check_contiguous(&element.children, first.span().start, last.span().end)?;
                if first.span().start < span.start || last.span().end > span.end {
                    return Err("child span escapes element".to_string());
                }
            }
        }
    }
    if cursor != end {
        return Err(format!("tail gap: ended at {cursor}, expected {end}"));
    }
    Ok(())
}

fn count(nodes: &[Node]) -> (u64, u64) {
    let mut elements = 0;
    let mut directives = 0;
    for node in nodes {
        if let Node::Element(element) = node {
            elements += 1;
            directives +=
                element.attributes.iter().filter(|attr| attr.directive.is_some()).count() as u64;
            let (child_elements, child_directives) = count(&element.children);
            elements += child_elements;
            directives += child_directives;
        }
    }
    (elements, directives)
}

fn collect(dir: &PathBuf, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name != "node_modules" && name != "dist" && !name.starts_with('.') {
                collect(&path, files);
            }
        } else if name.ends_with(".vue") {
            files.push(path);
        }
    }
}
