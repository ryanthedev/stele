//! Black-box tests over the real `stele` binary's error paths (DW-5.4).
//! File loading happens *before* any terminal setup in `main.rs`, so these
//! error paths are reachable from a plain subprocess with no controlling
//! terminal — no PTY needed.

use std::process::Command;

fn stele_bin() -> &'static str {
    env!("CARGO_BIN_EXE_stele")
}

#[test]
fn test_dw_5_4_missing_file_exits_nonzero_with_clean_stderr() {
    let output = Command::new(stele_bin())
        .arg("/nonexistent/does-not-exist.md")
        .output()
        .expect("failed to run stele binary");

    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("stele:"), "stderr was: {stderr}");
    assert!(stderr.contains("could not read file"));
}

#[test]
fn test_dw_5_4_invalid_utf8_exits_nonzero_with_clean_stderr() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("stele-cli-invalid-utf8-{}.md", std::process::id()));
    std::fs::write(&path, [0x48, 0x65, 0x6c, 0x6c, 0x6f, 0xff, 0xfe]).unwrap();

    let output = Command::new(stele_bin())
        .arg(&path)
        .output()
        .expect("failed to run stele binary");
    std::fs::remove_file(&path).ok();

    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("stele:"), "stderr was: {stderr}");
    assert!(stderr.contains("not valid UTF-8"));
}
