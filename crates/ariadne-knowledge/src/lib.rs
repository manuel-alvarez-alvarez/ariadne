//! The knowledge base: a symbol index over every registered repository.
//!
//! Four parts, driven by the daemon:
//!
//! - [`languages`]: which grammar, tags query, comment syntax and test marker
//!   rule a file extension is read with.
//! - [`parser`]: one file's text to the definitions in it.
//! - [`store`]: the SQLite file that holds them, keyed by blob, with an FTS5
//!   table over their identifiers.
//! - [`index`]: how a git ref is walked into the store, parsing only what
//!   changed since the last indexed commit.
//!
//! The store is disposable: it is rebuilt from the repositories whenever its
//! schema changes, and nothing else depends on it.

pub mod index;
pub mod languages;
pub mod parser;
pub mod store;

pub use index::Indexed;
pub use languages::Language;
pub use parser::Symbol;
pub use store::{
    Hit, KnowledgeStore, LanguageCount, OutlineEntry, RefStatus, SearchQuery, State, Status,
};
