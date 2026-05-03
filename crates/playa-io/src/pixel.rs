//! Raster payloads produced by decode paths (engine maps to viewport [`PixelBuffer`] / [`PixelFormat`]).

use half::f16;

#[derive(Debug, Clone)]
pub enum RawPixelBuffer {
    U8(Vec<u8>),
    F16(Vec<f16>),
    F32(Vec<f32>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawPixelFormat {
    Rgba8,
    RgbaF16,
    RgbaF32,
}

#[derive(Debug, Clone)]
pub struct DecodedRaster {
    pub buffer: RawPixelBuffer,
    pub format: RawPixelFormat,
    pub width: usize,
    pub height: usize,
}
