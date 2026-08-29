//! The `.srclip` container: a [`Clip`] pre-decoded to frames, packed as a
//! JPEG sequence so the browser build can play it back without ffmpeg or a
//! filesystem — see `src/wasm.rs`'s module docs for why that pre-decoding
//! step exists at all. [`encode`] is the native half, run once by `showreel
//! web-pack` (`src/bin/showreel.rs`) wherever ffmpeg already lives; [`decode`]
//! is the wasm-side half, built only from this crate's existing dependencies
//! (`image`'s JPEG decoder, already pulled in for stills).
//!
//! ```text
//! b"SRCLIP1\0"      8 bytes, a magic tag
//! width, height     u32 LE each
//! fps                f64 LE
//! frame_count        u32 LE
//! frame_count times: u32 LE jpeg_len, then that many JPEG bytes
//! ```
//!
//! JPEG, not PNG: a clip is photographic, decoded, source footage, not the
//! flat-colour UI PNG is good at, and the size difference is the whole reason
//! this format exists rather than just PNG-encoding each frame the way
//! `Canvas::encode_png` already can.

use crate::assets::clip::{Clip, ClipLoop};
use anyhow::{Context, Result, bail};

pub const MAGIC: &[u8; 8] = b"SRCLIP1\0";

/// Pack every frame of `clip` (already decoded, by ffmpeg, elsewhere) as a
/// `.srclip` container. `quality` is the JPEG quality (1-100); `fps` is
/// stamped into the header for the reader's own bookkeeping but is otherwise
/// just `clip`'s own frame rate.
pub fn encode(clip: &Clip, quality: u8) -> Result<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    let (width, height) = clip.size();
    // Walk forward until the clip itself says there's nothing more, rather
    // than trusting `frame_count()` up front: for a streaming `Clip` (see
    // `assets/clip.rs`) that count can be an estimate until playback has
    // actually reached the end, and this sequential, single-pass walk is
    // exactly the access pattern the streaming decoder is fast for anyway.
    let mut jpegs: Vec<Vec<u8>> = Vec::new();
    loop {
        let i = jpegs.len();
        let t = i as f64 / clip.fps();
        let Some(pm) = clip.frame_at(t, ClipLoop::Stop)? else { break };
        // JPEG has no alpha; demultiply tiny-skia's premultiplied RGBA into
        // straight RGB the same way `Canvas::to_rgb24` does for the encoder.
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        for p in pm.pixels() {
            let a = p.alpha() as u32;
            let (r, g, b) = if a == 0 {
                (0, 0, 0)
            } else {
                let un = |c: u8| (c as u32 * 255 / a).min(255) as u8;
                (un(p.red()), un(p.green()), un(p.blue()))
            };
            rgb.extend_from_slice(&[r, g, b]);
        }
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, quality)
            .encode(&rgb, width, height, image::ExtendedColorType::Rgb8)
            .with_context(|| format!("encoding frame {i}"))?;
        jpegs.push(jpeg);
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&clip.fps().to_le_bytes());
    out.extend_from_slice(&(jpegs.len() as u32).to_le_bytes());
    for jpeg in jpegs {
        out.extend_from_slice(&(jpeg.len() as u32).to_le_bytes());
        out.extend_from_slice(&jpeg);
    }
    Ok(out)
}

/// Unpack a `.srclip` container back to frames, ready for [`Clip::from_frames`].
pub fn decode(bytes: &[u8]) -> Result<(Vec<tiny_skia::Pixmap>, f64)> {
    if bytes.len() < 24 || &bytes[0..8] != MAGIC {
        bail!("not a .srclip container");
    }
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let width = u32_at(8);
    let height = u32_at(12);
    let fps = f64::from_le_bytes(bytes[16..24].try_into().unwrap());
    let frame_count = u32_at(24);
    let mut cursor = 28usize;
    let mut frames = Vec::with_capacity(frame_count as usize);
    for i in 0..frame_count {
        if cursor + 4 > bytes.len() {
            bail!("truncated before frame {i}'s length");
        }
        let jlen = u32_at(cursor) as usize;
        cursor += 4;
        if cursor + jlen > bytes.len() {
            bail!("truncated frame {i}");
        }
        let jpeg = &bytes[cursor..cursor + jlen];
        cursor += jlen;
        let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
            .with_context(|| format!("decoding frame {i}"))?
            .to_rgba8();
        if img.width() != width || img.height() != height {
            bail!("frame {i} is {}x{}, container says {width}x{height}", img.width(), img.height());
        }
        let mut p = tiny_skia::Pixmap::new(width, height)
            .with_context(|| format!("frame {i}: zero-sized"))?;
        let dst = p.pixels_mut();
        for (j, px) in img.pixels().enumerate() {
            let [r, g, b, a] = px.0;
            dst[j] = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
        }
        frames.push(p);
    }
    Ok((frames, fps))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp_clip(n: usize) -> Clip {
        let frames = (0..n)
            .map(|i| {
                let mut p = tiny_skia::Pixmap::new(8, 6).unwrap();
                p.fill(tiny_skia::Color::from_rgba8((i * 30) as u8, 40, 200, 255));
                p
            })
            .collect();
        Clip::from_frames(frames, 5.0)
    }

    #[test]
    fn a_clip_survives_the_round_trip() {
        let clip = ramp_clip(4);
        let packed = encode(&clip, 90).unwrap();
        assert!(packed.starts_with(MAGIC));
        let (frames, fps) = decode(&packed).unwrap();
        assert_eq!(frames.len(), 4);
        assert_eq!(fps, 5.0);
        assert_eq!((frames[0].width(), frames[0].height()), (8, 6));
        // Lossy, so allow drift, but the last frame's blue channel should
        // still read as clearly the largest component.
        let px = frames[3].pixels()[0];
        assert!(px.blue() > px.red() && px.blue() > px.green());
    }

    #[test]
    fn a_truncated_container_is_a_named_error_not_a_panic() {
        let packed = encode(&ramp_clip(2), 90).unwrap();
        let err = decode(&packed[..packed.len() - 5]).unwrap_err().to_string();
        assert!(err.contains("truncated"), "{err}");
    }
}
