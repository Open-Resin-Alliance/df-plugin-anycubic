//! Thumbnail handling for AFZ files.
//!
//! AFZ stores preview images as standard PNGs inside the ZIP archive,
//! so we just need to decode the base64 thumbnail from the job, resize
//! it to the required dimensions, and re-encode as PNG.

use crate::engine::SlicerV3Error;
use base64::Engine;
use std::io::Cursor;

/// Decode a base64 string that may be either a plain base64 blob or a
/// `data:image/png;base64,...` data-URL.
fn decode_base64(input: &str) -> Result<Vec<u8>, SlicerV3Error> {
    let payload = input
        .split_once(',')
        .map(|(_, rhs)| rhs)
        .unwrap_or(input)
        .trim();

    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| SlicerV3Error::Png(format!("invalid base64 preview payload: {e}")))
}

/// Decode PNG bytes into (width, height, RGB8 pixels).
fn decode_png_rgb8(png_bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), SlicerV3Error> {
    let cursor = Cursor::new(png_bytes);
    let mut decoder = png::Decoder::new(cursor);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);

    let mut reader = decoder
        .read_info()
        .map_err(|e| SlicerV3Error::Png(format!("png decode header failed: {e}")))?;

    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| SlicerV3Error::Png(format!("png decode frame failed: {e}")))?;

    let src = &buf[..info.buffer_size()];
    let mut out = Vec::with_capacity((info.width as usize) * (info.height as usize) * 3);

    match info.color_type {
        png::ColorType::Rgb => out.extend_from_slice(src),
        png::ColorType::Rgba => {
            for px in src.chunks_exact(4) {
                out.push(px[0]);
                out.push(px[1]);
                out.push(px[2]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in src {
                out.push(g);
                out.push(g);
                out.push(g);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for px in src.chunks_exact(2) {
                out.push(px[0]);
                out.push(px[0]);
                out.push(px[0]);
            }
        }
        _ => {
            return Err(SlicerV3Error::Png(
                "unsupported PNG colour type for preview".to_string(),
            ));
        }
    }

    Ok((info.width, info.height, out))
}

/// Nearest-neighbour resize of an RGB8 buffer.
fn resize_rgb_nearest(src_w: u32, src_h: u32, src: &[u8], dst_w: u32, dst_h: u32) -> Vec<u8> {
    if src_w == dst_w && src_h == dst_h {
        return src.to_vec();
    }
    let mut out = vec![0u8; (dst_w as usize) * (dst_h as usize) * 3];
    for y in 0..dst_h {
        let sy = ((y as u64) * (src_h as u64) / (dst_h as u64)) as usize;
        for x in 0..dst_w {
            let sx = ((x as u64) * (src_w as u64) / (dst_w as u64)) as usize;
            let si = (sy * src_w as usize + sx) * 3;
            let di = (y as usize * dst_w as usize + x as usize) * 3;
            out[di..di + 3].copy_from_slice(&src[si..si + 3]);
        }
    }
    out
}

/// Encode RGB8 pixels into a PNG byte vector.
fn encode_png_rgb8(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, SlicerV3Error> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);

        let mut writer = encoder
            .write_header()
            .map_err(|e| SlicerV3Error::Png(format!("png encode header failed: {e}")))?;
        writer
            .write_image_data(rgb)
            .map_err(|e| SlicerV3Error::Png(format!("png encode data failed: {e}")))?;
    }
    Ok(buf)
}

/// Build a PNG thumbnail at the given target size from a base64-encoded source.
/// Returns `None` if the source is missing or empty.
pub(super) fn build_preview_png(
    base64_source: Option<&str>,
    target_w: u32,
    target_h: u32,
) -> Option<Vec<u8>> {
    let src = base64_source.filter(|s| !s.is_empty())?;
    let png_bytes = decode_base64(src).ok()?;
    let (sw, sh, rgb) = decode_png_rgb8(&png_bytes).ok()?;
    let resized = resize_rgb_nearest(sw, sh, &rgb, target_w, target_h);
    encode_png_rgb8(target_w, target_h, &resized).ok()
}
