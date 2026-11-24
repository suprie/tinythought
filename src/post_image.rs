use std::io::Cursor;
use std::sync::LazyLock;

use image::codecs::{
    gif::GifDecoder,
    jpeg::{JpegDecoder, JpegEncoder},
    png::PngDecoder,
    webp::WebPDecoder,
};
use image::imageops::{self, FilterType};
use image::metadata::{LoopCount, Orientation};
use image::{
    AnimationDecoder, Delay, DynamicImage, GenericImage, GenericImageView, ImageDecoder,
    ImageFormat, Limits, Pixel, RgbaImage,
};

const MAX_EDGE: u32 = 1600;
const MAX_SOURCE_EDGE: u32 = 8192;
const MAX_SOURCE_PIXELS: u64 = 40_000_000;
const MAX_ANIMATION_SOURCE_PIXELS: u64 = 96_000_000;
const MAX_ANIMATION_OUTPUT_PIXELS: u64 = 12_000_000;
const MAX_ANIMATION_FRAMES: usize = 240;
const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
static IMAGE_PROCESSOR: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
static WATERMARK: LazyLock<RgbaImage> = LazyLock::new(|| {
    image::load_from_memory_with_format(
        include_bytes!("tinythought-watermark.png"),
        ImageFormat::Png,
    )
    .expect("the bundled Tinythought watermark is a valid PNG")
    .into_rgba8()
});

pub(crate) async fn prepare(bytes: Vec<u8>) -> Option<(Vec<u8>, &'static str)> {
    let permit = IMAGE_PROCESSOR.acquire().await.ok()?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        prepare_image(&bytes)
    })
    .await
    .ok()?
}

fn prepare_image(bytes: &[u8]) -> Option<(Vec<u8>, &'static str)> {
    let input = Cursor::new(bytes);
    let output = match image::guess_format(bytes).ok()? {
        ImageFormat::Jpeg => still_image(JpegDecoder::new(input).ok()?, ImageFormat::Jpeg),
        ImageFormat::Png => {
            let mut decoder = PngDecoder::with_limits(input, limits()).ok()?;
            if decoder.is_apng().ok()? {
                let (width, height) = check_size(&mut decoder)?;
                let orientation = decoder.orientation().ok()?;
                animation(decoder.apng().ok()?, width, height, orientation)
            } else {
                still_image(decoder, ImageFormat::Png)
            }
        }
        ImageFormat::Gif => {
            let mut decoder = GifDecoder::new(input).ok()?;
            let (width, height) = check_size(&mut decoder)?;
            animation(decoder, width, height, Orientation::NoTransforms)
        }
        ImageFormat::WebP => {
            let mut decoder = WebPDecoder::new(input).ok()?;
            if decoder.has_animation() {
                let (width, height) = check_size(&mut decoder)?;
                let orientation = decoder.orientation().ok()?;
                animation(decoder, width, height, orientation)
            } else {
                still_image(decoder, ImageFormat::WebP)
            }
        }
        _ => None,
    }?;
    (output.0.len() <= MAX_OUTPUT_BYTES).then_some(output)
}

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_EDGE);
    limits.max_image_height = Some(MAX_SOURCE_EDGE);
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits
}

fn check_size(decoder: &mut impl ImageDecoder) -> Option<(u32, u32)> {
    decoder.set_limits(limits()).ok()?;
    let (width, height) = decoder.dimensions();
    let pixels = u64::from(width) * u64::from(height);
    ((1..=MAX_SOURCE_PIXELS).contains(&pixels)).then_some((width, height))
}

fn still_image(
    mut decoder: impl ImageDecoder,
    format: ImageFormat,
) -> Option<(Vec<u8>, &'static str)> {
    check_size(&mut decoder)?;
    let orientation = decoder.orientation().ok()?;
    let image = prepare_pixels(DynamicImage::from_decoder(decoder).ok()?, orientation);
    if format == ImageFormat::Jpeg {
        let mut output = Vec::new();
        JpegEncoder::new_with_quality(&mut output, 85)
            .encode_image(&image)
            .ok()?;
        return Some((output, "jpg"));
    }
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, format).ok()?;
    Some((output.into_inner(), format.extensions_str()[0]))
}

fn prepare_pixels(mut image: DynamicImage, orientation: Orientation) -> DynamicImage {
    image.apply_orientation(orientation);
    if image.width().max(image.height()) > MAX_EDGE {
        image = image.resize(MAX_EDGE, MAX_EDGE, FilterType::Triangle);
    }
    add_watermark(&mut image);
    image
}

fn add_watermark(image: &mut DynamicImage) {
    let width = (image.width() / 3).min(image.height() / 2).min(208);
    // At smaller sizes the lettering would be illegible.
    if width < 48 {
        return;
    }

    let height = width * WATERMARK.height() / WATERMARK.width();
    let mark = imageops::resize(&*WATERMARK, width, height, FilterType::Triangle);
    let inset = width / 16;
    let left = image.width() - width - inset;
    let top = image.height() - height - inset;
    for (x, y, ink) in mark.enumerate_pixels().filter(|(_, _, pixel)| pixel[3] > 0) {
        let mut pixel = image.get_pixel(left + x, top + y);
        pixel.blend(ink);
        image.put_pixel(left + x, top + y, pixel);
    }
}

fn animation<'a>(
    decoder: impl AnimationDecoder<'a>,
    width: u32,
    height: u32,
    orientation: Orientation,
) -> Option<(Vec<u8>, &'static str)> {
    let plays = match decoder.loop_count() {
        LoopCount::Infinite => 0,
        LoopCount::Finite(plays) => plays.get(),
    };
    let pixels_per_frame = u64::from(width) * u64::from(height);
    let mut frames = Vec::new();
    let mut output_pixels = 0;
    for frame in decoder.into_frames() {
        let count = frames.len() + 1;
        if count > MAX_ANIMATION_FRAMES
            || pixels_per_frame * count as u64 > MAX_ANIMATION_SOURCE_PIXELS
        {
            return None;
        }
        let frame = frame.ok()?;
        let delay = frame.delay();
        let image =
            prepare_pixels(DynamicImage::ImageRgba8(frame.into_buffer()), orientation).into_rgba8();
        output_pixels += u64::from(image.width()) * u64::from(image.height());
        if output_pixels > MAX_ANIMATION_OUTPUT_PIXELS {
            return None;
        }
        frames.push((image, delay));
    }
    let (first, _) = frames.first()?;
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, first.width(), first.height());
    encoder.set_color(png::ColorType::Rgba);
    if frames.len() > 1 {
        encoder.set_animated(frames.len() as u32, plays).ok()?;
        // The decoder yields full composited frames, including transparent pixels.
        encoder.set_blend_op(png::BlendOp::Source).ok()?;
    }
    let animated = frames.len() > 1;
    let mut writer = encoder.write_header().ok()?;
    for (image, delay) in frames {
        if animated {
            let (numerator, denominator) = png_delay(delay);
            writer.set_frame_delay(numerator, denominator).ok()?;
        }
        writer.write_image_data(&image).ok()?;
    }
    writer.finish().ok()?;
    Some((output, "png"))
}

fn png_delay(delay: Delay) -> (u16, u16) {
    let (milliseconds, denominator) = delay.numer_denom_ms();
    let numerator = u64::from(milliseconds);
    let denominator = u64::from(denominator) * 1000;
    let (mut a, mut b) = (numerator, denominator);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    if let (Ok(n), Ok(d)) = (u16::try_from(numerator / a), u16::try_from(denominator / a)) {
        return (n, d);
    }
    let seconds = numerator as f64 / denominator as f64;
    let scale = (f64::from(u16::MAX) / seconds).clamp(1.0, 1000.0) as u16;
    ((seconds * f64::from(scale)).round() as u16, scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Frame, ImageEncoder, Rgba, RgbaImage};

    #[test]
    fn jpeg_applies_orientation_before_removing_exif() {
        let mut source = Vec::new();
        let mut encoder = JpegEncoder::new(&mut source);
        // Little-endian TIFF header and one EXIF orientation entry (90° clockwise).
        encoder
            .set_exif_metadata(vec![
                b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0,
                0, 0, 0,
            ])
            .unwrap();
        encoder.encode_image(&image::RgbImage::new(8, 4)).unwrap();

        let (output, extension) = prepare_image(&source).unwrap();
        assert_eq!(extension, "jpg");
        let mut decoder = JpegDecoder::new(Cursor::new(output)).unwrap();
        assert_eq!(decoder.dimensions(), (4, 8));
        assert!(decoder.exif_metadata().unwrap().is_none());
    }

    #[test]
    fn animated_gif_keeps_its_frames_and_timing() {
        let mut source = Vec::new();
        let colors = [Rgba([255, 0, 0, 255]), Rgba([0, 0, 255, 255])];
        let delays = [40, 80];
        {
            let mut encoder = GifEncoder::new(&mut source);
            encoder.set_repeat(Repeat::Finite(3)).unwrap();
            for (color, delay) in colors.into_iter().zip(delays) {
                let pixels = RgbaImage::from_pixel(240, 160, color);
                let frame = Frame::from_parts(pixels, 0, 0, Delay::from_numer_denom_ms(delay, 1));
                encoder.encode_frame(frame).unwrap();
            }
        }

        let (output, extension) = prepare_image(&source).unwrap();
        assert_eq!(extension, "png");
        let decoder = PngDecoder::new(Cursor::new(output))
            .unwrap()
            .apng()
            .unwrap();
        assert!(matches!(decoder.loop_count(), LoopCount::Finite(plays) if plays.get() == 3));
        let frames = decoder.into_frames().collect_frames().unwrap();
        assert_eq!(frames.len(), 2);
        for ((frame, color), delay) in frames.iter().zip(colors).zip(delays) {
            assert_eq!(*frame.buffer().get_pixel(0, 0), color);
            assert!(
                frame.buffer().enumerate_pixels().any(|(x, y, pixel)| {
                    x > 150 && y > 130 && pixel[0] > 220 && pixel[1] > 220 && pixel[2] > 220
                }),
                "the lower-right watermark should appear on every frame"
            );
            let (numerator, denominator) = frame.delay().numer_denom_ms();
            assert_eq!(numerator, delay * denominator);
        }
    }

    #[test]
    fn rejects_incomplete_or_unsupported_files() {
        assert!(prepare_image(b"\x89PNG\r\n\x1a\n").is_none());
        assert!(prepare_image(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>").is_none());
    }
}
