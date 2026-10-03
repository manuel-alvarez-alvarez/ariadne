//! The `attention` family: how much it needed a person.

use super::StatsFilter;
use crate::{Result, Store};

/// What the `attention` family answers. Empty until its task fills it in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AttentionStats {}

impl Store {
    /// How much it needed a person, over the facts the filter keeps.
    pub async fn attention_stats(&self, _filter: &StatsFilter) -> Result<AttentionStats> {
        Ok(AttentionStats::default())
    }
}
