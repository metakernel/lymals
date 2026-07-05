use std::process::Command;

use tempfile::tempdir;

#[test]
fn stdio_with_log_file_keeps_stdout_clean_and_creates_log() {
    let temp = tempdir().unwrap();
    let log_file = temp.path().join("lumals.log");

    let output = Command::new(env!("CARGO_BIN_EXE_lumals"))
        .arg("--stdio")
        .arg("--log-file")
        .arg(&log_file)
        .output()
        .expect("lumals should run");

    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(log_file.exists());
}
