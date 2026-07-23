use std::process::Command;

#[test]
fn production_binary_help_and_version_need_no_node_or_bun() {
    let binary = env!("CARGO_BIN_EXE_pi-rs");
    for option in ["--help", "--version"] {
        let output = Command::new(binary)
            .arg(option)
            .env("PATH", "")
            .output()
            .expect("pi-rs must execute");
        assert!(output.status.success(), "{option}");
        assert!(!output.stdout.is_empty(), "{option}");
        assert!(output.stderr.is_empty(), "{option}");
    }
}
