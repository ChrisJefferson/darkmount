// SPDX-License-Identifier: MPL-2.0

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::{FilterType, crop_imm, resize, rotate90, rotate270};
use image::{ColorType, DynamicImage, ImageReader, Rgb, RgbImage};

use crate::dock::{IMAGE_HEIGHT as DOCK_HEIGHT, IMAGE_WIDTH as DOCK_WIDTH, PIXEL_BYTES};
use crate::{Error, Result};

const KEY_SIZE: u32 = 120;
const MAX_KEY_JPEG_BYTES: usize = 32 * 1024 - 9;

pub fn load(path: impl AsRef<std::path::Path>) -> Result<DynamicImage> {
    Ok(ImageReader::open(path)?.with_guessed_format()?.decode()?)
}

pub fn encode_display_key(image: &DynamicImage, zoom: f32) -> Result<Vec<u8>> {
    if !zoom.is_finite() || zoom < 1.0 {
        return Err(Error::InvalidZoom);
    }
    let rgb = image.to_rgb8();
    let side = ((rgb.width().min(rgb.height()) as f32) / zoom).floor() as u32;
    if side == 0 {
        return Err(Error::InvalidZoom);
    }
    let left = (rgb.width() - side) / 2;
    let top = (rgb.height() - side) / 2;
    let cropped = crop_imm(&rgb, left, top, side, side).to_image();
    let resized = resize(&cropped, KEY_SIZE, KEY_SIZE, FilterType::Lanczos3);
    let stored = rotate90(&resized);
    let mut last = Vec::new();
    for quality in [92, 85, 75, 65, 55, 45] {
        let mut encoded = Vec::new();
        JpegEncoder::new_with_quality(&mut encoded, quality).encode(
            stored.as_raw(),
            KEY_SIZE,
            KEY_SIZE,
            ColorType::Rgb8.into(),
        )?;
        if encoded.len() <= MAX_KEY_JPEG_BYTES / 2 {
            return Ok(encoded);
        }
        last = encoded;
    }
    if last.len() > MAX_KEY_JPEG_BYTES {
        return Err(Error::DisplayImageTooLarge {
            actual: last.len(),
            maximum: MAX_KEY_JPEG_BYTES,
        });
    }
    Ok(last)
}

pub fn decode_display_key(jpeg: &[u8]) -> Result<RgbImage> {
    let stored = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)?.to_rgb8();
    if stored.dimensions() != (KEY_SIZE, KEY_SIZE) {
        return Err(Error::InvalidDecodedImageSize {
            width: stored.width(),
            height: stored.height(),
            expected_width: KEY_SIZE,
            expected_height: KEY_SIZE,
        });
    }
    Ok(rotate270(&stored))
}

pub fn encode_dock(image: &DynamicImage) -> Vec<u8> {
    let rgb = image.to_rgb8();
    let target_ratio = DOCK_WIDTH as f64 / DOCK_HEIGHT as f64;
    let ratio = rgb.width() as f64 / rgb.height() as f64;
    let cropped = if ratio > target_ratio {
        let width = (rgb.height() as f64 * target_ratio).round() as u32;
        crop_imm(&rgb, (rgb.width() - width) / 2, 0, width, rgb.height()).to_image()
    } else {
        let height = (rgb.width() as f64 / target_ratio).round() as u32;
        crop_imm(&rgb, 0, (rgb.height() - height) / 2, rgb.width(), height).to_image()
    };
    let resized = resize(
        &cropped,
        u32::from(DOCK_WIDTH),
        u32::from(DOCK_HEIGHT),
        FilterType::Lanczos3,
    );
    let mut pixels = Vec::with_capacity(PIXEL_BYTES);
    for pixel in resized.pixels() {
        let [red, green, blue] = pixel.0;
        let value =
            (u16::from(red & 0xf8) << 8) | (u16::from(green & 0xfc) << 3) | u16::from(blue >> 3);
        pixels.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(pixels.len(), PIXEL_BYTES);
    pixels
}

pub fn decode_dock(pixels: &[u8]) -> Result<RgbImage> {
    if pixels.len() != PIXEL_BYTES {
        return Err(Error::InvalidDockPixelDataSize {
            actual: pixels.len(),
            expected: PIXEL_BYTES,
        });
    }
    let mut image = RgbImage::new(u32::from(DOCK_WIDTH), u32::from(DOCK_HEIGHT));
    for (pixel, bytes) in image.pixels_mut().zip(pixels.chunks_exact(2)) {
        let value = u16::from_le_bytes([bytes[0], bytes[1]]);
        let red = (((value >> 11) & 0x1f) * 255 / 31) as u8;
        let green = (((value >> 5) & 0x3f) * 255 / 63) as u8;
        let blue = ((value & 0x1f) * 255 / 31) as u8;
        *pixel = Rgb([red, green, blue]);
    }
    Ok(image)
}

pub fn encode_png(image: &RgbImage) -> Result<Vec<u8>> {
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image.clone()).write_to(&mut output, image::ImageFormat::Png)?;
    Ok(output.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_rgb565_round_trip_preserves_representable_colours() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(640, 480, Rgb([248, 252, 248])));
        let encoded = encode_dock(&image);
        let decoded = decode_dock(&encoded).unwrap();
        assert_eq!(decoded.get_pixel(0, 0).0, [255, 255, 255]);
    }

    #[test]
    fn key_encoder_produces_device_sized_jpeg() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(240, 120, Rgb([20, 30, 40])));
        let encoded = encode_display_key(&image, 1.0).unwrap();
        let decoded = image::load_from_memory(&encoded).unwrap();
        assert_eq!(decoded.width(), KEY_SIZE);
        assert_eq!(decoded.height(), KEY_SIZE);
    }
}
