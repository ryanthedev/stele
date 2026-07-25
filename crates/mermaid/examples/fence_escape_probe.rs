//! Probe: can a mermaid diagram's own (author-controlled) label text produce a
//! rendered grid line that closes the `~~~` fence
//! `stele::decor::mermaid::as_plain_fence` wraps it in?
//!
//! That wrapper's doc comment asserts "a box-drawing grid can never contain"
//! `~~~`. Labels are arbitrary text from the document, so that claim is worth
//! checking rather than trusting.
//!
//! Run: `cargo run -p mermaid --example fence_escape_probe`

/// CommonMark closes a `~~~` fence on a line of >= 3 tildes, indented at most
/// 3 spaces, with nothing but whitespace after it.
fn closes_a_tilde_fence(line: &str) -> bool {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return false;
    }
    let rest = line.trim_start_matches(' ');
    let tildes = rest.chars().take_while(|&c| c == '~').count();
    tildes >= 3 && rest[tildes..].trim().is_empty()
}

fn main() {
    let cases: &[(&str, &str)] = &[
        (
            "flowchart tilde label",
            "graph LR\n  A[~~~~~~~~] --> B[x]\n",
        ),
        ("flowchart tilde only", "graph TD\n  A[~~~]\n"),
        (
            "flowchart long tilde node",
            "graph TD\n  A[~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~]\n",
        ),
        ("sequence tilde msg", "sequenceDiagram\n  A->>B: ~~~~~~~~\n"),
        (
            "pie tilde",
            "pie title ~~~~~~\n  \"~~~~\" : 10\n  \"b\" : 5\n",
        ),
        (
            "journey tilde",
            "journey\n  title ~~~~~~\n  section ~~~~\n    ~~~~: 5: Me\n",
        ),
        (
            "gantt tilde",
            "gantt\n  title ~~~~~~\n  section ~~~~\n  ~~~~ :a1, 2024-01-01, 30d\n",
        ),
        ("state tilde", "stateDiagram-v2\n  [*] --> ~~~~\n"),
        (
            "class tilde",
            "classDiagram\n  class Foo {\n    +~~~~~~~~ bar\n  }\n",
        ),
        ("mindmap tilde", "mindmap\n  root((~~~~))\n    ~~~~~~~~\n"),
        (
            "timeline tilde",
            "timeline\n  title ~~~~~~\n  ~~~~ : ~~~~~~~~\n",
        ),
        ("er tilde", "erDiagram\n  A ||--o{ B : \"~~~~~~~~\"\n"),
    ];

    let mut escapes = 0;
    for (name, src) in cases {
        match mermaid::render(src) {
            Ok(grid) => {
                let bad: Vec<&str> = grid.lines().filter(|l| closes_a_tilde_fence(l)).collect();
                if bad.is_empty() {
                    println!("{name:<26} ok   ({} lines)", grid.lines().count());
                } else {
                    escapes += 1;
                    println!(
                        "{name:<26} FENCE ESCAPE -> {} line(s), first: {:?}",
                        bad.len(),
                        bad[0]
                    );
                }
            }
            Err(e) => println!("{name:<26} render err (falls back to plain fence): {e}"),
        }
    }
    println!("\nfence escapes: {escapes}");
}
