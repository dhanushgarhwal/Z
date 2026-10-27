//! Z archive engine.
//!
//! Design rules: stream everything, never trust a path from an archive, never hold a whole
//! archive in memory, and use every core when the format allows it.

pub mod create;
pub mod extract;
pub mod format;
pub mod fsutil;
pub mod list;
pub mod progress;

pub use create::{create, CreateOpts, Level};
pub use extract::{extract, test, ExtractOpts, Overwrite, Report};
pub use format::Format;
pub use list::{list, Entry};
pub use progress::Progress;
