//! Bug-bash probe: segment a string and print per-cluster widths.
//! Run: cargo run -p width --example probe -- <escaped-string>
//! Input accepts \uXXXX and \UXXXXXXXX escapes.
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '\\' && i + 1 < b.len() && (b[i + 1] == 'u' || b[i + 1] == 'U') {
            let n = if b[i + 1] == 'u' { 4 } else { 8 };
            let hex: String = b[i + 2..(i + 2 + n).min(b.len())].iter().collect();
            if let Ok(cp) = u32::from_str_radix(&hex, 16)
                && let Some(c) = char::from_u32(cp)
            {
                out.push(c);
                i += 2 + n;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn main() {
    let engine = width::WidthEngine::new(width::WidthConfig::default());
    let wide = width::WidthEngine::new(width::WidthConfig {
        ambiguous_wide: true,
    });
    for arg in std::env::args().skip(1) {
        let s = unescape(&arg);
        println!("input {arg:?}");
        for g in width::graphemes(&s) {
            let cps: Vec<String> = g.chars().map(|c| format!("U+{:04X}", c as u32)).collect();
            println!(
                "  cluster {:?} [{}] narrow={} wide={}",
                g,
                cps.join(" "),
                engine.cluster_width(g),
                wide.cluster_width(g)
            );
        }
        println!(
            "  display_width narrow={} wide={}",
            engine.display_width(&s),
            wide.display_width(&s)
        );
    }
}
