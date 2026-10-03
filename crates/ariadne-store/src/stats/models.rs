//! The `models` family: which model does the job.

use super::StatsFilter;
use crate::{Result, Store};

/// What the `models` family answers. Empty until its task fills it in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelStats {}

impl Store {
    /// Which model does the job, over the facts the filter keeps.
    pub async fn models_stats(&self, _filter: &StatsFilter) -> Result<ModelStats> {
        Ok(ModelStats::default())
    }
}
