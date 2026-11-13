use image::{ImageBuffer, RgbaImage};
use std::io::Cursor;

const MAX_IMAGE_PIXELS: usize = 40_000_000; // Guard against decompression bombs (~40 MP)

/// Encodes raw RGBA bytes into PNG format.
pub(crate) fn rgba_to_png(
    width: usize,
    height: usize,
    rgba: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    if width.saturating_mul(height) > MAX_IMAGE_PIXELS {
        return Err("Image dimensions exceed maximum safe limit".into());
    }
    let width_u32 = u32::try_from(width)?;
    let height_u32 = u32::try_from(height)?;

    let img: RgbaImage = ImageBuffer::from_raw(width_u32, height_u32, rgba.to_vec())
        .ok_or("Invalid image dimensions or byte length")?;

    let mut png_bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)?;
    Ok(png_bytes)
}

/// Decodes PNG bytes into (width, height, raw RGBA bytes).
pub(crate) fn png_to_rgba(
    png_bytes: &[u8],
) -> Result<(usize, usize, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
    let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)?;
    let width = usize::try_from(img.width())?;
    let height = usize::try_from(img.height())?;
    if width.saturating_mul(height) > MAX_IMAGE_PIXELS {
        return Err("Image dimensions exceed maximum safe limit".into());
    }
    let rgba = img.to_rgba8();
    Ok((width, height, rgba.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_png_roundtrip() {
        // Create 2x2 RGBA image (Red, Green, Blue, White)
        let width = 2;
        let height = 2;
        let raw_rgba: Vec<u8> = vec![
            255, 0, 0, 255, // Red
            0, 255, 0, 255, // Green
            0, 0, 255, 255, // Blue
            255, 255, 255, 255, // White
        ];

        let png = rgba_to_png(width, height, &raw_rgba).expect("Encoding PNG failed");
        assert_eq!(&png[0..4], &[137, 80, 78, 71]);

        let (decoded_w, decoded_h, decoded_rgba) = png_to_rgba(&png).expect("Decoding PNG failed");
        assert_eq!(decoded_w, width);
        assert_eq!(decoded_h, height);
        assert_eq!(decoded_rgba, raw_rgba);
    }

    #[test]
    fn test_oversized_image_rejected() {
        let err = rgba_to_png(10_000, 10_000, &[]).unwrap_err();
        assert!(err.to_string().contains("exceed maximum safe limit"));
    }
}
