//! Probe: does `highlight_line` really preserve every code line's text
//! exactly, across every fenced block in `testdocs/05-code-highlighting.md`?
//!
//! The unit test asserts this over six hand-written one-liners. lumis hands
//! back `(text, scope)` spans and this crate concatenates them; if a grammar
//! ever leaves a gap between spans, the missing bytes vanish from the painted
//! line with no other symptom. This walks every real fixture line instead.
//!
//! Also checks that every emitted `StyleId::Capture` id round-trips through
//! `Capture::from_id` (i.e. stays inside the allocated range and can never
//! be mistaken for another role).
//!
//! Run: `cargo run -p highlight --example preservation_probe`

use layout::StyleId;

fn main() {
    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("testdocs/05-code-highlighting.md"),
    )
    .unwrap();

    let mut lang: Option<String> = None;
    let mut checked = 0usize;
    let mut mismatches: Vec<(String, String, String)> = Vec::new();
    let mut ids = std::collections::BTreeSet::new();

    for line in doc.lines() {
        if let Some(rest) = line.strip_prefix("```") {
            lang = match lang {
                Some(_) => None,
                None => Some(rest.trim().to_string()),
            };
            continue;
        }
        let Some(tag) = lang.as_deref() else { continue };
        let tag = tag.split([',', ' ']).next().unwrap_or("");
        let runs = highlight::highlight_line(line, Some(tag).filter(|t| !t.is_empty()));
        let rebuilt: String = runs.iter().map(|r| r.text.as_str()).collect();
        checked += 1;
        if rebuilt != line {
            mismatches.push((tag.to_string(), line.to_string(), rebuilt));
        }
        for run in &runs {
            if let StyleId::Capture(id) = run.style_id {
                ids.insert(id);
            }
        }
    }

    println!("code lines checked : {checked}");
    println!("text mismatches    : {}", mismatches.len());
    for (tag, want, got) in mismatches.iter().take(10) {
        println!("  [{tag}]\n    want {want:?}\n    got  {got:?}");
    }
    println!(
        "distinct Capture ids emitted: {:?} (theme palette covers {} roles)",
        ids,
        highlight::role_count()
    );
    // An id outside the allocated range collapses to `Plain` on the way back,
    // which would silently merge two distinct roles onto one color.
    let collapses: Vec<u16> = ids
        .iter()
        .copied()
        .filter(|&id| {
            highlight::Capture::from_id(id) == highlight::Capture::Plain
                && id != highlight::Capture::Plain.id()
        })
        .collect();
    println!("Capture ids that silently collapse to Plain: {collapses:?}");

    // A 10k-line code block: every line highlighted independently. Bound the
    // total wall time so a per-line cost that is fine once is not fine 10,000
    // times over.
    let started = std::time::Instant::now();
    for i in 0..10_000 {
        let _ = highlight::highlight_line(&format!("let x{i}: i32 = {i};"), Some("rust"));
    }
    println!(
        "10k-line rust block: {:?} total ({:?}/line)",
        started.elapsed(),
        started.elapsed() / 10_000
    );
}
