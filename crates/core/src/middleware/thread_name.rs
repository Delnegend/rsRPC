//! Generic game engine thread name filter.

use super::ActivityMiddleware;
use crate::cmd::Activity;

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
