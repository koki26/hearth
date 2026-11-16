//! Player pictures. Whatever is uploaded is decoded and drawn again as a
//! small JPEG, so nothing of the original file survives: no metadata (camera,
//! location), no second file hidden after the image, nothing a browser could
//! be talked into reading as something else.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use image::{ImageFormat, ImageReader, Limits};

/// What a browser may send. Already shrunk by the page, so this is generous.
pub const MAX_UPLOAD: usize = 2 * 1024 * 1024;
/// The stored picture is this many pixels on a side.
pub const SIDE: u32 = 256;
/// Uploads per account per hour.
pub const PER_HOUR: u32 = 10;

pub fn path(data_dir: &Path, id: u64) -> PathBuf {
    data_dir.join("avatars").join(format!("{id}.jpg"))
}

/// A picture waiting for review.
pub fn pending_path(data_dir: &Path, id: u64) -> PathBuf {
    data_dir.join("avatars").join(format!("{id}.pending.jpg"))
}

/// Runs the host's checker on a redrawn picture: `<command> <file>`, and a
/// non-zero exit refuses it. Empty: nothing to run, so it passes.
pub fn check(command: &str, file: &Path) -> Result<()> {
    let mut words = command.split_whitespace();
    let Some(program) = words.next() else {
        return Ok(());
    };
    let status = std::process::Command::new(program)
        .args(words)
        .arg(file)
        .stdin(std::process::Stdio::null())
        .status()
        .with_context(|| format!("could not run the picture check {program:?}"))?;
    if !status.success() {
        bail!("that picture is not allowed here");
    }
    Ok(())
}

/// The approved picture is replaced by the one that waited.
pub fn approve(data_dir: &Path, id: u64) -> Result<()> {
    std::fs::rename(pending_path(data_dir, id), path(data_dir, id))?;
    Ok(())
}

/// PNG, JPEG, WebP or GIF in; a square `SIDE`-pixel JPEG out, or why not.
pub fn shrink(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    match reader.format() {
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif) => {}
        _ => bail!("that is not a PNG, JPEG, WebP or GIF picture"),
    }
    // A small file can claim to be enormous: refuse before allocating it.
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let picture = reader.decode().context("that picture could not be read")?;
    // Transparent parts come out the colour of the page, not black.
    let mut rgba = picture.resize_to_fill(SIDE, SIDE, image::imageops::FilterType::Lanczos3).to_rgba8();
    for pixel in rgba.pixels_mut() {
        let a = u16::from(pixel[3]);
        for (c, bg) in pixel.0[..3].iter_mut().zip([23u16, 28, 58]) {
            *c = ((u16::from(*c) * a + bg * (255 - a)) / 255) as u8;
        }
    }
    let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 84).encode_image(&rgb)?;
    Ok(out)
}

/// Written beside and renamed, so a picture is never half there.
pub fn save(path: &Path, jpeg: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().expect("has a folder"))?;
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, jpeg)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

/// Both pictures, the approved one and any waiting.
pub fn remove(data_dir: &Path, id: u64) {
    let _ = std::fs::remove_file(path(data_dir, id));
    let _ = std::fs::remove_file(pending_path(data_dir, id));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::from_pixel(width, height, image::Rgba([200, 80, 20, 128]))
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn any_picture_comes_out_a_small_square_jpeg() {
        let jpeg = shrink(&png(640, 300)).unwrap();
        let back = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (SIDE, SIDE));
        assert_eq!(image::guess_format(&jpeg).unwrap(), ImageFormat::Jpeg);
    }

    #[test]
    fn what_is_not_a_picture_is_refused() {
        assert!(shrink(b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>").is_err());
        assert!(shrink(b"GIF89a but not really").is_err());
        assert!(shrink(&png(4, 4)[..20]).is_err(), "cut short");
        // A header that claims 60000 x 60000 pixels must not get that memory.
        let mut huge = png(1, 1);
        huge[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        assert!(shrink(&huge).is_err());
    }
}
