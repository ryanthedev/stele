//! Conformance harness: vendored CommonMark 0.31.2 + GFM extension suites,
//! run through the AST→HTML shim (`ast::html`).
//!
//! DW-2.1: ≥649/652 CommonMark tests pass and every failure is documented
//! in `crates/ast/CONFORMANCE.md` (cap 3).
//! DW-2.2: the four GFM extension suites pass in full.

use serde::Deserialize;

#[derive(Deserialize)]
struct SpecCase {
    markdown: String,
    html: String,
    example: u32,
    section: String,
}

fn load(path: &str) -> Vec<SpecCase> {
    let full = format!("{}/tests/spec/{path}", env!("CARGO_MANIFEST_DIR"));
    let data = std::fs::read_to_string(&full)
        .unwrap_or_else(|e| panic!("missing vendored suite {full}: {e}"));
    serde_json::from_str(&data).expect("vendored suite parses")
}

fn run_case(case: &SpecCase, opts: &ast::ParseOptions) -> Result<(), String> {
    let doc = ast::Document::parse_with(&case.markdown, opts);
    let got = ast::html::to_html(&doc);
    if got == case.html { Ok(()) } else { Err(got) }
}

/// Failing example numbers documented in CONFORMANCE.md, as `example NNN`.
fn documented_deviations() -> Vec<u32> {
    let path = format!("{}/CONFORMANCE.md", env!("CARGO_MANIFEST_DIR"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("- example ") {
            let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = num.parse() {
                out.push(n);
            }
        }
    }
    out
}

#[test]
fn test_dw_2_1_commonmark_conformance() {
    let cases = load("commonmark-0.31.2.json");
    assert_eq!(cases.len(), 652, "vendored suite must have 652 examples");
    let failures: Vec<u32> = cases
        .iter()
        .filter(|c| run_case(c, &ast::ParseOptions::commonmark()).is_err())
        .map(|c| c.example)
        .collect();
    let passed = cases.len() - failures.len();
    let documented = documented_deviations();
    assert!(
        passed >= 649,
        "CommonMark conformance below the bar: {passed}/652; failing: {failures:?}"
    );
    assert!(
        documented.len() <= 3,
        "deviation ledger exceeds the cap of 3: {documented:?}"
    );
    for f in &failures {
        assert!(
            documented.contains(f),
            "example {f} fails but is not documented in CONFORMANCE.md (failing set: {failures:?})"
        );
    }
}

fn run_gfm_section(section: &str) {
    let cases = load("gfm-extensions.json");
    let mut failed = Vec::new();
    let mut total = 0;
    for case in cases.iter().filter(|c| c.section == section) {
        total += 1;
        if let Err(got) = run_case(case, &ast::ParseOptions::default()) {
            failed.push(format!(
                "example {} \nmarkdown: {:?}\nexpected: {:?}\ngot:      {:?}",
                case.example, case.markdown, case.html, got
            ));
        }
    }
    assert!(total > 0, "no cases found for section {section:?}");
    assert!(
        failed.is_empty(),
        "GFM {section} failures ({}/{total}):\n{}",
        failed.len(),
        failed.join("\n")
    );
}

#[test]
fn test_dw_2_2_gfm_tables() {
    run_gfm_section("Tables (extension)");
}

#[test]
fn test_dw_2_2_gfm_strikethrough() {
    run_gfm_section("Strikethrough (extension)");
}

#[test]
fn test_dw_2_2_gfm_task_lists() {
    run_gfm_section("Task list items (extension)");
}

#[test]
fn test_dw_2_2_gfm_autolinks() {
    run_gfm_section("Autolinks (extension)");
}

/// Dev loop helper: `cargo test -p ast --test conformance -- --ignored
/// dump_failures --nocapture` prints every failing CommonMark example.
#[test]
#[ignore]
fn dump_failures() {
    let cases = load("commonmark-0.31.2.json");
    let mut by_section: std::collections::BTreeMap<String, usize> = Default::default();
    let mut n_fail = 0;
    for case in &cases {
        if let Err(got) = run_case(case, &ast::ParseOptions::commonmark()) {
            n_fail += 1;
            *by_section.entry(case.section.clone()).or_default() += 1;
            if n_fail <= 40 {
                println!(
                    "=== example {} ({})\n--- markdown\n{:?}\n--- expected\n{:?}\n--- got\n{:?}\n",
                    case.example, case.section, case.markdown, case.html, got
                );
            }
        }
    }
    println!("TOTAL FAILURES: {n_fail}/652");
    for (s, n) in by_section {
        println!("  {s}: {n}");
    }
}
