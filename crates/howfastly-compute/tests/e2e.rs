use std::process::Command;

#[test]
fn viceroy() {
    let status = Command::new("nu")
        .arg("tests/e2e.nu")
        .status()
        .expect("nu not found, run inside the dev shell");
    assert!(status.success());
}
