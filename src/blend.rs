use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    Difference,
    ColorDodge,
    ColorBurn,
    Hue,
    Saturation,
    Color,
    Luminosity,
    HardLight,
    SoftLight,
    Exclusion,
    LinearDodge,
    LinearBurn,
    VividLight,
}

impl BlendMode {
    pub const ALL: [Self; 19] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::Darken,
        Self::Lighten,
        Self::Difference,
        Self::ColorDodge,
        Self::ColorBurn,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
        Self::HardLight,
        Self::SoftLight,
        Self::Exclusion,
        Self::LinearDodge,
        Self::LinearBurn,
        Self::VividLight,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Overlay => "Overlay",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
            Self::Difference => "Difference",
            Self::ColorDodge => "Color Dodge",
            Self::ColorBurn => "Color Burn",
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Color => "Color",
            Self::Luminosity => "Luminosity",
            Self::HardLight => "Hard Light",
            Self::SoftLight => "Soft Light",
            Self::Exclusion => "Exclusion",
            Self::LinearDodge => "Linear Dodge",
            Self::LinearBurn => "Linear Burn",
            Self::VividLight => "Vivid Light",
        }
    }
}

fn luminance(c: [f32; 3]) -> f32 {
    c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11
}
fn saturation(c: [f32; 3]) -> f32 {
    c.into_iter().fold(f32::MIN, f32::max) - c.into_iter().fold(f32::MAX, f32::min)
}

fn set_luminance(mut c: [f32; 3], value: f32) -> [f32; 3] {
    let delta = value - luminance(c);
    c = c.map(|v| v + delta);
    let min = c.into_iter().fold(f32::MAX, f32::min);
    let max = c.into_iter().fold(f32::MIN, f32::max);
    if min < 0.0 {
        c = c.map(|v| value + (v - value) * value / (value - min).max(1e-6));
    }
    if max > 1.0 {
        c = c.map(|v| value + (v - value) * (1.0 - value) / (max - value).max(1e-6));
    }
    c
}

fn set_saturation(c: [f32; 3], value: f32) -> [f32; 3] {
    let min = c.into_iter().fold(f32::MAX, f32::min);
    let max = c.into_iter().fold(f32::MIN, f32::max);
    if max <= min {
        [0.0; 3]
    } else {
        c.map(|v| (v - min) * value / (max - min))
    }
}

fn soft_light(d: f32, s: f32) -> f32 {
    if s <= 0.5 {
        d - (1.0 - 2.0 * s) * d * (1.0 - d)
    } else {
        let shaped = if d <= 0.25 {
            ((16.0 * d - 12.0) * d + 4.0) * d
        } else {
            d.sqrt()
        };
        d + (2.0 * s - 1.0) * (shaped - d)
    }
}

fn vivid_light(d: f32, s: f32) -> f32 {
    if s <= 0.5 {
        if s <= 0.0 {
            0.0
        } else {
            1.0 - ((1.0 - d) / (2.0 * s)).min(1.0)
        }
    } else if s >= 1.0 {
        1.0
    } else {
        (d / (2.0 * (1.0 - s))).min(1.0)
    }
}

/// W3C compositing formula, with straight sRGB inputs and outputs.
pub fn composite(dst: [f32; 4], src: [f32; 4], mode: BlendMode) -> [f32; 4] {
    let alpha = src[3] + dst[3] * (1.0 - src[3]);
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let d = [dst[0], dst[1], dst[2]];
    let s = [src[0], src[1], src[2]];
    let mixed = match mode {
        BlendMode::Hue => set_luminance(set_saturation(s, saturation(d)), luminance(d)),
        BlendMode::Saturation => set_luminance(set_saturation(d, saturation(s)), luminance(d)),
        BlendMode::Color => set_luminance(s, luminance(d)),
        BlendMode::Luminosity => set_luminance(d, luminance(s)),
        _ => std::array::from_fn(|i| match mode {
            BlendMode::Multiply => d[i] * s[i],
            BlendMode::Screen => d[i] + s[i] - d[i] * s[i],
            BlendMode::Overlay => {
                if d[i] <= 0.5 {
                    2.0 * d[i] * s[i]
                } else {
                    1.0 - 2.0 * (1.0 - d[i]) * (1.0 - s[i])
                }
            }
            BlendMode::Darken => d[i].min(s[i]),
            BlendMode::Lighten => d[i].max(s[i]),
            BlendMode::Difference => (d[i] - s[i]).abs(),
            BlendMode::ColorDodge => {
                if d[i] == 0.0 {
                    0.0
                } else if s[i] >= 1.0 {
                    1.0
                } else {
                    (d[i] / (1.0 - s[i])).min(1.0)
                }
            }
            BlendMode::ColorBurn => {
                if d[i] == 1.0 {
                    1.0
                } else if s[i] <= 0.0 {
                    0.0
                } else {
                    1.0 - ((1.0 - d[i]) / s[i]).min(1.0)
                }
            }
            BlendMode::HardLight => {
                if s[i] <= 0.5 {
                    2.0 * d[i] * s[i]
                } else {
                    1.0 - 2.0 * (1.0 - d[i]) * (1.0 - s[i])
                }
            }
            BlendMode::SoftLight => soft_light(d[i], s[i]),
            BlendMode::Exclusion => d[i] + s[i] - 2.0 * d[i] * s[i],
            BlendMode::LinearDodge => (d[i] + s[i]).min(1.0),
            BlendMode::LinearBurn => (d[i] + s[i] - 1.0).max(0.0),
            BlendMode::VividLight => vivid_light(d[i], s[i]),
            _ => s[i],
        }),
    };
    let mut result = [0.0; 4];
    for i in 0..3 {
        result[i] = ((1.0 - src[3]) * dst[3] * dst[i]
            + (1.0 - dst[3]) * src[3] * src[i]
            + dst[3] * src[3] * mixed[i])
            / alpha;
    }
    result[3] = alpha;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_over_and_blend_on_transparency() {
        let result = composite(
            [0.0, 0.0, 1.0, 1.0],
            [1.0, 0.0, 0.0, 0.5],
            BlendMode::Normal,
        );
        assert_eq!(result, [0.5, 0.0, 0.5, 1.0]);
        for mode in BlendMode::ALL {
            let result = composite([0.0; 4], [0.2, 0.4, 0.8, 0.5], mode);
            assert_eq!(result, [0.2, 0.4, 0.8, 0.5]);
        }
    }
    #[test]
    fn new_blend_modes_match_reference_values() {
        let out = |d: f32, s: f32, mode| composite([d, d, d, 1.0], [s, s, s, 1.0], mode)[0];
        assert!((out(0.25, 0.25, BlendMode::HardLight) - 0.125).abs() < 1e-6);
        assert!((out(0.75, 0.75, BlendMode::HardLight) - 0.875).abs() < 1e-6);
        assert!((out(0.5, 0.5, BlendMode::SoftLight) - 0.5).abs() < 1e-6);
        assert!((out(1.0, 1.0, BlendMode::Exclusion) - 0.0).abs() < 1e-6);
        assert!((out(0.2, 0.3, BlendMode::LinearDodge) - 0.5).abs() < 1e-6);
        assert!((out(0.7, 0.6, BlendMode::LinearDodge) - 1.0).abs() < 1e-6);
        assert!((out(0.7, 0.6, BlendMode::LinearBurn) - 0.3).abs() < 1e-6);
        assert!((out(0.2, 0.3, BlendMode::LinearBurn) - 0.0).abs() < 1e-6);
        assert!((out(0.5, 0.5, BlendMode::VividLight) - 0.5).abs() < 1e-6);
        assert!((out(0.5, 0.0, BlendMode::VividLight) - 0.0).abs() < 1e-6);
        assert!((out(0.5, 1.0, BlendMode::VividLight) - 1.0).abs() < 1e-6);
    }
}
