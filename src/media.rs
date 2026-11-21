use image::{ImageBuffer, RgbaImage};
use std::io::Cursor;

const MAX_IMAGE_PIXELS: usize = 40_000_000; // Guard against decompression bombs (~40 MP)

/// Encodes raw RGBA bytes into PNG format.
///
/// # Errors
/// Returns an error if the dimensions or buffer length are invalid, the image
/// exceeds the allocation guard, or PNG encoding fails.
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

/// Decodes PNG bytes into (width, height, raw RGBA bytes), validating header dimensions
/// and applying decoder memory limits *before* allocating the pixel buffer.
///
/// # Errors
/// Returns an error if the PNG header is invalid, the image exceeds the allocation
/// guard, or decoding to RGBA fails.
pub(crate) fn png_to_rgba(
    png_bytes: &[u8],
) -> Result<(usize, usize, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
    let (header_w, header_h) =
        image::ImageReader::with_format(Cursor::new(png_bytes), image::ImageFormat::Png)
            .into_dimensions()?;
    let width = usize::try_from(header_w)?;
    let height = usize::try_from(header_h)?;
    if width == 0 || height == 0 || width.saturating_mul(height) > MAX_IMAGE_PIXELS {
        return Err("Image dimensions exceed maximum safe limit".into());
    }

    let mut reader =
        image::ImageReader::with_format(Cursor::new(png_bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(
        u64::try_from(MAX_IMAGE_PIXELS)
            .unwrap_or(40_000_000)
            .saturating_mul(4),
    );
    reader.limits(limits);

    let img = reader.decode()?;
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

    #[test]
    fn test_png_decode_rejects_oversized_header_before_allocation() {
        // Minimal PNG signature + IHDR chunk declaring 10_000 x 10_000 (100 MP > 40 MP limit)
        // with no IDAT pixel data; must be rejected from header dimensions alone.
        let mut crafted = vec![
            137, 80, 78, 71, 13, 10, 26, 10, // PNG signature
            0, 0, 0, 13, // IHDR length = 13
            b'I', b'H', b'D', b'R',
        ];
        crafted.extend_from_slice(&10_000_u32.to_be_bytes());
        crafted.extend_from_slice(&10_000_u32.to_be_bytes());
        crafted.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
        crafted.extend_from_slice(&0xba4e_6227_u32.to_be_bytes()); // valid IHDR CRC-32
        crafted.extend_from_slice(&[0, 0, 0, 0, b'I', b'D', b'A', b'T', 0x35, 0xaf, 0x06, 0x1e]);
        crafted.extend_from_slice(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]);

        let err = png_to_rgba(&crafted).unwrap_err();
        assert!(
            err.to_string().contains("exceed maximum safe limit"),
            "Unexpected error: {err}"
        );
    }
}
