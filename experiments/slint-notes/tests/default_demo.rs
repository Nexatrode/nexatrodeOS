#![cfg(not(any(feature = "workspace-ui", feature = "full-suite-ui")))]

use std::{fs, process::Command};

#[test]
fn default_demo_renders_and_handles_input() {
    let output_path = std::env::temp_dir().join(format!("slint-default-demo-{}.ppm", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_personal-os-slint-notes-spike"))
        .args(["300", "400"])
        .arg(&output_path)
        .args(["--exercise-input", "--compare-line"])
        .output()
        .expect("run the default Notes demo");
    assert!(output.status.success(), "demo failed: {}", String::from_utf8_lossy(&output.stderr));
    let image = fs::read(&output_path).expect("read the rendered image");
    fs::remove_file(&output_path).expect("remove the rendered image");
    assert!(image.starts_with(b"P6\n300 400\n255\n"));
    assert_eq!(image.len(), b"P6\n300 400\n255\n".len() + 300 * 400 * 3);
}
