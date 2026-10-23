//! Application error adapter for Sav's context manager.
use crate::Result;
pub use sav::runtime::context::{
    Compaction, DEFAULT_CONTEXT_TOKENS, estimate_messages, estimate_request, estimate_tokens,
};
#[derive(Debug, Clone)]
pub struct ContextManager(pub(crate) sav::runtime::ContextManager);
impl ContextManager {
    pub fn for_model(limit: usize) -> Self {
        Self(sav::runtime::ContextManager::for_model(limit))
    }
    pub fn new(limit: usize, keep: usize) -> Self {
        Self(sav::runtime::ContextManager::new(limit, keep))
    }
    pub fn with_bounds(limit: usize, warmup: usize, keep: usize) -> Self {
        Self(sav::runtime::ContextManager::with_bounds(
            limit, warmup, keep,
        ))
    }
    pub fn prepare(
        &mut self,
        request: &mut sav::ModelRequest,
        reserve: u32,
    ) -> Result<Option<Compaction>> {
        self.0.prepare(request, reserve).map_err(Into::into)
    }
}
impl std::ops::Deref for ContextManager {
    type Target = sav::runtime::ContextManager;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ContextManager {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
