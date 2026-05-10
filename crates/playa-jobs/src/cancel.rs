//! Shared cancellation primitive.
//!
//! Replaces the per-feature `Arc<AtomicBool> cancel_flag` pattern that the
//! audit found duplicated in encode_ui (`encode_ui.rs:42`) and improvised in
//! gpu_blend_bridge teardown. One token is created per [`crate::Job`] and
//! handed to the provider through [`crate::JobContext`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::job::JobError;

#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the token cancelled. Idempotent.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
    }

    /// Cheap flag read; suitable for hot loops.
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    /// Convenience for providers: short-circuit a long polling loop.
    /// `if ctx.cancel.check_err()? { ... }` — but as `?` already returns Err,
    /// just call this between long-running operations and propagate.
    #[inline]
    pub fn check_err(&self) -> Result<(), JobError> {
        if self.is_cancelled() {
            Err(JobError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_token_not_cancelled() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        assert!(t.check_err().is_ok());
    }

    #[test]
    fn cancel_propagates_through_clones() {
        let a = CancelToken::new();
        let b = a.clone();
        a.cancel();
        assert!(b.is_cancelled());
        assert!(matches!(b.check_err(), Err(JobError::Cancelled)));
    }

    #[test]
    fn cancel_is_idempotent() {
        let t = CancelToken::new();
        t.cancel();
        t.cancel();
        assert!(t.is_cancelled());
    }
}
