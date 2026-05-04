#[cfg(feature = "exr")]
mod native;
#[cfg(feature = "exr")]
pub use native::*;

#[cfg(not(feature = "exr"))]
mod stub;
#[cfg(not(feature = "exr"))]
pub use stub::*;
