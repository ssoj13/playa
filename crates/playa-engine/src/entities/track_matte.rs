//! Track-matte application: multiply a layer's composited alpha by a
//! sampled channel of another (masking) frame.
//!
//! Sampling is canvas-aligned (direct (x, y) lookup) — **not**
//! transform-aware. Both frames must have identical dimensions; a
//! mismatch logs a warning and returns the layer frame untouched. This
//! keeps Phase 1b small: full AE-style transform-following mattes are
//! a v1.1 enhancement.
//!
//! Resolution walks `Layer.mask_ref_uuid → project.media → RefNode →
//! target node → target's frame at the same `frame_idx``. Any missing
//! link → returns `None` (caller falls back to unmasked layer).

use uuid::Uuid;

use half::f16 as F16;

use super::frame::{Frame, FrameStatus, PixelBuffer};
use super::node::{ComputeContext, Node};
use super::ref_node::Channel;

/// Sample a single channel value (normalised to `0.0..=1.0`) from a
/// frame at the given pixel coordinates. HDR float sources are clamped
/// — track mattes are alpha-domain, out-of-range values are nonsense.
/// `Channel::Composite` returns `1.0` (no-op mask) since "composite as
/// mask" has no scalar interpretation.
pub(crate) fn sample_channel(frame: &Frame, x: usize, y: usize, channel: Channel) -> f32 {
    let w = frame.width();
    let idx = (y * w + x) * 4;
    let buffer = frame.buffer();
    let (r, g, b, a) = match buffer.as_ref() {
        PixelBuffer::U8(d) => (
            d[idx] as f32 / 255.0,
            d[idx + 1] as f32 / 255.0,
            d[idx + 2] as f32 / 255.0,
            d[idx + 3] as f32 / 255.0,
        ),
        PixelBuffer::F16(d) => (
            d[idx].to_f32().clamp(0.0, 1.0),
            d[idx + 1].to_f32().clamp(0.0, 1.0),
            d[idx + 2].to_f32().clamp(0.0, 1.0),
            d[idx + 3].to_f32().clamp(0.0, 1.0),
        ),
        PixelBuffer::F32(d) => (
            d[idx].clamp(0.0, 1.0),
            d[idx + 1].clamp(0.0, 1.0),
            d[idx + 2].clamp(0.0, 1.0),
            d[idx + 3].clamp(0.0, 1.0),
        ),
    };
    match channel {
        Channel::Alpha => a,
        Channel::Red => r,
        Channel::Green => g,
        Channel::Blue => b,
        Channel::Luminance => 0.2126 * r + 0.7152 * g + 0.0722 * b,
        Channel::Composite => 1.0,
    }
}

/// Apply a track matte to `layer_frame`: multiply each pixel's alpha by
/// the corresponding pixel's channel value from `mask`. Returns a new
/// `Frame` with modified alpha; the original `Arc<PixelBuffer>` is left
/// untouched (clone-on-write).
///
/// Returns `layer_frame` unchanged when:
/// - dimensions differ (logged as warn)
/// - mask is not in `Loaded` status (logged as trace — mask still
///   resolving in a worker)
pub fn apply_track_matte(layer_frame: Frame, mask: &Frame, channel: Channel) -> Frame {
    let (lw, lh) = (layer_frame.width(), layer_frame.height());
    let (mw, mh) = (mask.width(), mask.height());
    if (lw, lh) != (mw, mh) {
        log::warn!(
            "track_matte: dimension mismatch (layer {lw}x{lh} vs mask {mw}x{mh}) — mask skipped"
        );
        return layer_frame;
    }
    if mask.status() != FrameStatus::Loaded {
        log::trace!(
            "track_matte: mask not Loaded ({:?}) — mask skipped this frame",
            mask.status()
        );
        return layer_frame;
    }

    let buffer = layer_frame.buffer();
    let mut new_buffer = (*buffer).clone();
    match &mut new_buffer {
        PixelBuffer::U8(data) => {
            for y in 0..lh {
                for x in 0..lw {
                    let idx = (y * lw + x) * 4;
                    let m = sample_channel(mask, x, y, channel);
                    let new_a = (data[idx + 3] as f32 * m).clamp(0.0, 255.0).round() as u8;
                    data[idx + 3] = new_a;
                }
            }
            let PixelBuffer::U8(buf) = new_buffer else {
                unreachable!()
            };
            Frame::from_u8_buffer(buf, lw, lh)
        }
        PixelBuffer::F16(data) => {
            for y in 0..lh {
                for x in 0..lw {
                    let idx = (y * lw + x) * 4;
                    let m = sample_channel(mask, x, y, channel);
                    let cur = data[idx + 3].to_f32();
                    data[idx + 3] = F16::from_f32(cur * m);
                }
            }
            let PixelBuffer::F16(buf) = new_buffer else {
                unreachable!()
            };
            Frame::from_f16_buffer(buf, lw, lh)
        }
        PixelBuffer::F32(data) => {
            for y in 0..lh {
                for x in 0..lw {
                    let idx = (y * lw + x) * 4;
                    let m = sample_channel(mask, x, y, channel);
                    data[idx + 3] *= m;
                }
            }
            let PixelBuffer::F32(buf) = new_buffer else {
                unreachable!()
            };
            Frame::from_f32_buffer(buf, lw, lh)
        }
    }
}

/// Resolve a `Layer.mask_ref_uuid` through `project.media` to a
/// `(mask_frame, channel)` pair. Returns `None` on any resolve failure:
/// orphan ref uuid, ref points at nothing, target node missing, target
/// produces no frame at this index.
///
/// Logging is intentionally `trace` — track matte is best-effort.
pub fn resolve_mask_frame(
    ref_uuid: Uuid,
    frame_idx: i32,
    ctx: &ComputeContext,
) -> Option<(Frame, Channel)> {
    let ref_arc = ctx.media.get(&ref_uuid).or_else(|| {
        log::trace!("track_matte: mask_ref_uuid {ref_uuid} not in media");
        None
    })?;
    let ref_node = ref_arc.as_ref_node().or_else(|| {
        log::trace!("track_matte: mask_ref_uuid {ref_uuid} is not a RefNode");
        None
    })?;
    let target = ref_node.target().or_else(|| {
        log::trace!("track_matte: ref {ref_uuid} has no target");
        None
    })?;
    let target_arc = ctx.media.get(&target).or_else(|| {
        log::trace!("track_matte: ref target {target} not in media");
        None
    })?;
    let target_frame = target_arc.compute(frame_idx, ctx)?;
    Some((target_frame, ref_node.channel()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_u8(w: usize, h: usize, rgba: [u8; 4]) -> Frame {
        let mut buf = Vec::with_capacity(w * h * 4);
        for _ in 0..(w * h) {
            buf.extend_from_slice(&rgba);
        }
        let f = Frame::from_u8_buffer(buf, w, h);
        let _ = f.set_status(FrameStatus::Loaded);
        f
    }

    #[test]
    fn sample_alpha_u8_normalises_to_01() {
        let frame = frame_u8(2, 2, [10, 20, 30, 255]);
        assert!((sample_channel(&frame, 0, 0, Channel::Alpha) - 1.0).abs() < 1e-6);
        assert!((sample_channel(&frame, 1, 1, Channel::Red) - 10.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn sample_luminance_uses_rec709_weights() {
        let frame = frame_u8(1, 1, [255, 0, 0, 255]);
        let l = sample_channel(&frame, 0, 0, Channel::Luminance);
        assert!((l - 0.2126).abs() < 1e-6, "got {l}");
    }

    #[test]
    fn composite_channel_returns_1_for_noop_mask() {
        let frame = frame_u8(1, 1, [0, 0, 0, 0]);
        assert_eq!(sample_channel(&frame, 0, 0, Channel::Composite), 1.0);
    }

    #[test]
    fn apply_matte_multiplies_alpha_by_mask_alpha() {
        let layer = frame_u8(2, 2, [200, 200, 200, 200]);
        let mask = frame_u8(2, 2, [0, 0, 0, 128]);
        let result = apply_track_matte(layer, &mask, Channel::Alpha);
        let buf = result.buffer();
        let PixelBuffer::U8(data) = buf.as_ref() else {
            panic!("expected U8")
        };
        // alpha = 200 * (128/255) ≈ 100.39 → 100 (rounded)
        for px in data.chunks_exact(4) {
            assert_eq!(px[3], 100, "got {}", px[3]);
        }
    }

    #[test]
    fn apply_matte_full_mask_preserves_alpha() {
        let layer = frame_u8(2, 2, [10, 20, 30, 200]);
        let mask = frame_u8(2, 2, [255, 255, 255, 255]); // all 1.0
        let result = apply_track_matte(layer, &mask, Channel::Alpha);
        let buf = result.buffer();
        let PixelBuffer::U8(data) = buf.as_ref() else {
            panic!()
        };
        for px in data.chunks_exact(4) {
            assert_eq!(px[3], 200);
        }
    }

    #[test]
    fn apply_matte_zero_mask_zeros_alpha() {
        let layer = frame_u8(2, 2, [10, 20, 30, 200]);
        let mask = frame_u8(2, 2, [0, 0, 0, 0]);
        let result = apply_track_matte(layer, &mask, Channel::Alpha);
        let buf = result.buffer();
        let PixelBuffer::U8(data) = buf.as_ref() else {
            panic!()
        };
        for px in data.chunks_exact(4) {
            assert_eq!(px[3], 0);
        }
    }

    #[test]
    fn apply_matte_dimension_mismatch_returns_layer_unchanged() {
        let layer = frame_u8(2, 2, [10, 20, 30, 200]);
        let mask = frame_u8(4, 4, [0, 0, 0, 0]);
        let result = apply_track_matte(layer, &mask, Channel::Alpha);
        let buf = result.buffer();
        let PixelBuffer::U8(data) = buf.as_ref() else {
            panic!()
        };
        for px in data.chunks_exact(4) {
            assert_eq!(px[3], 200, "alpha must be preserved on dim mismatch");
        }
    }

    #[test]
    fn apply_matte_luma_channel_uses_brightness() {
        // Mask is bright white → luma ≈ 1.0 → alpha preserved.
        // Build separately for clarity.
        let layer = frame_u8(1, 1, [10, 20, 30, 240]);
        let mask = frame_u8(1, 1, [255, 255, 255, 0]);
        let result = apply_track_matte(layer, &mask, Channel::Luminance);
        let buf = result.buffer();
        let PixelBuffer::U8(data) = buf.as_ref() else {
            panic!()
        };
        assert_eq!(data[3], 240);
    }
}
