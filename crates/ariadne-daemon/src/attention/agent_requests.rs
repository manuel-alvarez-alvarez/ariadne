//! The agent-request producer: a session's own question to the user.
//!
//! Not implemented yet — a later task gives it its eligibility rules (009)
//! and migrates the question surface onto it, preserving what it answers
//! today. Registered here, empty, so the producer registry and the route
//! need no change when it lands.

use ariadne_api::attention::AttentionItemDto;
use ariadne_store::{Result, Store};

pub(crate) async fn items(_store: &Store) -> Result<Vec<AttentionItemDto>> {
    Ok(Vec::new())
}
