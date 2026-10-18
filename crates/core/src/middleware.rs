//! Activity middleware pipeline.
//!
//! Provides an extensible pipeline to inspect, clean, and enrich Discord activities
//! before they are serialized and broadcast to connected Discord clients.

use std::sync::Arc;

use crate::cmd::Activity;

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

/// Filters out generic game engine thread names from activity `details` and `state`.
///
/// Engines such as Unreal Engine 5 set the main thread's name to `"GameThread"`,
/// which can leak into process scanners and window titles.
#[derive(Clone, Copy, Debug, Default)]
pub struct ThreadNameFilter;

impl ThreadNameFilter {
    fn is_generic_thread(s: &str) -> bool {
        matches!(
            s.trim(),
            "GameThread"
                | "RenderThread"
                | "RHIThread"
                | "AudioThread"
                | "MainThrd"
                | "MainThread"
                | "wine-preloader"
                | "wine64-preloader"
                | "wineserver"
        )
    }
}

impl ActivityMiddleware for ThreadNameFilter {
    fn process(&self, activity: &mut Activity) {
        if let Some(details) = &activity.details
            && Self::is_generic_thread(details)
        {
            activity.details = None;
        }
        if let Some(state) = &activity.state
            && Self::is_generic_thread(state)
        {
            activity.state = None;
        }
    }
}

/// Cleans promotional mod watermarks and version banners from activity assets.
///
/// For example, the Risk of Rain 2 community mod sets `large_text` to
/// `"DiscordRichPresence v1.3.5"`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModWatermarkFilter;

impl ModWatermarkFilter {
    fn is_mod_watermark(s: &str) -> bool {
        let lower = s.trim().to_lowercase();
        lower.starts_with("discordrichpresence")
            || lower.starts_with("discord-rpc")
            || lower.starts_with("discordrpc")
            || lower.starts_with("discord rich presence")
    }
}

impl ActivityMiddleware for ModWatermarkFilter {
    fn process(&self, activity: &mut Activity) {
        if let Some(assets) = activity.assets.as_mut() {
            if let Some(large_text) = &assets.large_text
                && Self::is_mod_watermark(large_text)
            {
                assets.large_text = None;
            }
            if let Some(small_text) = &assets.small_text
                && Self::is_mod_watermark(small_text)
            {
                assets.small_text = None;
            }
        }
    }
}
