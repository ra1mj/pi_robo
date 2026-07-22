mod support;

use base64::Engine;
use image::{DynamicImage, ImageFormat};
use pi_agent::Tool;
use pi_protocol::ContentBlock;
use pi_test_support::FakeCancellation;
use pi_tools::{ImagePolicy, ReadTool, detect_supported_image_mime_type};
use serde_json::json;
use std::io::Cursor;
use support::{RecordingUpdates, TempRoot, call, output_text};

fn encoded_image(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
    let image = DynamicImage::new_rgb8(width, height);
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, format)
        .expect("encode synthetic image");
    output.into_inner()
}

#[tokio::test]
async fn detects_and_round_trips_supported_image_formats() {
    let root = TempRoot::new("image-formats");
    let cases = [
        (ImageFormat::Jpeg, "image/jpeg"),
        (ImageFormat::Png, "image/png"),
        (ImageFormat::Gif, "image/gif"),
        (ImageFormat::WebP, "image/webp"),
        (ImageFormat::Bmp, "image/bmp"),
    ];
    let tool = ReadTool::new(root.path(), ImagePolicy::default());
    for (index, (format, mime_type)) in cases.into_iter().enumerate() {
        let bytes = encoded_image(format, 2, 2);
        assert_eq!(detect_supported_image_mime_type(&bytes), Some(mime_type));
        let file = format!("image-{index}.data");
        std::fs::write(root.path().join(&file), bytes).expect("write image fixture");
        let output = tool
            .execute(
                &call("read", json!({ "path": file })),
                &FakeCancellation::default(),
                &RecordingUpdates::default(),
            )
            .await
            .expect("read image");
        let image = output.content.iter().find_map(|block| match block {
            ContentBlock::Image(image) => Some(image),
            _ => None,
        });
        let image = image.expect("canonical image block");
        if mime_type == "image/bmp" {
            assert_eq!(image.mime_type, "image/png");
        } else {
            assert_eq!(image.mime_type, mime_type);
        }
        assert!(
            !base64::engine::general_purpose::STANDARD
                .decode(&image.data)
                .expect("base64 image")
                .is_empty()
        );
    }
}

#[tokio::test]
async fn resizes_images_and_can_block_image_content() {
    let root = TempRoot::new("image-policy");
    std::fs::write(
        root.path().join("large.png"),
        encoded_image(ImageFormat::Png, 8, 4),
    )
    .expect("write image");
    let policy = ImagePolicy {
        max_width: 2,
        max_height: 2,
        ..ImagePolicy::default()
    };
    let tool = ReadTool::new(root.path(), policy);
    let output = tool
        .execute(
            &call("read", json!({ "path": "large.png" })),
            &FakeCancellation::default(),
            &RecordingUpdates::default(),
        )
        .await
        .expect("resized image");
    assert!(output_text(&output).contains("original 8x4"));

    let blocked = ReadTool::new(
        root.path(),
        ImagePolicy {
            block_images: true,
            ..ImagePolicy::default()
        },
    )
    .execute(
        &call("read", json!({ "path": "large.png" })),
        &FakeCancellation::default(),
        &RecordingUpdates::default(),
    )
    .await
    .expect("blocked image");
    assert!(
        blocked
            .content
            .iter()
            .all(|block| !matches!(block, ContentBlock::Image(_)))
    );
    assert!(output_text(&blocked).contains("image output is disabled"));
}

#[tokio::test]
async fn malformed_detected_image_is_omitted_without_panicking() {
    let root = TempRoot::new("image-malformed");
    let mut malformed = b"\x89PNG\r\n\x1a\n".to_vec();
    malformed.extend_from_slice(&13_u32.to_be_bytes());
    malformed.extend_from_slice(b"IHDR");
    std::fs::write(root.path().join("broken.png"), malformed).expect("write malformed image");
    let output = ReadTool::new(root.path(), ImagePolicy::default())
        .execute(
            &call("read", json!({ "path": "broken.png" })),
            &FakeCancellation::default(),
            &RecordingUpdates::default(),
        )
        .await
        .expect("malformed image result");
    assert!(output_text(&output).contains("Image omitted"));
    assert!(
        output
            .content
            .iter()
            .all(|block| !matches!(block, ContentBlock::Image(_)))
    );
}

#[tokio::test]
async fn decoded_pixel_limit_rejects_oversized_images_before_full_processing() {
    let root = TempRoot::new("image-pixel-limit");
    std::fs::write(
        root.path().join("bounded.png"),
        encoded_image(ImageFormat::Png, 2, 2),
    )
    .expect("write bounded image");
    let output = ReadTool::new(
        root.path(),
        ImagePolicy {
            max_decoded_pixels: 1,
            ..ImagePolicy::default()
        },
    )
    .execute(
        &call("read", json!({ "path": "bounded.png" })),
        &FakeCancellation::default(),
        &RecordingUpdates::default(),
    )
    .await
    .expect("bounded image result");
    assert!(output_text(&output).contains("decoded-pixel limit"));
    assert!(
        output
            .content
            .iter()
            .all(|block| !matches!(block, ContentBlock::Image(_)))
    );
}
