//! X-Trans development in linear camera RGB. Rawler's developer assumes a 2×2
//! Bayer pattern, including when it applies repeating sensor black levels.
use anyhow::{Result, ensure};
use image::Rgb32FImage;
use rawler::{
    CFA, RawImage,
    imgop::{Point, Rect},
    rawimage::RawPhotometricInterpretation,
};
use rayon::prelude::*;

pub(super) fn develop(raw: &RawImage) -> Result<Rgb32FImage> {
    let RawPhotometricInterpretation::Cfa(config) = &raw.photometric else {
        anyhow::bail!("Missing X-Trans color pattern");
    };
    let active = raw
        .active_area
        .unwrap_or(Rect::new(Point::zero(), raw.dim()));
    validate_crop(active, raw.width, raw.height)?;
    ensure!(
        active.d.w >= 6 && active.d.h >= 6,
        "X-Trans image is too small"
    );
    let crop = raw.crop_area.unwrap_or(active);
    validate_crop(crop, raw.width, raw.height)?;
    ensure!(
        crop.p.x >= active.p.x
            && crop.p.y >= active.p.y
            && crop.p.x + crop.d.w <= active.p.x + active.d.w
            && crop.p.y + crop.d.h <= active.p.y + active.d.h,
        "RAW crop is outside the active sensor area"
    );

    let black = &raw.blacklevel;
    let levels = black.as_vec();
    ensure!(
        black.cpp == 1
            && black.width > 0
            && black.height > 0
            && black.width.checked_mul(black.height) == Some(levels.len()),
        "Invalid X-Trans black levels"
    );
    ensure!(
        raw.whitelevel.0.len() == 1,
        "Unsupported X-Trans white levels"
    );
    let white = raw.whitelevel.0[0] as f32;
    ensure!(
        levels
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0 && *v < white),
        "Invalid X-Trans black/white levels"
    );
    let source = raw.data.as_f32();
    ensure!(
        source.len() == raw.width * raw.height,
        "Invalid RAW sensor data"
    );
    let mut sensor = vec![0.0; active.d.w * active.d.h];
    sensor
        .par_chunks_mut(active.d.w)
        .enumerate()
        .for_each(|(y, row)| {
            let sy = y + active.p.y;
            for (x, value) in row.iter_mut().enumerate() {
                let sx = x + active.p.x;
                let black = levels[(sy % black.height) * black.width + sx % black.width];
                *value = ((source[sy * raw.width + sx] - black) / (white - black)).max(0.0);
            }
        });
    let cfa = config.cfa.shift(active.p.x, active.p.y);
    let camera = demosaic(&sensor, active.d.w, active.d.h, &cfa);
    if crop == active {
        Ok(camera)
    } else {
        Ok(image::imageops::crop_imm(
            &camera,
            (crop.p.x - active.p.x) as u32,
            (crop.p.y - active.p.y) as u32,
            crop.d.w as u32,
            crop.d.h as u32,
        )
        .to_image())
    }
}

fn validate_crop(crop: Rect, width: usize, height: usize) -> Result<()> {
    ensure!(
        crop.d.w > 0
            && crop.d.h > 0
            && crop.p.x.checked_add(crop.d.w).is_some_and(|x| x <= width)
            && crop.p.y.checked_add(crop.d.h).is_some_and(|y| y <= height),
        "Invalid RAW sensor crop"
    );
    Ok(())
}

struct Neighbor {
    dx: isize,
    dy: isize,
    weight: f32,
}

/// Reconstruct green first, then interpolate red/blue differences from green.
/// The 6×6 phase selects actual sensor samples; measured channels are retained,
/// and no display transfer, white balance, or highlight clipping is applied.
fn demosaic(sensor: &[f32], width: usize, height: usize, cfa: &CFA) -> Rgb32FImage {
    let kernels: [[Vec<Neighbor>; 3]; 36] = std::array::from_fn(|phase| {
        std::array::from_fn(|channel| {
            let mut neighbors = Vec::new();
            for dy in -3_isize..=3 {
                for dx in -3_isize..=3 {
                    let distance = dx * dx + dy * dy;
                    // Green is dense enough to use a smaller neighborhood.
                    if distance == 0 || (channel == 1 && distance > 4) {
                        continue;
                    }
                    let y = (phase / 6 + 6).wrapping_add_signed(dy);
                    let x = (phase % 6 + 6).wrapping_add_signed(dx);
                    if cfa.color_at(y, x) == channel {
                        neighbors.push(Neighbor {
                            dx,
                            dy,
                            weight: 1.0 / distance as f32,
                        });
                    }
                }
            }
            neighbors
        })
    });
    let interpolate = |x: usize, y: usize, channel: usize, green: Option<&[f32]>| {
        let mut sum = 0.0;
        let mut weights = 0.0;
        for neighbor in &kernels[(y % 6) * 6 + x % 6][channel] {
            let sx = x.wrapping_add_signed(neighbor.dx);
            let sy = y.wrapping_add_signed(neighbor.dy);
            if sx < width && sy < height {
                let i = sy * width + sx;
                let value = sensor[i] - green.map_or(0.0, |g| g[i]);
                sum += value * neighbor.weight;
                weights += neighbor.weight;
            }
        }
        sum / weights.max(f32::EPSILON)
    };
    let mut green = vec![0.0; sensor.len()];
    green
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, value) in row.iter_mut().enumerate() {
                *value = if cfa.color_at(y, x) == 1 {
                    sensor[y * width + x]
                } else {
                    interpolate(x, y, 1, None)
                };
            }
        });
    let mut camera = Rgb32FImage::new(width as u32, height as u32);
    camera
        .as_mut()
        .par_chunks_mut(width * 3)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, rgb) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let i = y * width + x;
                rgb[1] = green[i];
                let measured = cfa.color_at(y, x);
                for channel in [0, 2] {
                    rgb[channel] = if measured == channel {
                        sensor[i]
                    } else {
                        (green[i] + interpolate(x, y, channel, Some(&green))).max(0.0)
                    };
                }
            }
        });
    camera
}
