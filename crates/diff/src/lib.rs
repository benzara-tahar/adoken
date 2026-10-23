//! PR review diff engine (spec 0004), pure and gpui-free.
//!
//! - [`FileDiff::compute`]: FR-1 local line diff of base vs target blob text.
//! - [`rows`]: unified / side-by-side row layouts with collapsed unchanged
//!   regions, built once per file so the view virtualizes by index (FR-4).
//! - [`words`]: intraline changed ranges for paired `-`/`+` lines.
//! - [`lex`]: syntax-free lexical token coloring (plain text first, AC-14).
//! - [`anchor`]: FR-5 thread anchoring, including mapping lines across
//!   iterations and the "comments on older code" fallback.

pub mod anchor;
mod file;
pub mod lex;
pub mod rows;
mod text;
pub mod words;

pub use file::{DiffLine, FileDiff, LineKind, NO_PAIR};
pub use text::Text;

/// Which file a line number refers to: `Left` = base (old), `Right` = target
/// (new). Matches ADO `threadContext.leftFile*` / `rightFile*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}
