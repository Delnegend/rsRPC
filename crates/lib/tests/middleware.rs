#![allow(
    clippy::assertions_on_constants,
    clippy::unreachable,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Tests for the activity middleware pipeline.

use rsrpc::cmd::{Activity, Assets};
use rsrpc::middleware::{
    ActivityMiddleware, MiddlewarePipeline, ModWatermarkFilter, ThreadNameFilter,
};

#[test]
fn thread_name_filter_clears_generic_engine_thread_names() {
    let filter = ThreadNameFilter;

    // 1. Details matching "GameThread" should be dropped
    let mut act = Activity {
        details: Some("GameThread".to_string()),
        state: Some("Playing Solo".to_string()),
        ..Default::default()
    };
    filter.process(&mut act);
    assert_eq!(act.details, None);
    assert_eq!(act.state.as_deref(), Some("Playing Solo"));

    // 2. RenderThread and other engine threads should also be dropped
    let mut act2 = Activity {
        details: Some("RenderThread".to_string()),
        state: Some("MainThrd".to_string()),
        ..Default::default()
    };
    filter.process(&mut act2);
    assert_eq!(act2.details, None);
    assert_eq!(act2.state, None);

    // 3. Legitimate game strings must be preserved
    let mut act3 = Activity {
        details: Some("Wave 5 - Hell on Earth".to_string()),
        state: Some("Outpost".to_string()),
        ..Default::default()
    };
    filter.process(&mut act3);
    assert_eq!(act3.details.as_deref(), Some("Wave 5 - Hell on Earth"));
    assert_eq!(act3.state.as_deref(), Some("Outpost"));
}

#[test]
fn mod_watermark_filter_clears_mod_promotions() {
    let filter = ModWatermarkFilter;

    // 1. Risk of Rain 2 mod watermark in large_text should be cleared
    let mut act = Activity {
        assets: Some(Assets {
            large_text: Some("DiscordRichPresence v1.3.5".to_string()),
            small_text: Some("Huntress".to_string()),
            large_image: Some("stage_1".to_string()),
            small_image: Some("char_huntress".to_string()),
        }),
        ..Default::default()
    };
    filter.process(&mut act);
    let assets = act.assets.unwrap();
    assert_eq!(assets.large_text, None);
    assert_eq!(assets.small_text.as_deref(), Some("Huntress"));
    assert_eq!(assets.large_image.as_deref(), Some("stage_1"));

    // 2. Other mod variations should also be cleared
    let mut act2 = Activity {
        assets: Some(Assets {
            large_text: Some("discord-rpc v2.0".to_string()),
            small_text: Some("Discord Rich Presence".to_string()),
            large_image: None,
            small_image: None,
        }),
        ..Default::default()
    };
    filter.process(&mut act2);
    let assets2 = act2.assets.unwrap();
    assert_eq!(assets2.large_text, None);
    assert_eq!(assets2.small_text, None);

    // 3. Legitimate map names must be preserved
    let mut act3 = Activity {
        assets: Some(Assets {
            large_text: Some("Titanic Plains".to_string()),
            small_text: Some("Commando".to_string()),
            large_image: None,
            small_image: None,
        }),
        ..Default::default()
    };
    filter.process(&mut act3);
    let assets3 = act3.assets.unwrap();
    assert_eq!(assets3.large_text.as_deref(), Some("Titanic Plains"));
    assert_eq!(assets3.small_text.as_deref(), Some("Commando"));
}

#[test]
fn default_pipeline_cleans_both_thread_names_and_watermarks() {
    let pipeline = MiddlewarePipeline::default_rules();

    let mut act = Activity {
        details: Some("GameThread".to_string()),
        state: Some("Wave 10".to_string()),
        assets: Some(Assets {
            large_text: Some("DiscordRichPresence v1.3.5".to_string()),
            small_text: Some("Huntress".to_string()),
            large_image: None,
            small_image: None,
        }),
        ..Default::default()
    };

    pipeline.process(&mut act);

    // Both should be cleaned simultaneously
    assert_eq!(act.details, None);
    assert_eq!(act.state.as_deref(), Some("Wave 10"));

    let assets = act.assets.unwrap();
    assert_eq!(assets.large_text, None);
    assert_eq!(assets.small_text.as_deref(), Some("Huntress"));
}

struct CustomPrefixMiddleware;

impl ActivityMiddleware for CustomPrefixMiddleware {
    fn process(&self, activity: &mut Activity) {
        if let Some(details) = activity.details.as_mut() {
            *details = format!("[MODIFIED] {details}");
        }
    }
}

#[test]
fn custom_middleware_can_be_added_to_pipeline() {
    let mut pipeline = MiddlewarePipeline::new();
    pipeline.add(CustomPrefixMiddleware);

    let mut act = Activity {
        details: Some("Playing".to_string()),
        ..Default::default()
    };

    pipeline.process(&mut act);
    assert_eq!(act.details.as_deref(), Some("[MODIFIED] Playing"));
}
