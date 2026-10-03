//! The `time` family: how long it takes.

use super::StatsFilter;
use crate::{Result, Store};

/// What the `time` family answers. Empty until its task fills it in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimeStats {}

impl Store {
    /// How long it takes, over the facts the filter keeps.
    pub async fn time_stats(&self, _filter: &StatsFilter) -> Result<TimeStats> {
        Ok(TimeStats::default())
    }
}
