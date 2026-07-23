mod support;

use base64::Engine;
use pi_cli::{RootCancellation, parse_args, prepare_prompts};
use pi_store::StorePaths;
use pi_tools::ImagePolicy;
use support::TempDir;

#[tokio::test]
async fn stdin_text_file_and_first_message_preserve_compatible_order() {
    let root = TempDir::new("input-order");
    let home = root.path().join("home");
    let cwd = root.path().join("project");
    std::fs::create_dir_all(&cwd).expect("project");
    std::fs::write(cwd.join("prompt.txt"), "file").expect("text fixture");
    let arguments = ["--mode", "text", "@prompt.txt", "message", "second"]
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let args = parse_args(&arguments).expect("arguments");
    let paths = StorePaths::new(home.join("agent"), &cwd, &home).expect("paths");
    let prompts = prepare_prompts(
        &args,
        &paths,
        Some("stdin"),
        ImagePolicy::default(),
        &RootCancellation::default(),
    )
    .await
    .expect("prompts");
    assert!(prompts[0].text.starts_with("stdin<file name="));
    assert!(prompts[0].text.ends_with("</file>\nmessage"));
    assert_eq!(prompts[1].text, "second");
}

#[tokio::test]
async fn image_file_becomes_a_processed_attachment() {
    let root = TempDir::new("image-input");
    let home = root.path().join("home");
    let cwd = root.path().join("project");
    std::fs::create_dir_all(&cwd).expect("project");
    let png = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
        .expect("PNG fixture");
    std::fs::write(cwd.join("pixel.png"), png).expect("image fixture");
    let arguments = ["--mode", "text", "@pixel.png", "inspect"]
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let args = parse_args(&arguments).expect("arguments");
    let paths = StorePaths::new(home.join("agent"), &cwd, &home).expect("paths");
    let prompts = prepare_prompts(
        &args,
        &paths,
        None,
        ImagePolicy::default(),
        &RootCancellation::default(),
    )
    .await
    .expect("prompts");
    assert_eq!(prompts[0].images.len(), 1);
    assert_eq!(prompts[0].images[0].mime_type, "image/png");
}
