//! Repro: a mermaid diagram whose label contains `~~~` closes the `~~~` fence
//! `decor::mermaid::as_plain_fence` wraps the rendered grid in, and everything
//! after the diagram is swallowed into a code block.
//!
//! `as_plain_fence`'s doc comment asserts that a box-drawing grid "can never
//! contain" `~~~`. That holds for flowchart-family diagrams, whose labels sit
//! inside box borders — but `mermaid-text` renders `gantt`, `journey` and
//! `timeline` labels flush left with no border, so a tilde label reaches
//! column 0 unguarded. See `cargo run -p mermaid --example fence_escape_probe`
//! for which diagram kinds can do it.
//!
//! Run: `cargo run -p stele --example mermaid_leak_repro`

fn main() {
    let src = "# Doc\n\n\
               ```mermaid\n\
               gantt\n  title T\n  section ~~~~\n  ~~~~ :a1, 2024-01-01, 30d\n\
               ```\n\n\
               # After the diagram\n\n\
               This paragraph must survive.\n";

    let out = stele::decor::mermaid::preprocess(src).into_owned();
    println!("=== preprocessed source ===\n{out}=== end ===\n");

    println!("=== blocks the parser then sees ===");
    let doc = ast::Document::parse(&out);
    for block in doc.blocks() {
        let rendered = match &block.kind {
            ast::BlockKind::CodeBlock { literal, .. } => format!("CodeBlock {literal:?}"),
            other => format!("{other:?}"),
        };
        println!("  {}", &rendered[..rendered.len().min(160)]);
    }

    let headings = doc
        .blocks()
        .iter()
        .filter(|b| matches!(b.kind, ast::BlockKind::Heading { .. }))
        .count();
    println!("\nheadings parsed: {headings} (expected 2: `# Doc` and `# After the diagram`)");
    if headings < 2 {
        println!(
            "FENCE ESCAPE: the diagram's `~~~~` label closed the wrapper fence; \
             every block after the diagram was swallowed into a code block."
        );
    }
}
