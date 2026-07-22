use pi_tools::resolve_path;
use std::path::Path;

#[test]
fn resolves_relative_paths_against_authoritative_cwd() {
    let cwd = Path::new("/tmp/pi-cwd");
    assert_eq!(
        resolve_path("a/../b.txt", cwd).expect("resolved path"),
        Path::new("/tmp/pi-cwd/b.txt")
    );
    assert_eq!(
        resolve_path("/var/tmp/../log.txt", cwd).expect("absolute path"),
        Path::new("/var/log.txt")
    );
    assert_eq!(
        resolve_path("/../../safe.txt", cwd).expect("root-clamped path"),
        Path::new("/safe.txt")
    );
}

#[test]
fn rejects_empty_paths_and_relative_cwd() {
    assert!(resolve_path("", Path::new("/tmp")).is_err());
    assert!(resolve_path("file", Path::new("relative")).is_err());
}
