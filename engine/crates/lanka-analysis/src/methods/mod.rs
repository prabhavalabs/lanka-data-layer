//! The analysis method registry (11 §4, tier 1).

use std::sync::Arc;

use crate::method::AnalysisMethod;

pub mod fit;
pub mod group_compare;
pub mod regress;
pub mod relate;
pub mod simple;

#[derive(Clone)]
pub struct Registry {
    list: Vec<Arc<dyn AnalysisMethod>>,
}

impl Registry {
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            list: vec![
                Arc::new(simple::Describe),
                Arc::new(relate::Relate),
                Arc::new(regress::Regress),
                Arc::new(group_compare::GroupCompare),
                Arc::new(simple::Exposure),
                Arc::new(simple::Zonal),
                Arc::new(simple::Classify),
            ],
        }
    }

    /// Adds a method (distribution crates, 12 §8), replacing one with the
    /// same id.
    #[must_use]
    pub fn with(mut self, m: Arc<dyn AnalysisMethod>) -> Self {
        self.list.retain(|x| x.info().id != m.info().id);
        self.list.push(m);
        self
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<dyn AnalysisMethod>> {
        self.list.iter().find(|m| m.info().id == id).cloned()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn AnalysisMethod>> {
        self.list.iter()
    }
}
