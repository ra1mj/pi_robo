use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use pi_agent::{Cancellation, ToolError};
use std::io::Cursor;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Clone, Debug)]
pub struct ImagePolicy {
    pub block_images: bool,
    pub auto_resize: bool,
    pub max_width: u32,
    pub max_height: u32,
    pub max_base64_bytes: usize,
    pub max_decoded_pixels: u64,
    pub jpeg_quality: u8,
}

impl Default for ImagePolicy {
    fn default() -> Self {
        Self {
            block_images: false,
            auto_resize: true,
            max_width: 2_000,
            max_height: 2_000,
            max_base64_bytes: 4_718_592,
            max_decoded_pixels: 100_000_000,
            jpeg_quality: 80,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessedImage {
    pub data: String,
    pub mime_type: String,
    pub hint: Option<String>,
}

pub fn detect_supported_image_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return (bytes.get(3) != Some(&0xf7)).then_some("image/jpeg");
    }
    if bytes.starts_with(PNG_SIGNATURE) {
        return (is_png(bytes) && !is_animated_png(bytes)).then_some("image/png");
    }
    if bytes.starts_with(b"GIF") {
        return Some("image/gif");
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    if bytes.starts_with(b"BM") && is_bmp(bytes) {
        return Some("image/bmp");
    }
    None
}

pub(crate) async fn process_image(
    bytes: Vec<u8>,
    mime_type: &str,
    policy: ImagePolicy,
    cancellation: &dyn Cancellation,
) -> Result<Option<ProcessedImage>, ToolError> {
    if cancellation.is_cancelled() {
        return Err(ToolError::cancelled());
    }
    let mime_type = mime_type.to_owned();
    let result =
        tokio::task::spawn_blocking(move || process_image_blocking(bytes, &mime_type, &policy))
            .await
            .map_err(|error| ToolError::execution(format!("image worker failed: {error}")))?;
    if cancellation.is_cancelled() {
        return Err(ToolError::cancelled());
    }
    result.map_err(ToolError::execution)
}

fn process_image_blocking(
    bytes: Vec<u8>,
    mime_type: &str,
    policy: &ImagePolicy,
) -> Result<Option<ProcessedImage>, String> {
    if !policy.auto_resize && mime_type != "image/bmp" {
        return Ok(Some(ProcessedImage {
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
            mime_type: mime_type.to_owned(),
            hint: None,
        }));
    }

    let format = image_format(mime_type).ok_or_else(|| "unsupported image format".to_owned())?;
    let (original_width, original_height) =
        ImageReader::with_format(Cursor::new(bytes.as_slice()), format)
            .into_dimensions()
            .map_err(|error| format!("could not read image dimensions: {error}"))?;
    let pixels = u64::from(original_width).saturating_mul(u64::from(original_height));
    if pixels > policy.max_decoded_pixels {
        return Err(format!(
            "image dimensions {original_width}x{original_height} exceed the decoded-pixel limit"
        ));
    }
    let dimension_limit = u32::try_from(policy.max_decoded_pixels).unwrap_or(u32::MAX);
    let mut limits = Limits::default();
    limits.max_image_width = Some(dimension_limit);
    limits.max_image_height = Some(dimension_limit);
    limits.max_alloc = Some(policy.max_decoded_pixels.saturating_mul(8));
    let mut reader = ImageReader::with_format(Cursor::new(bytes.as_slice()), format);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| format!("could not decode image within memory limits: {error}"))?;

    let encoded_size = bytes.len().div_ceil(3).saturating_mul(4);
    if mime_type != "image/bmp"
        && original_width <= policy.max_width
        && original_height <= policy.max_height
        && encoded_size < policy.max_base64_bytes
    {
        return Ok(Some(ProcessedImage {
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
            mime_type: mime_type.to_owned(),
            hint: None,
        }));
    }

    let (mut width, mut height) = bounded_dimensions(
        original_width,
        original_height,
        policy.max_width,
        policy.max_height,
    );
    loop {
        let resized = if width == original_width && height == original_height {
            image.clone()
        } else {
            image.resize_exact(width, height, FilterType::Lanczos3)
        };
        for (candidate, candidate_mime) in encode_candidates(&resized, policy.jpeg_quality)? {
            let base64_size = candidate.len().div_ceil(3).saturating_mul(4);
            if base64_size < policy.max_base64_bytes {
                let changed = mime_type == "image/bmp"
                    || width != original_width
                    || height != original_height
                    || candidate_mime != mime_type;
                let hint = changed.then(|| {
                    format!(
                        "[Image: original {original_width}x{original_height}, displayed at {width}x{height}.]"
                    )
                });
                return Ok(Some(ProcessedImage {
                    data: base64::engine::general_purpose::STANDARD.encode(candidate),
                    mime_type: candidate_mime.to_owned(),
                    hint,
                }));
            }
        }
        if width == 1 && height == 1 {
            return Ok(None);
        }
        width = if width == 1 {
            1
        } else {
            (width.saturating_mul(3) / 4).max(1)
        };
        height = if height == 1 {
            1
        } else {
            (height.saturating_mul(3) / 4).max(1)
        };
    }
}

fn bounded_dimensions(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let mut width = width;
    let mut height = height;
    if width > max_width {
        height = ((u64::from(height) * u64::from(max_width)) / u64::from(width))
            .try_into()
            .unwrap_or(1);
        width = max_width;
    }
    if height > max_height {
        width = ((u64::from(width) * u64::from(max_height)) / u64::from(height))
            .try_into()
            .unwrap_or(1);
        height = max_height;
    }
    (width.max(1), height.max(1))
}

fn encode_candidates(
    image: &DynamicImage,
    preferred_quality: u8,
) -> Result<Vec<(Vec<u8>, &'static str)>, String> {
    let mut candidates = Vec::new();
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| format!("could not encode PNG: {error}"))?;
    candidates.push((png.into_inner(), "image/png"));

    let mut qualities = vec![preferred_quality, 85, 70, 55, 40];
    qualities.dedup();
    for quality in qualities {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, quality)
            .encode_image(image)
            .map_err(|error| format!("could not encode JPEG: {error}"))?;
        candidates.push((jpeg, "image/jpeg"));
    }
    Ok(candidates)
}

fn image_format(mime_type: &str) -> Option<ImageFormat> {
    match mime_type {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::WebP),
        "image/bmp" => Some(ImageFormat::Bmp),
        _ => None,
    }
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 16
        && read_u32_be(bytes, PNG_SIGNATURE.len()) == 13
        && bytes.get(12..16) == Some(b"IHDR")
}

fn is_animated_png(bytes: &[u8]) -> bool {
    let mut offset = PNG_SIGNATURE.len();
    while offset + 8 <= bytes.len() {
        let length = usize::try_from(read_u32_be(bytes, offset)).unwrap_or(usize::MAX);
        let chunk_type = offset + 4;
        if bytes.get(chunk_type..chunk_type + 4) == Some(b"acTL") {
            return true;
        }
        if bytes.get(chunk_type..chunk_type + 4) == Some(b"IDAT") {
            return false;
        }
        let Some(next) = offset
            .checked_add(12)
            .and_then(|value| value.checked_add(length))
        else {
            return false;
        };
        if next <= offset || next > bytes.len() {
            return false;
        }
        offset = next;
    }
    false
}

fn is_bmp(bytes: &[u8]) -> bool {
    if bytes.len() < 26 {
        return false;
    }
    let declared_size = read_u32_le(bytes, 2);
    let pixel_offset = read_u32_le(bytes, 10);
    let dib_size = read_u32_le(bytes, 14);
    if declared_size != 0 && declared_size < 26 {
        return false;
    }
    if pixel_offset < 14_u32.saturating_add(dib_size) {
        return false;
    }
    if declared_size != 0 && pixel_offset >= declared_size {
        return false;
    }
    let (planes, bits) = if dib_size == 12 {
        (read_u16_le(bytes, 22), read_u16_le(bytes, 24))
    } else if (40..=124).contains(&dib_size) && bytes.len() >= 30 {
        (read_u16_le(bytes, 26), read_u16_le(bytes, 28))
    } else {
        return false;
    };
    planes == 1 && matches!(bits, 1 | 4 | 8 | 16 | 24 | 32)
}

fn read_u16_le(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        bytes.get(offset).copied().unwrap_or(0),
        bytes.get(offset + 1).copied().unwrap_or(0),
    ])
}

fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes.get(offset).copied().unwrap_or(0),
        bytes.get(offset + 1).copied().unwrap_or(0),
        bytes.get(offset + 2).copied().unwrap_or(0),
        bytes.get(offset + 3).copied().unwrap_or(0),
    ])
}

fn read_u32_be(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        bytes.get(offset).copied().unwrap_or(0),
        bytes.get(offset + 1).copied().unwrap_or(0),
        bytes.get(offset + 2).copied().unwrap_or(0),
        bytes.get(offset + 3).copied().unwrap_or(0),
    ])
}
