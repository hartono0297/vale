//! The lyrics card's colours, pure: the album art's dominant colour from its
//! pixels, the two-stop gradient built from it, and the contrast maths that
//! keeps the white text readable on both stops.
//!
//! Text is always white. The gradient is darkened until, at BOTH stops, the
//! current line (white) has at least 7:1 and the dimmed lines (white at 40 %
//! composited in sRGB over the stop) at least 3:1. The progress fill is a
//! lighter tint of the art colour with at least 3:1 against the bottom stop.

/// The current line's white on a stop: at least this.
pub const CURRENT_MIN: f64 = 7.0;
/// The dimmed lines (40 % white) on a stop: at least this.
pub const DIM_MIN: f64 = 3.0;
/// The progress fill against the bottom stop: at least this.
pub const ACCENT_MIN: f64 = 3.0;
/// The alpha of the past and upcoming lines.
pub const DIM_ALPHA: f64 = 0.40;

/// Relative luminance targets of the stops (the maths below also checks the
/// contrast, so a saturated colour that needs more is darkened further).
const TOP_LUM: f64 = 0.045;
const BOTTOM_LUM: f64 = 0.012;

fn chan(rgb: u32, shift: u32) -> u8 {
    ((rgb >> shift) & 0xFF) as u8
}

pub fn pack(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

fn lin(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn unlin(v: f64) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let c = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round().clamp(0.0, 255.0) as u8
}

/// WCAG relative luminance of `0xRRGGBB`.
pub fn luminance(rgb: u32) -> f64 {
    0.2126 * lin(chan(rgb, 16)) + 0.7152 * lin(chan(rgb, 8)) + 0.0722 * lin(chan(rgb, 0))
}

/// WCAG contrast ratio of two colours.
pub fn contrast(a: u32, b: u32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// `fg` at `alpha` over `bg`, blended per channel in sRGB (what the screen
/// shows for text drawn with an alpha).
pub fn composite(fg: u32, alpha: f64, bg: u32) -> u32 {
    let mix = |shift: u32| {
        let f = f64::from(chan(fg, shift));
        let b = f64::from(chan(bg, shift));
        (f * alpha + b * (1.0 - alpha)).round().clamp(0.0, 255.0) as u8
    };
    pack(mix(16), mix(8), mix(0))
}

/// Contrast of the current line (white) on `stop`.
pub fn current_contrast(stop: u32) -> f64 {
    contrast(0xFFFFFF, stop)
}

/// Contrast of the dimmed lines (white at 40 %, composited) on `stop`.
pub fn dim_contrast(stop: u32) -> f64 {
    contrast(composite(0xFFFFFF, DIM_ALPHA, stop), stop)
}

/// A stop meets the text rules.
pub fn stop_ok(stop: u32) -> bool {
    current_contrast(stop) >= CURRENT_MIN && dim_contrast(stop) >= DIM_MIN
}

/// The average colour of the art, straight (not premultiplied) BGRA pixels
/// `w` x `h` (decoded at about 16 x 16). Pixels that are nearly transparent
/// are skipped; saturated ones weigh more, so a mostly grey cover with a red
/// object gives red; the hue is kept. `None` when no pixel counts.
pub fn dominant_rgb(bgra: &[u8], w: u32, h: u32) -> Option<u32> {
    let n = (w as usize).saturating_mul(h as usize).min(bgra.len() / 4);
    let (mut sr, mut sg, mut sb, mut sw) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for px in bgra.chunks_exact(4).take(n) {
        let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
        if a < 32 {
            continue;
        }
        let (rf, gf, bf) = (f64::from(r), f64::from(g), f64::from(b));
        let max = rf.max(gf).max(bf);
        let min = rf.min(gf).min(bf);
        let sat = if max <= 0.0 { 0.0 } else { (max - min) / max };
        let value = max / 255.0;
        // Saturated, not-too-dark pixels lead; a grey one still counts a little.
        let weight = (0.05 + sat * sat * 3.0) * (0.15 + 0.85 * value) * f64::from(a) / 255.0;
        sr += rf * weight;
        sg += gf * weight;
        sb += bf * weight;
        sw += weight;
    }
    if sw <= 0.0 {
        return None;
    }
    let to = |s: f64| (s / sw).round().clamp(0.0, 255.0) as u8;
    Some(pack(to(sr), to(sg), to(sb)))
}

/// `rgb` with its linear channels scaled by `k` (same hue and chroma ratio).
fn scaled(rgb: u32, k: f64) -> u32 {
    pack(
        unlin(lin(chan(rgb, 16)) * k),
        unlin(lin(chan(rgb, 8)) * k),
        unlin(lin(chan(rgb, 0)) * k),
    )
}

/// `rgb` darkened (scaled in linear light) until its luminance is at most
/// `target` and `ok` accepts it. A black input stays black.
fn darkened(rgb: u32, target: f64, ok: impl Fn(u32) -> bool) -> u32 {
    let l = luminance(rgb);
    let mut k = if l > target { target / l } else { 1.0 };
    let mut out = scaled(rgb, k);
    // Rounding to 8 bits and saturated colours (whose dimmed text composite
    // is brighter than a grey's) may need a few steps more.
    let mut guard = 0;
    while !ok(out) && guard < 64 {
        k *= 0.9;
        out = scaled(rgb, k);
        guard += 1;
    }
    if ok(out) { out } else { 0x000000 }
}

/// A lighter tint of `rgb` (mixed towards white) with at least `ACCENT_MIN`
/// against `bottom`.
fn accent_for(rgb: u32, bottom: u32) -> u32 {
    let mut t = 0.45;
    loop {
        let mix = |shift: u32| {
            let c = f64::from(chan(rgb, shift));
            (c + (255.0 - c) * t).round().clamp(0.0, 255.0) as u8
        };
        let a = pack(mix(16), mix(8), mix(0));
        if contrast(a, bottom) >= ACCENT_MIN || t >= 1.0 {
            return a;
        }
        t = (t + 0.05).min(1.0);
    }
}

/// The card's gradient from the art colour `rgb`: `(top, bottom, accent)`.
/// The top stop is the art colour darkened to a relative luminance of about
/// 0.045 (less if its saturation needs it), the bottom the same hue at about
/// 0.012; both meet the text rules. The accent is a lighter tint.
pub fn card_gradient(rgb: u32) -> (u32, u32, u32) {
    let top = darkened(rgb, TOP_LUM, stop_ok);
    let bottom = darkened(top, BOTTOM_LUM.min(luminance(top)), stop_ok);
    let accent = accent_for(rgb, bottom);
    (top, bottom, accent)
}

/// The gradient when the track has no art: the two surfaces of Vale's dark
/// window palette (`top` the lighter, `bottom` the window background), kept
/// inside the text rules, and the accent `accent` lightened if it must be.
pub fn palette_gradient(top: u32, bottom: u32, accent: u32) -> (u32, u32, u32) {
    let top = darkened(top, TOP_LUM, stop_ok);
    let bottom = darkened(bottom, BOTTOM_LUM.max(luminance(bottom)).min(TOP_LUM), stop_ok);
    let accent = if contrast(accent, bottom) >= ACCENT_MIN {
        accent
    } else {
        accent_for(accent, bottom)
    };
    (top, bottom, accent)
}

/// A colour between `a` and `b` (`t` 0..=1), per channel.
pub fn lerp(a: u32, b: u32, t: f32) -> u32 {
    let t = f64::from(t.clamp(0.0, 1.0));
    let mix = |shift: u32| {
        let (x, y) = (f64::from(chan(a, shift)), f64::from(chan(b, shift)));
        (x + (y - x) * t).round().clamp(0.0, 255.0) as u8
    };
    pack(mix(16), mix(8), mix(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` pixels of one straight BGRA colour.
    fn flat(rgb: u32, a: u8, n: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for _ in 0..n {
            v.extend([chan(rgb, 0), chan(rgb, 8), chan(rgb, 16), a]);
        }
        v
    }

    #[test]
    fn contrast_maths_matches_known_values() {
        assert!((contrast(0xFFFFFF, 0x000000) - 21.0).abs() < 1e-9);
        assert!((contrast(0x777777, 0xFFFFFF) - 4.478).abs() < 0.01);
        assert_eq!(luminance(0x000000), 0.0);
        assert!((luminance(0xFFFFFF) - 1.0).abs() < 1e-9);
        // 40 % white over black: 102 per channel.
        assert_eq!(composite(0xFFFFFF, 0.4, 0x000000), 0x666666);
        assert_eq!(composite(0xFFFFFF, 1.0, 0x123456), 0xFFFFFF);
        assert_eq!(composite(0xFFFFFF, 0.0, 0x123456), 0x123456);
    }

    #[test]
    fn the_dominant_colour_of_flat_art_is_that_colour() {
        for rgb in [0xE02020u32, 0x2040C0, 0x20A060, 0xF0F0F0, 0x808080] {
            let got = dominant_rgb(&flat(rgb, 255, 256), 16, 16).unwrap();
            assert_eq!(got, rgb, "{rgb:06X}");
        }
    }

    #[test]
    fn nearly_transparent_pixels_do_not_count() {
        let mut px = flat(0x00FF00, 10, 128);
        px.extend(flat(0xFF0000, 255, 128));
        assert_eq!(dominant_rgb(&px, 16, 16), Some(0xFF0000));
        assert_eq!(dominant_rgb(&flat(0xFF0000, 31, 256), 16, 16), None);
        assert_eq!(dominant_rgb(&[], 0, 0), None);
        // A short buffer reads what is there.
        assert_eq!(dominant_rgb(&flat(0x336699, 255, 4), 16, 16), Some(0x336699));
    }

    #[test]
    fn saturated_pixels_lead_and_the_hue_stays() {
        // A mostly grey cover with a small red patch: the average is red-ish,
        // not grey, and red stays the strongest channel.
        let mut px = flat(0x808080, 255, 220);
        px.extend(flat(0xE01010, 255, 36));
        let got = dominant_rgb(&px, 16, 16).unwrap();
        let (r, g, b) = (chan(got, 16), chan(got, 8), chan(got, 0));
        assert!(r > g && r > b, "{got:06X}");
        assert!(u16::from(r) - u16::from(g) > 20, "{got:06X}");
        // A plain mix of the two would be much greyer.
        let plain_g = (220.0 * 128.0 + 36.0 * 16.0) / 256.0;
        assert!(f64::from(g) < plain_g, "{got:06X}");
    }

    /// The numbers the note records.
    #[test]
    fn the_gradient_meets_the_text_rules_for_every_kind_of_art() {
        let arts: [(&str, u32); 8] = [
            ("white", 0xFFFFFF),
            ("yellow", 0xFFE600),
            ("cyan", 0x00E5FF),
            ("red", 0xE02020),
            ("mid-grey", 0x808080),
            ("black", 0x000000),
            ("blue", 0x2020FF),
            ("green", 0x20D040),
        ];
        for (name, art) in arts {
            let (top, bottom, accent) = card_gradient(art);
            let (ct, cb) = (current_contrast(top), current_contrast(bottom));
            let (dt, db) = (dim_contrast(top), dim_contrast(bottom));
            let ab = contrast(accent, bottom);
            println!(
                "{name:9} art {art:06X} top {top:06X} L {:.4} cur {ct:.2} dim {dt:.2} | bottom {bottom:06X} L {:.4} cur {cb:.2} dim {db:.2} | accent {accent:06X} vs bottom {ab:.2}",
                luminance(top),
                luminance(bottom)
            );
            assert!(ct >= CURRENT_MIN && cb >= CURRENT_MIN, "{name} current");
            assert!(dt >= DIM_MIN && db >= DIM_MIN, "{name} dimmed");
            assert!(ab >= ACCENT_MIN, "{name} accent {ab}");
            assert!(luminance(top) <= 0.045 + 1e-3, "{name} top lum");
            assert!(luminance(bottom) <= luminance(top) + 1e-9, "{name} order");
            assert!(luminance(bottom) <= 0.012 + 1e-3, "{name} bottom lum");
        }
    }

    #[test]
    fn the_hue_survives_the_darkening() {
        let (top, bottom, accent) = card_gradient(0xE02020);
        for c in [top, bottom] {
            let (r, g, b) = (chan(c, 16), chan(c, 8), chan(c, 0));
            assert!(r > g && r > b, "{c:06X}");
        }
        // The accent is lighter than the art's own darkened stop.
        assert!(luminance(accent) > luminance(top) * 2.0);
        // Grey stays grey.
        let (top, _, _) = card_gradient(0x808080);
        assert_eq!(chan(top, 16), chan(top, 8));
        assert_eq!(chan(top, 8), chan(top, 0));
    }

    #[test]
    fn a_dark_palette_is_kept_and_a_weak_accent_is_lightened() {
        // The neutral dark window surfaces (#232527 over #1B1D1F) already pass.
        let (top, bottom, accent) = palette_gradient(0x232527, 0x1B1D1F, 0xD97757);
        assert!(stop_ok(top) && stop_ok(bottom));
        assert!(luminance(top) <= 0.045 + 1e-3);
        assert!(contrast(accent, bottom) >= ACCENT_MIN);
        // A dim accent on a dark bottom is lifted.
        let (_, bottom, accent) = palette_gradient(0x232527, 0x1B1D1F, 0x303030);
        assert!(contrast(accent, bottom) >= ACCENT_MIN);
        // A light "dark" surface is darkened to pass.
        let (top, bottom, _) = palette_gradient(0x808080, 0x606060, 0xFFFFFF);
        assert!(stop_ok(top) && stop_ok(bottom));
    }

    #[test]
    fn colours_blend_per_channel() {
        assert_eq!(lerp(0x000000, 0xFFFFFF, 0.0), 0x000000);
        assert_eq!(lerp(0x000000, 0xFFFFFF, 1.0), 0xFFFFFF);
        assert_eq!(lerp(0x000000, 0xFEFEFE, 0.5), 0x7F7F7F);
        assert_eq!(lerp(0x102030, 0x102030, 0.7), 0x102030);
        assert_eq!(lerp(0x000000, 0xFFFFFF, 9.0), 0xFFFFFF);
    }
}
