//! Activity middleware pipeline.
//!
//! Provides an extensible pipeline to inspect, clean, and enrich Discord activities
//! before they are serialized and broadcast to connected Discord clients.

use std::sync::Arc;

use crate::cmd::Activity;

mod mod_watermark;
mod thread_name;

pub use mod_watermark::ModWatermarkFilter;
pub use thread_name::ThreadNameFilter;

/// Middleware that inspects or mutates an [`Activity`] before dispatch.
pub trait ActivityMiddleware: Send + Sync {
    /// Process and optionally mutate the given activity.
    fn process(&self, activity: &mut Activity);
}

/// Ordered pipeline of activity middlewares.
#[derive(Clone)]
pub struct MiddlewarePipeline {
    middlewares: Vec<Arc<dyn ActivityMiddleware>>,
}

impl MiddlewarePipeline {
    /// Create the middleware pipeline with all declared middlewares included.
    pub fn new() -> Self {
        Self {
            middlewares: vec![Arc::new(ThreadNameFilter), Arc::new(ModWatermarkFilter)],
        }
    }

    /// Run all middlewares in sequence on the given activity.
    pub fn process(&self, activity: &mut Activity) {
        for mw in &self.middlewares {
            mw.process(activity);
        }
    }
}

impl Default for MiddlewarePipeline {
    fn default() -> Self {
        Self::new()
    }
}
