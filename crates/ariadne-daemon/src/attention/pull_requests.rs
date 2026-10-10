//! The pull-request producer: a request's next step.
//!
//! Not implemented yet — a later task gives it its eligibility rules (026,
//! 029) and migrates the "ready to merge" / "review posted" surface onto
//! it, preserving what it answers today. Registered here, empty, so the
//! producer registry and the route need no change when it lands.

use ariadne_api::attention::AttentionItemDto;
use ariadne_store::{Result, Store};

pub(crate) async fn items(_store: &Store) -> Result<Vec<AttentionItemDto>> {
    Ok(Vec::new())
}
