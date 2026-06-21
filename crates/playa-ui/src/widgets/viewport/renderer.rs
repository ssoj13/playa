//! Viewport raster presenter — playa glue over the reusable `egui-hdr-view`.
//!
//! The wgpu present pipeline + tonemap shader now live in `egui-hdr-view`
//! ([`ViewportRenderer`] = `HdrView`, [`ViewportPaintCallback`] =
//! `HdrPaintCallback`). This module re-exports them under the playa names the
//! app uses and provides the small translation from playa's frame / viewport /
//! shader-preset types into the widget API (the widget is hermetic — no playa
//! types). The presenter's own methods (`new`, `set_output_format`,
//! `needs_texture_update`, `destroy`) are called directly on the re-exported
//! type; only the playa-typed operations below need translation.

use egui_hdr_view::{HdrFormat, HdrView, Mvp, Tonemap};
use playa_engine::entities::frame::{PixelBuffer, PixelFormat};

use super::ViewportRenderState;
use super::shaders::Shaders;

/// GPU image presenter (exposure/gamma/tonemap). Held in `Arc<Mutex<_>>` by the app.
pub use egui_hdr_view::HdrView as ViewportRenderer;
/// egui paint callback for the presenter (`inner: Arc<Mutex<HdrView>>`).
pub use egui_hdr_view::HdrPaintCallback as ViewportPaintCallback;

/// playa viewport transform → widget MVP.
fn to_mvp(rs: &ViewportRenderState) -> Mvp {
    Mvp {
        model: rs.model_matrix,
        view: rs.view_matrix,
        proj: rs.projection_matrix,
    }
}

/// Map the active shader preset to a tonemap mode (applied to HDR frames only).
pub fn update_tonemap(hdr: &mut HdrView, shaders: &Shaders) {
    hdr.tonemap = match shaders.current_shader.as_str() {
        "tonemap_reinhard" => Tonemap::Reinhard,
        "tonemap_aces" => Tonemap::Aces,
        _ => Tonemap::None,
    };
}

/// Stage the current frame: pack the pixel buffer to interleaved RGBA bytes
/// (u8 as-is, f16 via `to_bits`, f32 via cast) and hand it to the presenter
/// together with the quad transform.
pub fn stage_frame(
    hdr: &mut HdrView,
    rs: &ViewportRenderState,
    width: usize,
    height: usize,
    pixel_buffer: &PixelBuffer,
    pixel_format: PixelFormat,
) {
    let format = match pixel_format {
        PixelFormat::Rgba8 => HdrFormat::Rgba8,
        PixelFormat::RgbaF16 => HdrFormat::Rgba16F,
        PixelFormat::RgbaF32 => HdrFormat::Rgba32F,
    };
    let bytes = match pixel_buffer {
        PixelBuffer::U8(data) => data.clone(),
        PixelBuffer::F16(data) => {
            let bits: Vec<u16> = data.iter().map(|x| x.to_bits()).collect();
            bytemuck::cast_slice(&bits).to_vec()
        }
        PixelBuffer::F32(data) => bytemuck::cast_slice(data.as_slice()).to_vec(),
    };
    hdr.stage_frame(format, bytes, width, height, to_mvp(rs));
}

/// Update the transform without re-uploading pixels (pan/zoom-only frames).
pub fn skip_upload(hdr: &mut HdrView, rs: &ViewportRenderState) {
    hdr.skip_upload_this_frame(to_mvp(rs));
}
