//! RGB frame resizing and padding utilities
//!
//! Provides CPU-based resize/crop/pad operations for RGB24 data.
//! For YUV conversion, see `convert` module with SwsContext.

/// Resize or pad RGB24 frame to target dimensions
///
/// Handles mixed resolution sequences by:
/// - If src > dst: center-crop to target size
/// - If src < dst: center + letterbox with fill_color
/// - If src == dst: copy as-is (fast path)
///
/// # Arguments
///
/// - `src_data`: Source RGB24 data (width * height * 3 bytes)
/// - `src_w`, `src_h`: Source dimensions
/// - `dst_w`, `dst_h`: Target dimensions
/// - `fill_color`: RGB color for letterbox padding [R, G, B]
///
/// # Returns
///
/// New RGB24 buffer at target dimensions
///
/// # Examples
///
/// ```rust,no_run
/// # use playa::rgb_cvt::resize_or_pad_rgb24;
/// let src = vec![255u8; 640 * 480 * 3];
/// // Pad to 1920x1080 with green background
/// let dst = resize_or_pad_rgb24(&src, 640, 480, 1920, 1080, [0, 100, 0]);
/// ```
#[allow(dead_code)]
pub fn resize_or_pad_rgb24(
    src_data: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    fill_color: [u8; 3],
) -> Vec<u8> {
    // Fast path: same size
    if src_w == dst_w && src_h == dst_h {
        return src_data.to_vec();
    }

    let mut dst_data = vec![0u8; (dst_w * dst_h * 3) as usize];

    // Fill with background color
    for y in 0..dst_h {
        for x in 0..dst_w {
            let dst_idx = ((y * dst_w + x) * 3) as usize;
            dst_data[dst_idx] = fill_color[0]; // R
            dst_data[dst_idx + 1] = fill_color[1]; // G
            dst_data[dst_idx + 2] = fill_color[2]; // B
        }
    }

    // Calculate copy region (center-aligned)
    let copy_w = src_w.min(dst_w);
    let copy_h = src_h.min(dst_h);

    let src_offset_x = if src_w > dst_w {
        (src_w - dst_w) / 2
    } else {
        0
    };
    let src_offset_y = if src_h > dst_h {
        (src_h - dst_h) / 2
    } else {
        0
    };

    let dst_offset_x = if dst_w > src_w {
        (dst_w - src_w) / 2
    } else {
        0
    };
    let dst_offset_y = if dst_h > src_h {
        (dst_h - src_h) / 2
    } else {
        0
    };

    // Copy pixel data
    for y in 0..copy_h {
        let src_y = src_offset_y + y;
        let dst_y = dst_offset_y + y;

        for x in 0..copy_w {
            let src_x = src_offset_x + x;
            let dst_x = dst_offset_x + x;

            let src_idx = ((src_y * src_w + src_x) * 3) as usize;
            let dst_idx = ((dst_y * dst_w + dst_x) * 3) as usize;

            // Copy RGB triplet
            dst_data[dst_idx] = src_data[src_idx]; // R
            dst_data[dst_idx + 1] = src_data[src_idx + 1]; // G
            dst_data[dst_idx + 2] = src_data[src_idx + 2]; // B
        }
    }

    dst_data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resize_same_size() {
        let src = vec![255u8; 640 * 480 * 3];
        let dst = resize_or_pad_rgb24(&src, 640, 480, 640, 480, [0, 0, 0]);
        assert_eq!(src, dst);
    }

    #[test]
    fn test_pad_smaller_to_larger() {
        // 2x2 white image → 4x4 with green padding
        let src = vec![255u8; 2 * 2 * 3];
        let dst = resize_or_pad_rgb24(&src, 2, 2, 4, 4, [0, 100, 0]);

        // Check center 2x2 is white
        for y in 1..3 {
            for x in 1..3 {
                let idx = ((y * 4 + x) * 3) as usize;
                assert_eq!(dst[idx], 255, "R at ({}, {})", x, y);
                assert_eq!(dst[idx + 1], 255, "G at ({}, {})", x, y);
                assert_eq!(dst[idx + 2], 255, "B at ({}, {})", x, y);
            }
        }

        // Check corners are green
        let corners = [(0, 0), (3, 0), (0, 3), (3, 3)];
        for (x, y) in corners {
            let idx = ((y * 4 + x) * 3) as usize;
            assert_eq!(dst[idx], 0, "R at corner ({}, {})", x, y);
            assert_eq!(dst[idx + 1], 100, "G at corner ({}, {})", x, y);
            assert_eq!(dst[idx + 2], 0, "B at corner ({}, {})", x, y);
        }
    }

    #[test]
    fn test_crop_larger_to_smaller() {
        // 4x4 with specific pattern → center 2x2
        let mut src = vec![0u8; 4 * 4 * 3];

        // Fill center 2x2 with white
        for y in 1..3 {
            for x in 1..3 {
                let idx = ((y * 4 + x) * 3) as usize;
                src[idx] = 255; // R
                src[idx + 1] = 255; // G
                src[idx + 2] = 255; // B
            }
        }

        let dst = resize_or_pad_rgb24(&src, 4, 4, 2, 2, [0, 0, 0]);

        // All pixels should be white (center crop)
        for y in 0..2 {
            for x in 0..2 {
                let idx = ((y * 2 + x) * 3) as usize;
                assert_eq!(dst[idx], 255, "R at ({}, {})", x, y);
                assert_eq!(dst[idx + 1], 255, "G at ({}, {})", x, y);
                assert_eq!(dst[idx + 2], 255, "B at ({}, {})", x, y);
            }
        }
    }
}
