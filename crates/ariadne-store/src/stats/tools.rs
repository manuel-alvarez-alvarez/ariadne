//! The `tools` family: what the agents do.

use super::StatsFilter;
use crate::{Result, Store};

/// What the `tools` family answers. Empty until its task fills it in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolStats {}

impl Store {
    /// What the agents do, over the facts the filter keeps.
    pub async fn tools_stats(&self, _filter: &StatsFilter) -> Result<ToolStats> {
        Ok(ToolStats::default())
    }
}
