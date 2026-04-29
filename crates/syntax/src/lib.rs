//! Tree-sitter–driven syntax highlighting, isolated from buffer storage,
//! theming, and rendering.

mod grammar;
mod highlight;
mod registry;
mod snapshot;
mod store;

pub use grammar::Grammar;
pub use highlight::{HighlightId, HighlightSpan};
pub use registry::SyntaxRegistry;
pub use snapshot::SyntaxSnapshot;
pub use store::SyntaxStore;
