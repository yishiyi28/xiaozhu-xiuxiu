//! ICC profile extraction, conversion to sRGB, and embedding on export.
//!
//! Imported images are converted to the editor's sRGB working space as soon as
//! they are decoded. Exports embed the compact sRGB profile in PNG (iCCP),
//! JPEG (APP2) and TIFF (tag 34675). WebP export stays profile-less because the
//! image crate's lossless encoder does not emit a VP8X container.

use std::borrow::Cow;
use std::io::Cursor;

use image::RgbaImage;
use qcms::{DataType, Intent, Profile, Transform};

/// Compact sRGB profile shipped with the application.
pub const SRGB: &[u8] = include_bytes!("../assets/color/sRGB.icc");

/// Upper bound for an embedded profile. Real display profiles are far smaller;
/// this only keeps hostile files from forcing large allocations.
const MAX_PROFILE: usize = 8 * 1024 * 1024;

/// Extract an embedded ICC profile from a supported image container.
pub fn extract(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 12 {
        return None;
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png_profile(bytes)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        jpeg_profile(bytes)
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        tiff_profile(bytes)
    } else if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        webp_profile(bytes)
    } else {
        None
    }
}

fn png_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let reader = decoder.read_info().ok()?;
    let profile = reader.info().icc_profile.as_ref()?.to_vec();
    (profile.len() <= MAX_PROFILE).then_some(profile)
}

fn jpeg_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut chunks: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut total = 0u8;
    let mut offset = 2usize;
    while offset + 1 < bytes.len() {
        if bytes[offset] != 0xFF {
            return None;
        }
        offset += 1;
        // Skip fill bytes; markers may be padded with 0xFF.
        while offset < bytes.len() && bytes[offset] == 0xFF {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        // Start of scan or end of image: no more metadata segments.
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        // Standalone markers carry no length field.
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        let length = u16::from_be_bytes([*bytes.get(offset)?, *bytes.get(offset + 1)?]) as usize;
        if length < 2 {
            return None;
        }
        let data = bytes.get(offset + 2..offset + length)?;
        if marker == 0xE2 && data.len() >= 14 && data.starts_with(b"ICC_PROFILE\0") {
            let sequence = data[12];
            let count = data[13];
            if sequence == 0 || count == 0 || sequence > count {
                return None;
            }
            total = count;
            chunks.push((sequence, data[14..].to_vec()));
        }
        offset += length;
    }
    if total == 0 || chunks.len() != usize::from(total) {
        return None;
    }
    chunks.sort_by_key(|(sequence, _)| *sequence);
    for (index, (sequence, _)) in chunks.iter().enumerate() {
        if *sequence != (index + 1) as u8 {
            return None;
        }
    }
    let profile: Vec<u8> = chunks.into_iter().flat_map(|(_, data)| data).collect();
    (profile.len() <= MAX_PROFILE).then_some(profile)
}

fn tiff_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = tiff::decoder::Decoder::new(Cursor::new(bytes)).ok()?;
    let profile = decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).ok()?;
    (profile.len() <= MAX_PROFILE).then_some(profile)
}

fn webp_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let fourcc = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        let start = offset + 8;
        let end = start.checked_add(size)?;
        let data = bytes.get(start..end)?;
        if fourcc == b"ICCP" {
            return (size <= MAX_PROFILE).then(|| data.to_vec());
        }
        // RIFF chunks are padded to an even size.
        offset = end + (size & 1);
    }
    None
}

/// Whether a profile already describes sRGB closely enough that converting
/// would only introduce rounding. Missing or malformed profiles count as sRGB
/// so callers keep the source bytes untouched.
pub fn is_srgb_equivalent(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > MAX_PROFILE {
        return true;
    }
    let Some(source) = Profile::new_from_slice(bytes, false) else {
        return true;
    };
    if source.is_sRGB() {
        return true;
    }
    let destination = Profile::new_sRGB();
    let Some(transform) =
        Transform::new(&source, &destination, DataType::RGBA8, Intent::Perceptual)
    else {
        return true;
    };
    maps_as_srgb(&transform)
}

/// Convert an image from `bytes`' profile to sRGB in place.
///
/// Returns whether a conversion was applied. Missing, malformed, or already
/// sRGB-equivalent profiles leave the pixels untouched.
pub fn convert_to_srgb(image: &mut RgbaImage, bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > MAX_PROFILE {
        return false;
    }
    let Some(source) = Profile::new_from_slice(bytes, false) else {
        return false;
    };
    convert_with(&source, image)
}

fn convert_with(source: &Profile, image: &mut RgbaImage) -> bool {
    if source.is_sRGB() {
        return false;
    }
    let destination = Profile::new_sRGB();
    let Some(transform) = Transform::new(source, &destination, DataType::RGBA8, Intent::Perceptual)
    else {
        return false;
    };
    // A profile can differ in bytes while still describing sRGB. Probe a small
    // lattice of colors; if it maps within one code value, skip the full pass.
    if maps_as_srgb(&transform) {
        return false;
    }
    transform.apply(image.as_mut());
    true
}

fn maps_as_srgb(transform: &Transform) -> bool {
    let levels = [0u8, 128, 255];
    let mut probe = Vec::with_capacity(27 * 4);
    for red in levels {
        for green in levels {
            for blue in levels {
                probe.extend_from_slice(&[red, green, blue, 255]);
            }
        }
    }
    let original = probe.clone();
    transform.apply(&mut probe);
    probe
        .iter()
        .zip(original.iter())
        .enumerate()
        .filter(|(index, _)| index % 4 != 3)
        .all(|(_, (converted, source))| converted.abs_diff(*source) <= 1)
}

/// Insert sRGB into a JPEG bitstream as one or more APP2 ICC segments.
///
/// Returns false when the buffer is not a JPEG or the profile needs more than
/// the 255 chunks the format allows.
pub fn embed_jpeg(bytes: &mut Vec<u8>, profile: &[u8]) -> bool {
    if bytes.len() < 2 || bytes[0..2] != [0xFF, 0xD8] || profile.is_empty() {
        return false;
    }
    // 65535 - 2 (length) - 12 ("ICC_PROFILE\0") - 2 (sequence/count).
    const CHUNK: usize = 65_519;
    let count = profile.len().div_ceil(CHUNK);
    if count == 0 || count > 255 {
        return false;
    }
    let mut segments = Vec::with_capacity(profile.len() + count * 18);
    for (index, chunk) in profile.chunks(CHUNK).enumerate() {
        let length = 2 + 14 + chunk.len();
        segments.extend_from_slice(&[0xFF, 0xE2]);
        segments.extend_from_slice(&(length as u16).to_be_bytes());
        segments.extend_from_slice(b"ICC_PROFILE\0");
        segments.push((index + 1) as u8);
        segments.push(count as u8);
        segments.extend_from_slice(chunk);
    }
    bytes.splice(2..2, segments);
    true
}

/// Build PNG metadata carrying the sRGB profile.
pub fn png_info(width: u32, height: u32) -> png::Info<'static> {
    let mut info = png::Info::with_size(width, height);
    info.color_type = png::ColorType::Rgba;
    info.bit_depth = png::BitDepth::Eight;
    info.icc_profile = Some(Cow::Borrowed(SRGB));
    info
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use qcms::{CIE_xyY, CIE_xyYTRIPLE};

    fn xy(x: f64, y: f64) -> CIE_xyY {
        CIE_xyY { x, y, Y: 1.0 }
    }

    #[test]
    fn fixture_profile_parses_and_is_treated_as_srgb() {
        assert!(Profile::new_from_slice(SRGB, false).is_some());
        let mut image = RgbaImage::from_pixel(2, 2, Rgba([180, 90, 30, 128]));
        let before = image.clone();
        assert!(!convert_to_srgb(&mut image, SRGB));
        assert_eq!(image, before);
    }

    #[test]
    fn linear_profile_is_converted_to_srgb() {
        // sRGB primaries with a linear (gamma 1.0) transfer curve: a mid gray
        // must brighten substantially when encoded as sRGB.
        let source = Profile::new_rgb_with_gamma_set(
            xy(0.3127, 0.3290),
            CIE_xyYTRIPLE {
                red: xy(0.64, 0.33),
                green: xy(0.30, 0.60),
                blue: xy(0.15, 0.06),
            },
            1.0,
            1.0,
            1.0,
        )
        .unwrap();
        let mut image = RgbaImage::from_pixel(2, 1, Rgba([128, 128, 128, 255]));
        assert!(convert_with(&source, &mut image));
        let pixel = image.get_pixel(0, 0);
        assert!(
            pixel[0] >= 180,
            "expected sRGB-encoded mid gray, got {pixel:?}"
        );
        assert_eq!(pixel[3], 255);
    }

    #[test]
    fn malformed_profile_is_ignored() {
        let mut image = RgbaImage::from_pixel(1, 1, Rgba([7, 8, 9, 255]));
        let before = image.clone();
        assert!(!convert_to_srgb(&mut image, b"not an ICC profile"));
        assert_eq!(image, before);
    }

    #[test]
    fn jpeg_embedding_round_trips_through_the_parser() {
        // Minimal JPEG skeleton: SOI, APP0 (length 2), EOI.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x02, 0xFF, 0xD9];
        assert!(embed_jpeg(&mut jpeg, SRGB));
        assert_eq!(extract(&jpeg).as_deref(), Some(SRGB));
        // SOI is still first and the original segments remain at the tail.
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        assert!(jpeg.ends_with(&[0xFF, 0xE0, 0x00, 0x02, 0xFF, 0xD9]));
    }

    #[test]
    fn unknown_containers_yield_no_profile() {
        assert!(extract(b"plain text, definitely not an image").is_none());
        assert!(extract(b"RIFF\x00\x00\x00\x00WEBP").is_none());
    }
}
