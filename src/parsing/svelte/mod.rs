//! Svelte language parser implementation
//!
//! Svelte files are HTML-shaped templates whose `<script>` blocks contain
//! JavaScript or TypeScript. The parser locates each script block with the
//! tree-sitter-svelte grammar and re-parses its body with the JS or TS parser
//! (selected by the block's `lang` attribute), then maps ranges back to
//! file-level positions. Template-level constructs handled directly include
//! `{#snippet}` definitions.
//!
//! ## Module Components
//!
//! - [`parser`]: script extraction, JS/TS delegation, and snippet collection
//! - [`behavior`]: Svelte-specific language behavior and resolution wiring
//! - [`definition`]: language registration
//! - [`resolution`]: symbol resolution, delegating to the JS resolver since
//!   `<script>` blocks are JS/TS
//! - [`audit`]: grammar node coverage tracking for the ABI audit pipeline

pub mod audit;
pub mod behavior;
pub mod definition;
pub mod parser;
pub mod resolution;

pub use behavior::SvelteBehavior;
pub use definition::SvelteLanguage;
pub use parser::SvelteParser;
pub use resolution::{SvelteInheritanceResolver, SvelteResolutionContext};

pub(crate) use definition::register;
