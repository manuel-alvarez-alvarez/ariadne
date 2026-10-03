//! The `work` family: what got done.

use super::StatsFilter;
use crate::{Result, Store};

/// What the `work` family answers. Empty until its task fills it in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkStats {}

impl Store {
    /// What got done, over the facts the filter keeps.
    pub async fn work_stats(&self, _filter: &StatsFilter) -> Result<WorkStats> {
        Ok(WorkStats::default())
    }
}
