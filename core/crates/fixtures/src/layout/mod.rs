//! The fixture layout, one module per area of the home folder.
//!
//! Sizes are small (a few KB) so a whole fixture stays well under 20 MB.

pub mod android;
pub mod everyday;
pub mod js;
pub mod traps;
pub mod xcode;

/// One kibibyte, for readable sizes.
pub const KB: usize = 1024;
