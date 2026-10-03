//! The `spend` family: what it spent.

use super::StatsFilter;
use crate::{Result, Store};

/// What the `spend` family answers. Empty until its task fills it in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpendStats {}

impl Store {
    /// What it spent, over the facts the filter keeps.
    pub async fn spend_stats(&self, _filter: &StatsFilter) -> Result<SpendStats> {
        Ok(SpendStats::default())
    }
}
