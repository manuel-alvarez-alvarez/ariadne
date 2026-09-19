//! The knowledge base: a symbol index over every registered repository.
//!
//! Four parts, driven by the daemon:
//!
//! - [`languages`]: which grammar, tags query, comment syntax and test marker
//!   rule a file extension is read with.
//! - [`parser`]: one file's text to the definitions in it, the names it
//!   names, and the modules it imports.
//! - [`interfaces`]: what a file offers or takes beyond its symbols — the
//!   packages of a manifest, the routes it registers or requests, the
//!   environment variables it sets or reads.
//! - [`resolve`]: a name to the definition behind it, nearest first, and an
//!   interface to its counterpart in another repository.
//! - [`map`]: which files of a ref carry it, ranked over the graph, under a
//!   token budget.
//! - [`store`]: the SQLite file that holds them, keyed by blob, with an FTS5
//!   table over their identifiers and the edges between them.
//! - [`index`]: how a git ref is walked into the store, parsing only what
//!   changed since the last indexed commit.
//!
//! The store is disposable: it is rebuilt from the repositories whenever its
//! schema changes, and nothing else depends on it.

pub mod index;
pub mod interfaces;
pub mod languages;
pub mod map;
pub mod parser;
pub mod resolve;
pub mod store;

pub use index::Indexed;
pub use interfaces::{Interface, InterfaceKind};
pub use languages::Language;
pub use map::RepoMap;
pub use parser::{EdgeKind, Symbol};
pub use store::{
    Definition, Hit, ImpactCaller, Interaction, InteractionEnd, KnowledgeStore, LanguageCount,
    OutlineEntry, RefStatus, Related, SearchQuery, State, Status, SymbolContext,
};
