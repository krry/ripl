use ratatui::style::Color;
use std::sync::{OnceLock, atomic::{AtomicU16, Ordering}};

// Root hue (degrees on the color wheel). Override via OURACLE_ROOT_HUE (1..=360).
const DEFAULT_ROOT_HUE: u16 = 217;
static ROOT_HUE: AtomicU16 = AtomicU16::new(0);
static ROOT_HUE_ENV: OnceLock<f32> = OnceLock::new();

pub fn text_primary() -> Color {
    hsl(hue_jing(), 0.35, 0.72)
}

pub fn text_secondary() -> Color {
    hsl(hue_qi(), 0.40, 0.70)
}

pub fn text_accent() -> Color {
    hsl(hue_shen(), 0.45, 0.76)
}

pub fn text_fade(t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let sat = 0.08 + 0.20 * t;
    let light = 0.18 + 0.35 * t;
    hsl(hue_jing(), sat, light)
}

pub fn text_warning() -> Color {
    hsl(12.0, 0.55, 0.62)
}

pub fn aura_color(energy: f32, noise: f32, shimmer: f32) -> Color {
    let t = (energy * 0.35 + noise * 0.45 + shimmer * 0.20).clamp(0.0, 1.0);

    // Tri-hue blend across the wheel; shen is a smaller band to avoid dominance.
    let hue = if t < 0.50 {
        let u = t / 0.50;
        lerp_hue(hue_jing(), hue_qi(), u)
    } else if t < 0.88 {
        let u = (t - 0.50) / 0.38;
        lerp_hue(hue_qi(), hue_shen(), u)
    } else {
        let u = (t - 0.88) / 0.12;
        lerp_hue(hue_shen(), hue_jing(), u)
    };

    let sat = (0.20 + 0.45 * energy + 0.18 * (noise - 0.5) + 0.10 * (shimmer - 0.5)).clamp(0.12, 0.72);
    let light = (0.30 + 0.40 * energy + 0.12 * (noise - 0.5) + 0.10 * (shimmer - 0.5)).clamp(0.20, 0.78);
    hsl(hue, sat, light)
}

pub fn hsl(h_deg: f32, s: f32, l: f32) -> Color {
    let h = (h_deg % 360.0 + 360.0) % 360.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r1, g1, b1) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    let r = ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;

    Color::Rgb(r, g, b)
}

fn lerp_hue(a: f32, b: f32, t: f32) -> f32 {
    let mut d = (b - a) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    (a + d * t + 360.0) % 360.0
}

fn root_hue() -> f32 {
    let override_hue = ROOT_HUE.load(Ordering::Relaxed);
    if override_hue > 0 {
        return override_hue as f32;
    }
    *ROOT_HUE_ENV.get_or_init(|| {
        let val = std::env::var("OURACLE_ROOT_HUE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(DEFAULT_ROOT_HUE as f32);
        val.clamp(1.0, 360.0)
    })
}

pub fn set_root_hue(value: u16) {
    let v = value.clamp(1, 360);
    ROOT_HUE.store(v, Ordering::Relaxed);
}

pub fn current_root_hue() -> u16 {
    let override_hue = ROOT_HUE.load(Ordering::Relaxed);
    if override_hue > 0 {
        return override_hue;
    }
    let val = std::env::var("OURACLE_ROOT_HUE")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_ROOT_HUE);
    val.clamp(1, 360)
}

fn hue_jing() -> f32 {
    root_hue()
}

fn hue_qi() -> f32 {
    (root_hue() + 222.5) % 360.0
}

fn hue_shen() -> f32 {
    (root_hue() + 137.5) % 360.0
}
