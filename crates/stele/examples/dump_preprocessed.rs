//! Run one source preprocessor over a file and print what it produced.
//!
//! ```text
//! cargo run -p stele --example dump_preprocessed -- d2l path/to/doc.md
//! ```
//!
//! The question this answers is "did the pass leave anything behind", which
//! the goldens cannot: a fixture asserts that a shape stele already knows
//! about is handled, and the interesting failures are the shapes it does not
//! know about yet. The `%%tab` directive sitting behind a blank line in
//! `01-d2l-linear-regression.md` was one of those — eleven markers stripped,
//! the twelfth left on screen, and nothing in the suite to say so. Piping a
//! real document through a pass and grepping the output for the marker it was
//! supposed to remove is how that gets caught.
//!
//! **One pass at a time, named on the command line, deliberately.** The order
//! the loader chains them in is `loader::preprocess`'s business and is not
//! public; re-implementing that order here would put a second copy of it in
//! the tree, where it would drift silently and be wrong exactly when someone
//! trusted this tool to reproduce a loader bug.

use stele::decor::{codecogs, d2l, quarto};

const PASSES: [&str; 3] = ["d2l", "quarto", "codecogs"];

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(pass), Some(path)) = (args.next(), args.next()) else {
        eprintln!("usage: dump_preprocessed <{}> <file>", PASSES.join("|"));
        std::process::exit(2);
    };

    let source = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("dump_preprocessed: {path}: {err}");
            std::process::exit(1);
        }
    };

    let out = match pass.as_str() {
        "d2l" => d2l::apply(&source),
        "quarto" => quarto::apply(&source),
        "codecogs" => codecogs::apply(&source),
        other => {
            eprintln!(
                "dump_preprocessed: no pass named {other:?} — one of {}",
                PASSES.join(", ")
            );
            std::process::exit(2);
        }
    };
    print!("{out}");
}
