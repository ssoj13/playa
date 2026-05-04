//! Bridge between **worker-thread** [`CompNode::compose_internal`](crate::entities::comp_node::CompNode)
//! and **main-thread**
//! [`CompositorType::blend_with_dim`](super::compositor::CompositorType::blend_with_dim).
//!
//! # Why this module exists (problem)
//!
//! Workers run without a bound OpenGL context, but [`super::gpu_compositor::GpuCompositor`] must
//! upload textures and draws on the UI thread while the GL context is current. Passing a raw
//! `&GpuCompositor` into [`super::node::ComputeContext`] would imply GL calls from arbitrary
//! threads—undefined behaviour.
//!
//! # What we do (solution)
//!
//! 1. The worker wraps the layer stack in [`GpuBlendRequest`] and sends it via an unbounded
//!    [`std::sync::mpsc::channel`].
//! 2. The UI drains the queue ([`GpuBlendBridge::drain_into_compositor`]) **after**
//!    [`CompositorType`](super::compositor::CompositorType) holds a GPU backend with GL current.
//! 3. Workers block on the per-request reply channel ([`GpuBlendBridge::delegate_blend_blocking`])
//!    until the drain runs—serialization that keeps `compute`'s synchronous contract.
//!
//! # Why callers need [`GpuBlendReport`]
//!
//! Ownership of pixel buffers crosses threads **once**. If enqueue fails (`Receiver<GpuBlendRequest>`
//! dropped), vectors never left the worker and can be reused by the CPU path **without cloning**.
//! After a successful send, buffers live only on the UI side; even when the UI reports
//! `GpuBlendReport::Completed(None)` the stack **was consumed there** — workers **must not** assume
//! the original `Vec` is still usable locally without another compositor rebuild.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use super::compositor::BlendMode;
use super::frame::Frame;

/// Payload moved from worker → UI for one finalized layer stack blend.
///
/// # Why [`std::sync::mpsc`] (not atomics/crossbeam)
///
/// Blocking wait + strict ownership transfer fits `std::mpsc`; unbounded enqueue avoids deadlock
/// when several workers spike while the UI is busy—memory is the throttle, latency is amortized via
/// `drain` each frame.
pub struct GpuBlendRequest {
    /// Raster stack after layer transforms/effects—the same shape [`CompositorType`] expects.
    pub frames: Vec<(Frame, f32, BlendMode, [f32; 9])>,
    /// Output canvas `(width_px, height_px)`.
    pub dim: (usize, usize),
    /// One-shot ack channel so workers resume after [`GpuBlendBridge::drain_into_compositor`].
    pub reply: Sender<Option<Frame>>,
}

/// Outcome when a worker attempts to offload blending to [`GpuBlendBridge::drain_into_compositor`].
///
/// Mirrors the old `clone + Option` pattern deterministically without implicit borrows magic.
#[must_use]
#[derive(Debug)]
pub enum GpuBlendReport {
    /// UI finished blending; corresponds to [`CompositorType::blend_with_dim`]'s optional output.
    ///
    /// `None` legitimately occurs when blending produces no raster (`None` consumes inputs anyway).
    /// **Workers must not** attempt a duplicate CPU blend with stale pointers—they no longer exist
    /// on this thread.
    Completed(Option<Frame>),
    /// `Receiver<GpuBlendRequest>` is gone—the job never left the worker.
    ///
    /// Return value is untouched `frames`; pass them to the worker `thread_local!` Cpu compositor in
    /// [`comp_node`](crate::entities::comp_node) (same path encoding uses—no preemptive clones).
    NotQueued(Vec<(Frame, f32, BlendMode, [f32; 9])>),
    /// Reply channel died after the enqueue (UI crash, teardown race). Raster stack may linger on UI
    /// queue—worker cannot salvage buffers.
    ReplyDisconnected,
}

/// Worker-visible handle cloned cheaply via [`Arc`]; points at the enqueue [`Sender`].
///
/// Lifetime note: clones share the channel; teardown requires receiver + sender drops ordered with
/// workers quiescing (handled by PlayaApp teardown path).
#[derive(Clone)]
pub struct GpuBlendBridge {
    tx: Sender<GpuBlendRequest>,
}

impl GpuBlendBridge {
    /// Builds a synchronous pair for [`gpu_blend_arc_pair`].
    pub fn pair() -> (Self, Receiver<GpuBlendRequest>) {
        let (tx, rx) = mpsc::channel();
        (Self { tx }, rx)
    }

    /// Worker thread: enqueue blend and block until [`Self::drain_into_compositor`] answers.
    ///
    /// Returns [`GpuBlendReport::NotQueued`] when the UI dropped its receiver (portable/mobile
    /// teardown, corrupted init). Caller must fall back locally using the embedded vector.
    ///
    /// Successful completion **always consumes** `frames` on the UI side—never reclaim them via
    /// another pattern match arm.
    pub fn delegate_blend_blocking(
        &self,
        frames: Vec<(Frame, f32, BlendMode, [f32; 9])>,
        dim: (usize, usize),
    ) -> GpuBlendReport {
        let (reply_tx, reply_rx) = mpsc::channel();
        let req = GpuBlendRequest {
            frames,
            dim,
            reply: reply_tx,
        };

        match self.tx.send(req) {
            Err(e) => GpuBlendReport::NotQueued(e.into_inner().frames),
            Ok(()) => match reply_rx.recv() {
                Ok(frame) => GpuBlendReport::Completed(frame),
                Err(_) => {
                    log::error!(
                        "GPU blend bridge: reply channel disconnected before raster result — likely UI teardown ordering"
                    );
                    GpuBlendReport::ReplyDisconnected
                }
            },
        }
    }

    /// UI thread/Glow context owner: drains every pending offload.
    ///
    /// Why `try_recv` loop vs blocking `recv`:
    ///
    /// The UI pumps once per frame in `update`; batching amortizes locking `project.compositor`
    /// and minimizes GL state thrash versus servicing one blocking worker at a time mid-frame,
    /// which historically starved input.
    ///
    /// Returns the number of finished requests (handy when wiring `request_repaint`).
    pub fn drain_into_compositor(
        rx: &Receiver<GpuBlendRequest>,
        compositor: &mut super::CompositorType,
    ) -> usize {
        let mut n = 0;
        loop {
            match rx.try_recv() {
                Ok(req) => {
                    let GpuBlendRequest {
                        frames,
                        dim,
                        reply,
                    } = req;
                    let out = compositor.blend_with_dim(frames, dim);
                    let _ = reply.send(out);
                    n += 1;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }
        n
    }
}

/// Creates [`Arc`] around [`GpuBlendBridge`] for PlayaApp snapshots + clones into workers cheaply.
pub fn gpu_blend_arc_pair() -> (Arc<GpuBlendBridge>, Receiver<GpuBlendRequest>) {
    let (b, rx) = GpuBlendBridge::pair();
    (Arc::new(b), rx)
}
