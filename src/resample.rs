//! Triangle resampling with one floating-point row, rather than a full canvas.
//! Borrowed image views also avoid copying a zoom crop before resizing it.
use image::{DynamicImage, GenericImageView, Pixel, Rgba, RgbaImage};

pub fn fit(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let ratio = (f64::from(max_width) / f64::from(width.max(1)))
        .min(f64::from(max_height) / f64::from(height.max(1)));
    (
        (f64::from(width) * ratio).round().max(1.0) as u32,
        (f64::from(height) * ratio).round().max(1.0) as u32,
    )
}

struct Sample {
    first: u32,
    weights: Vec<f32>,
}

fn sample(source: u32, target: u32, position: u32) -> Sample {
    let ratio = source as f32 / target as f32;
    let support = ratio.max(1.0);
    let center = (position as f32 + 0.5) * ratio;
    let first = ((center - support).floor() as i64).clamp(0, i64::from(source) - 1) as u32;
    let end =
        ((center + support).ceil() as i64).clamp(i64::from(first) + 1, i64::from(source)) as u32;
    let mut weights: Vec<_> = (first..end)
        .map(|index| (1.0 - ((index as f32 - (center - 0.5)) / support).abs()).max(0.0))
        .collect();
    let total: f32 = weights.iter().sum();
    for weight in &mut weights {
        *weight /= total;
    }
    Sample { first, weights }
}

pub fn resize<I: GenericImageView>(source: &I, width: u32, height: u32) -> RgbaImage
where
    I::Pixel: Pixel<Subpixel = u8>,
{
    resize_into(source, width, height, width, height)
}

pub fn crop_resize(
    source: &DynamicImage,
    region: (u32, u32, u32, u32),
    width: u32,
    height: u32,
) -> RgbaImage {
    let (x, y, w, h) = region;
    // Dispatch once for common formats instead of matching DynamicImage on
    // every source pixel. RGB inputs need no whole-image RGBA conversion.
    macro_rules! from_buffer {
        ($buffer:expr) => {
            resize(&*$buffer.view(x, y, w, h), width, height)
        };
    }
    match source {
        DynamicImage::ImageRgb8(buffer) => from_buffer!(buffer),
        DynamicImage::ImageRgba8(buffer) => from_buffer!(buffer),
        DynamicImage::ImageLuma8(buffer) => from_buffer!(buffer),
        DynamicImage::ImageLumaA8(buffer) => from_buffer!(buffer),
        _ => from_buffer!(source),
    }
}

/// Preserve aspect ratio, padding the bottom/right of a terminal cell canvas.
pub fn canvas(
    source: &impl GenericImageView<Pixel = Rgba<u8>>,
    width: u32,
    height: u32,
) -> RgbaImage {
    let (inner_width, inner_height) = fit(source.width(), source.height(), width, height);
    resize_into(source, inner_width, inner_height, width, height)
}

fn resize_into<I: GenericImageView>(
    source: &I,
    width: u32,
    height: u32,
    canvas_width: u32,
    canvas_height: u32,
) -> RgbaImage
where
    I::Pixel: Pixel<Subpixel = u8>,
{
    let mut output = RgbaImage::new(canvas_width, canvas_height);
    if source.width() == 0 || source.height() == 0 || width == 0 || height == 0 {
        return output;
    }
    if (width, height) == source.dimensions() {
        for y in 0..height {
            for x in 0..width {
                output.put_pixel(x, y, source.get_pixel(x, y).to_rgba());
            }
        }
        return output;
    }
    // Precompute horizontal weights once. Only one source-width float row lives
    // alongside the final RGBA8 canvas; no source-width x target-height scratch.
    let horizontal: Vec<_> = (0..width)
        .map(|x| sample(source.width(), width, x))
        .collect();
    let mut row = vec![[0.0f32; 4]; source.width() as usize];
    for y in 0..height {
        let vertical = sample(source.height(), height, y);
        row.fill([0.0; 4]);
        // Traverse each source row consecutively to keep reads cache friendly.
        for (offset, weight) in vertical.weights.iter().enumerate() {
            for (x, pixel) in row.iter_mut().enumerate() {
                let input = source
                    .get_pixel(x as u32, vertical.first + offset as u32)
                    .to_rgba()
                    .0;
                for channel in 0..4 {
                    pixel[channel] += f32::from(input[channel]) * weight;
                }
            }
        }
        for (x, horizontal) in horizontal.iter().enumerate() {
            let mut pixel = [0.0f32; 4];
            for (offset, weight) in horizontal.weights.iter().enumerate() {
                let input = row[horizontal.first as usize + offset];
                for channel in 0..4 {
                    pixel[channel] += input[channel] * weight;
                }
            }
            output.put_pixel(
                x as u32,
                y,
                Rgba(pixel.map(|v| v.round().clamp(0.0, 255.0) as u8)),
            );
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_triangle_matches_reference_for_downscale_upscale_and_borrowed_crops() {
        let image = RgbaImage::from_fn(41, 33, |x, y| {
            Rgba([
                (x * 29 + y * 17) as u8,
                (x * 7 + y * 31) as u8,
                (x * y) as u8,
                (x * 11 + y * 13) as u8,
            ])
        });
        for (x, y, w, h) in [
            (0, 0, 41, 33),
            (7, 11, 17, 19),
            (0, 0, 1, 33),
            (0, 0, 41, 1),
        ] {
            let view = image.view(x, y, w, h);
            for (width, height) in [(w, h), (1, 1), (9, 7), (83, 61)] {
                let expected = image::imageops::resize(
                    &*view,
                    width,
                    height,
                    image::imageops::FilterType::Triangle,
                );
                let actual = resize(&*view, width, height);
                assert_eq!(actual, expected, "{w}x{h} -> {width}x{height}");
            }
        }
    }

    #[test]
    fn fitted_canvas_keeps_transparency_and_cell_padding() {
        let image = RgbaImage::from_pixel(19, 7, Rgba([33, 44, 55, 120]));
        let canvas = canvas(&image, 40, 20);
        assert_eq!(canvas.dimensions(), (40, 20));
        assert_eq!(canvas.get_pixel(20, 10), &Rgba([33, 44, 55, 120]));
        assert_eq!(canvas.get_pixel(20, 19), &Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn rgb_and_grayscale_dispatch_preserve_crop_colors_without_rgba_source_copy() {
        let rgb =
            image::RgbImage::from_fn(19, 23, |x, y| image::Rgb([x as u8 * 11, y as u8 * 9, 77]));
        let gray = image::GrayImage::from_fn(19, 23, |x, y| image::Luma([(x * y) as u8]));
        for source in [DynamicImage::ImageRgb8(rgb), DynamicImage::ImageLuma8(gray)] {
            let expected = source.crop_imm(3, 5, 11, 17).to_rgba8();
            let expected =
                image::imageops::resize(&expected, 9, 13, image::imageops::FilterType::Triangle);
            assert_eq!(crop_resize(&source, (3, 5, 11, 17), 9, 13), expected);
        }
    }
}
