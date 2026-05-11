//! Bridge between **worker-thread** [`CompNode::compose_internal`](crate::entities::comp_node::CompNode)
//! and **main-thread**
//! [`CompositorType::blend_with_dim`](super::compositor::CompositorType::blend_with_dim).
//!
//! # Why this module exists (problem)
//!
//! Workers cannot block on [`crate::render_gpu::WgpuCompositor`]: uploads belong on the UI thread.
//!
//! Passing a raw
//! [`WgpuCompositor`](crate::render_gpu::WgpuCompositor) into [`super::node::ComputeContext`] would imply draws from arbitrary threads.
//!
//! # What we do (solution)
//!
//! 1. The worker wraps the layer stack in [`GpuBlendRequest`] and sends it via an unbounded
//!    [`std::sync::mpsc::channel`].
//! 2. The UI drains the queue ([`GpuBlendBridge::drain_into_compositor`]) **after**
//!    [`CompositorType`](super::compositor::CompositorType) holds a WGPU backend tied to [`eframe`].
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use super::compositor::LayerPayload;
use super::frame::Frame;

/// Payload moved from worker → UI for one finalized layer stack blend.
///
/// # Why [`std::sync::mpsc`] (not atomics/crossbeam)
///
/// Blocking wait + strict ownership transfer fits `std::mpsc`; unbounded enqueue avoids deadlock
/// when several workers spike while the UI is busy—memory is the throttle, latency is amortized via
/// `drain` each frame.
pub struct GpuBlendRequest {
    /// Layer stack after per-layer prep — same shape [`CompositorType`] expects.
    pub layers: Vec<LayerPayload>,
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
    /// Return value is untouched `layers`; pass them to the worker `thread_local!` Cpu compositor in
    /// [`comp_node`](crate::entities::comp_node) (same path encoding uses—no preemptive clones).
    NotQueued(Vec<LayerPayload>),
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
    /// Set by the UI thread on teardown so workers blocked inside
    /// [`Self::delegate_blend_blocking`] release cooperatively. Without this,
    /// [`Workers::drop`](crate::core::workers::Workers) hangs joining a thread parked in
    /// `recv()` when the UI side has already torn down its receiver (e.g. minimised window
    /// closed during preload).
    shutdown: Arc<AtomicBool>,
}

impl GpuBlendBridge {
    /// Builds a synchronous pair for [`gpu_blend_arc_pair`].
    pub fn pair() -> (Self, Receiver<GpuBlendRequest>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                tx,
                shutdown: Arc::new(AtomicBool::new(false)),
            },
            rx,
        )
    }

    /// UI thread: signal that any pending blends should abort.
    ///
    /// Idempotent. Workers inside [`Self::delegate_blend_blocking`] observe the flag on the
    /// next poll tick and return [`GpuBlendReport::ReplyDisconnected`] instead of waiting for
    /// a reply that may never come (receiver dropped, compositor torn down).
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
    }

    /// Worker thread: enqueue blend and block until [`Self::drain_into_compositor`] answers,
    /// the reply channel is dropped, or [`Self::shutdown`] is signalled.
    ///
    /// Returns [`GpuBlendReport::NotQueued`] when the UI dropped its receiver (portable/mobile
    /// teardown, corrupted init). Caller must fall back locally using the embedded vector.
    ///
    /// Successful completion **always consumes** `frames` on the UI side—never reclaim them via
    /// another pattern match arm.
    pub fn delegate_blend_blocking(
        &self,
        layers: Vec<LayerPayload>,
        dim: (usize, usize),
    ) -> GpuBlendReport {
        // 100 ms tick keeps shutdown latency bounded without burning CPU on a hot poll.
        const POLL: Duration = Duration::from_millis(100);

        let (reply_tx, reply_rx) = mpsc::channel();
        let req = GpuBlendRequest {
            layers,
            dim,
            reply: reply_tx,
        };

        match self.tx.send(req) {
            Err(send_err) => GpuBlendReport::NotQueued(send_err.0.layers),
            Ok(()) => loop {
                match reply_rx.recv_timeout(POLL) {
                    Ok(frame) => break GpuBlendReport::Completed(frame),
                    Err(RecvTimeoutError::Timeout) => {
                        if self.shutdown.load(Ordering::Acquire) {
                            log::warn!(
                                "GPU blend bridge: shutdown signalled while awaiting reply — releasing worker"
                            );
                            break GpuBlendReport::ReplyDisconnected;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        log::error!(
                            "GPU blend bridge: reply channel disconnected before raster result — likely UI teardown ordering"
                        );
                        break GpuBlendReport::ReplyDisconnected;
                    }
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
                        layers,
                        dim,
                        reply,
                    } = req;
                    let out = compositor.blend_with_dim(layers, dim);
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

#[cfg(test)]
mod tests {
    use super::{GpuBlendBridge, GpuBlendReport, GpuBlendRequest};
    use crate::entities::CompositorType;
    use crate::entities::Frame;
    use crate::entities::compositor::{BlendMode, CpuCompositor, LayerPayload};

    #[test]
    fn delegate_not_queued_when_ui_receiver_dropped() {
        let (bridge, rx) = GpuBlendBridge::pair();
        drop(rx);
        let f = Frame::placeholder(4, 4);
        let stack = vec![LayerPayload::pre_rendered(f, 1.0, BlendMode::Normal)];
        match bridge.delegate_blend_blocking(stack, (4, 4)) {
            GpuBlendReport::NotQueued(v) => assert_eq!(v.len(), 1),
            other => panic!("expected NotQueued, got {:?}", other),
        }
    }

    #[test]
    fn delegate_completed_after_drain_cpu() {
        let (bridge, rq_rx) = GpuBlendBridge::pair();
        let f = Frame::placeholder(2, 2);
        let stack = vec![LayerPayload::pre_rendered(f, 1.0, BlendMode::Normal)];

        // Blocking `recv` so the producer can enqueue before we blend (try_recv drains are UI-frame ordered).
        let consumer = std::thread::spawn(move || {
            let GpuBlendRequest {
                layers,
                dim,
                reply,
            } = rq_rx.recv().expect("enqueue");
            let mut compositor = CompositorType::Cpu(CpuCompositor);
            let out = compositor.blend_with_dim(layers, dim);
            let _ = reply.send(out);
        });

        let report = bridge.delegate_blend_blocking(stack, (2, 2));
        consumer.join().expect("consumer join");

        assert!(
            matches!(report, GpuBlendReport::Completed(Some(_))),
            "expected Completed(Some(..)), got {:?}",
            report
        );
    }
}
