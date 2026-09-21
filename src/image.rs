//! Grayscale image model. Pixels are linear code values 0..=255 stored row-major.
//! All geometry in analysis uses *pixel-center* coordinates (half-pixel convention):
//! pixel column `x`, row `y` has center `(x + 0.5, y + 0.5)`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct GrayImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PitchBand {
    /// half-open row interval [y0, y1) in original image coordinates
    pub y0: u32,
    pub y1: u32,
    /// pixel pitch in micrometres; None means unknown
    pub pitch_um: Option<f64>,
}

impl GrayImage {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), (width as usize) * (height as usize));
        Self {
            width,
            height,
            data,
        }
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.data[(y as usize) * (self.width as usize) + (x as usize)]
    }

    #[inline]
    pub fn value01(&self, x: u32, y: u32) -> f64 {
        self.get(x, y) as f64 / 255.0
    }
}

/// FNV-1a 64-bit checksum, rendered as 16 hex chars.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn checksum_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}

/// Deterministic xorshift64* PRNG so every rebuild produces identical fixtures.
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9e3779b97f4a7c15 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545f4914f6cdd1d)
    }

    /// uniform sample in [-1, 1]
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64) * 2.0 - 1.0
    }
}

/// Standard normal CDF via erf, so that differentiating an ideal edge yields a
/// Gaussian LSF with exactly the requested sigma (in pixels).
pub fn norm_cdf(d: f64, sigma: f64) -> f64 {
    0.5 * (1.0 + erf(d / (sigma * std::f64::consts::SQRT_2)))
}

/// Abramowitz & Stegun 7.1.26, |error| < 1.5e-7.
fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let p = 0.3275911;
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let t = 1.0 / (1.0 + p * x);
    let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-(x * x)).exp();
    sign * y
}

pub fn clamp255(v: f64) -> u8 {
    v.clamp(0.0, 1.0).mul_add(255.0, 0.5).floor() as u8
}
