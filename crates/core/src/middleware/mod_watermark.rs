//! Promotional mod watermark and version banner filter.

use super::ActivityMiddleware;
use crate::cmd::Activity;

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
