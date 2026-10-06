//! CF_DIB is a BMP without its file header, not necessarily a CF_DIBV5.
use image::{DynamicImage, ImageDecoder};
use std::io::Cursor;

pub(super) fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("剪贴板位图超过 64 MiB；请以原文件发送".into());
    }
    let decoder = image::codecs::bmp::BmpDecoder::new_without_file_header(Cursor::new(bytes))
        .map_err(|e| format!("剪贴板 CF_DIB 位图无法读取：{e}；请保存为文件后发送"))?;
    let (width, height) = decoder.dimensions();
    // Check before allocating pixels, including pathological top-down heights.
    if width == 0
        || height == 0
        || width > 32768
        || height > 32768
        || u64::from(width) * u64::from(height) > 16_000_000
    {
        return Err("剪贴板图片超过 1600 万像素；请以原文件发送".into());
    }
    // Keep RGB screenshots as RGB through PNG encoding, avoiding a second
    // full-size RGBA allocation solely to add an opaque alpha channel.
    DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("剪贴板 CF_DIB 位图无法解码：{e}；请保存为文件后发送"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(width: i32, height: i32, bits: u16, compression: u32, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 40];
        bytes[0..4].copy_from_slice(&40_u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&width.to_le_bytes());
        bytes[8..12].copy_from_slice(&height.to_le_bytes());
        bytes[12..14].copy_from_slice(&1_u16.to_le_bytes());
        bytes[14..16].copy_from_slice(&bits.to_le_bytes());
        bytes[16..20].copy_from_slice(&compression.to_le_bytes());
        bytes.extend_from_slice(pixels);
        bytes
    }

    #[test]
    fn screenshot_dib_handles_padding_orientation_and_unused_alpha() {
        // Bottom-up BGR24, with one byte of padding on each row.
        let image = decode(&bitmap(1, 2, 24, 0, &[255, 0, 0, 0, 0, 0, 255, 0])).unwrap();
        assert_eq!((image.width(), image.height()), (1, 2));
        assert_eq!(
            image.into_rgba8().as_raw(),
            &[255, 0, 0, 255, 0, 0, 255, 255]
        );
        // Top-down BGRX32: unused zero bytes must not make a screenshot invisible.
        let image = decode(&bitmap(1, -2, 32, 0, &[0, 0, 255, 0, 255, 0, 0, 0])).unwrap();
        assert_eq!(
            image.into_rgba8().as_raw(),
            &[255, 0, 0, 255, 0, 0, 255, 255]
        );
    }

    #[test]
    fn dib_reads_palette_and_rgb565_masks() {
        let mut palette = bitmap(1, 1, 8, 0, &[0, 0, 255, 0, 0, 255, 0, 0, 1, 0, 0, 0]);
        palette[32..36].copy_from_slice(&2_u32.to_le_bytes());
        assert_eq!(
            decode(&palette).unwrap().into_rgba8().as_raw(),
            &[0, 255, 0, 255]
        );
        let masks: Vec<u8> = [0xf800_u32, 0x07e0, 0x001f]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .chain([0, 0xf8, 0, 0])
            .collect();
        assert_eq!(
            decode(&bitmap(1, 1, 16, 3, &masks))
                .unwrap()
                .into_rgba8()
                .as_raw(),
            &[255, 0, 0, 255]
        );
    }

    #[test]
    fn screenshot_rgb_stages_as_lossless_png_without_an_rgba_copy() {
        let image = decode(&bitmap(1, -2, 32, 0, &[0, 0, 255, 0, 255, 0, 0, 0])).unwrap();
        assert_eq!(image.color(), image::ColorType::Rgb8);
        let staged = super::super::stage_pixels(
            image.width() as usize,
            image.height() as usize,
            image.as_bytes(),
            image.color(),
        )
        .unwrap();
        assert_eq!(
            image::open(&staged.path).unwrap().into_rgba8().as_raw(),
            &[255, 0, 0, 255, 0, 0, 255, 255]
        );
    }

    #[test]
    fn dib_rejects_truncated_and_oversized_images_before_pixel_allocation() {
        assert!(decode(&[0; 8]).is_err());
        assert!(decode(&bitmap(1, 1, 24, 0, &[])).is_err());
        assert!(decode(&bitmap(0, 1, 24, 0, &[])).is_err());
        assert!(decode(&bitmap(1, i32::MIN, 32, 0, &[])).is_err());
        assert!(
            decode(&bitmap(8000, 8000, 32, 0, &[]))
                .unwrap_err()
                .contains("1600")
        );
    }
}
