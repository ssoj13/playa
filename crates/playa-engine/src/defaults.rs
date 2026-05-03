//! Default timing and dimensions for nodes/comps (`Node` trait fallbacks).
//! Kept in the engine so entity code does not depend on app UI `config`.

/// Default frames per second for new comps/files.
pub const DEFAULT_FPS: f32 = 24.0;

/// Default composition dimensions (width, height).
pub const DEFAULT_DIM: (usize, usize) = (1920, 1080);

/// Default source length for new nodes.
pub const DEFAULT_SRC_LEN: i32 = 100;
