//! Not assertions — a measurement, kept because the numbers in
//! `svg::fonts`' doc comment are load-bearing and will go stale.
//!
//! `svg` skips the system font database entirely for a drawing with no
//! `<text>`, and the justification for that complexity is the gap between the
//! two cold numbers below. Re-run after bumping `resvg` or changing the font
//! strategy:
//!
//! ```text
//! cargo test -p gfx --test svg_cost -- --ignored --nocapture
//! ```
use std::time::Instant;

#[test]
#[ignore = "a benchmark, not a check — run it by name"]
fn measure_svg_render_cost() {
    let textless = r##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200">
        <rect width="400" height="200" fill="#222"/><circle cx="200" cy="100" r="60" fill="#fa0"/>
    </svg>"##;
    let labelled = r##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200">
        <rect width="400" height="200" fill="#222"/>
        <text x="20" y="100" font-family="Helvetica" font-size="28" fill="#eee">Hello, diagram</text>
    </svg>"##;

    let plain = std::env::temp_dir().join("stele-svg-cost-textless.svg");
    let texty = std::env::temp_dir().join("stele-svg-cost-text.svg");
    std::fs::write(&plain, textless).unwrap();
    std::fs::write(&texty, labelled).unwrap();

    let t = Instant::now();
    gfx::decode::decode_and_scale(&plain, (200, 100), Default::default()).unwrap();
    println!("no text, cold:            {:?}", t.elapsed());

    let t = Instant::now();
    gfx::decode::decode_and_scale(&texty, (200, 100), Default::default()).unwrap();
    println!("with text, cold (fonts):  {:?}", t.elapsed());

    let t = Instant::now();
    for _ in 0..20 {
        gfx::decode::decode_and_scale(&texty, (200, 100), Default::default()).unwrap();
    }
    println!("with text, warm:          {:?}", t.elapsed() / 20);

    let _ = std::fs::remove_file(&plain);
    let _ = std::fs::remove_file(&texty);
}
