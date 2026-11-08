//! The generated creator table lives in zartex-core.
//!
//! What it does: re-exports the creator table (`CREATORS`) and its `Creator` type from `zartex_core::creators`, so the app
//! imports them from this module. Nothing is defined here.
//! Used by: `main.rs` (the Add to mission dialog) and `creator_tools`.
pub use zartex_core::creators::*;
