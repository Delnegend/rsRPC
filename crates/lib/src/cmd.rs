use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;
use std::borrow::Cow;
use std::collections::HashMap;

/// Cutoff used by [`ActivityCmd::fix`] to decide whether a timestamp is
/// already in milliseconds: values above "now + 100 years (in seconds)" are
/// treated as milliseconds, anything smaller as seconds.
const TIMESTAMP_ALREADY_MS_CUTOFF_OFFSET_SECS: i64 = 100 * 365 * 24 * 3600;

/// Flag value set by [`ActivityCmd::fix`] when `instance` is true but no
/// explicit `flags` value is present.
const INSTANCE_FLAG_DEFAULT: u32 = 1;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ActivityPayload {
    pub activity: Option<Activity>,
    pub pid: Option<u64>,
    #[serde(rename = "socketId")]
    pub socket_id: Option<String>,
}

#[skip_serializing_none]
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct ActivityCmd {
    pub application_id: Option<String>,
    pub cmd: String,
    pub args: Option<ActivityCmdArgs>,
    pub data: Option<HashMap<String, String>>,
    pub evt: Option<String>,
    pub nonce: Value,
}

impl ActivityCmd {
    pub fn empty() -> Self {
        Self {
            application_id: None,
            cmd: "".to_string(),
            args: None,
            data: None,
            evt: None,
            nonce: Value::String("".to_string()),
        }
    }

    pub fn fix(&mut self) {
        self.fix_timestamps();
        self.fix_buttons();
        self.fix_flags();
    }

    /// The command nonce as plain text.
    ///
    /// String nonces are returned unquoted (`Value::String("abc")` yields
    /// `abc`); every other JSON value falls back to its JSON rendering.
    pub fn nonce_str(&self) -> Cow<'_, str> {
        match &self.nonce {
            Value::String(inner) => Cow::Borrowed(inner.as_str()),
            other => Cow::Owned(other.to_string()),
        }
    }

    fn fix_timestamps(&mut self) {
        if let Some(timestamps) = self
            .activity_mut()
            .and_then(|activity| activity.timestamps.as_mut())
        {
            let cur = chrono::Utc::now().timestamp() + TIMESTAMP_ALREADY_MS_CUTOFF_OFFSET_SECS;

            // convert starting timestamp
            if let Some(start) = timestamps.start.as_mut() {
                // convert timestamp if in seconds
                if start.as_millis() > cur {
                    *start = TimeoutValue::new(start.as_millis());
                } else {
                    *start = TimeoutValue::new(start.as_millis() * 1000);
                }
            }

            // convert ending timestamp
            if let Some(end) = timestamps.end.as_mut() {
                // convert timestamp if in seconds
                if end.as_millis() > cur {
                    *end = TimeoutValue::new(end.as_millis());
                } else {
                    *end = TimeoutValue::new(end.as_millis() * 1000);
                }
            }
        }
    }

    fn fix_buttons(&mut self) {
        // If `buttons` are an array of objects, we need to map the labels to `buttons` (as a string array) and the urls to `metadata.button_urls` (as an array of strings)
        if let Some(activity) = self.activity_mut() {
            if let Some(buttons) = activity.buttons.as_mut() {
                let mut button_urls: Vec<String> = vec![];
                let mut button_labels: Vec<Value> = vec![];

                for b in buttons {
                    if let Some(label) = b.get("label") {
                        // Unless the provider of the actvity REALLY screwed up, we can safely assume this is a string
                        button_labels.push(label.clone());
                    }
                    if let Some(url) = b.get("url") {
                        let text = match url.as_str() {
                            Some(text) => text.to_string(),
                            None => String::new(),
                        };
                        button_urls.push(text);
                    }
                }

                activity.metadata.get_or_insert_default().button_urls = Some(button_urls);

                activity.buttons = Some(button_labels);
            }
        }
    }

    fn fix_flags(&mut self) {
        if let Some(activity) = self.activity_mut() {
            if matches!(activity.instance, Some(true)) && activity.flags.is_none() {
                activity.flags = Some(INSTANCE_FLAG_DEFAULT);
            }
        }
    }

    fn activity_mut(&mut self) -> Option<&mut Activity> {
        self.args.as_mut().and_then(|args| args.activity.as_mut())
    }
}

#[skip_serializing_none]
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct ActivityCmdArgs {
    pub pid: Option<u64>,
    pub activity: Option<Activity>,
    // For INVITE_BROWSER
    pub code: Option<String>,
}

#[skip_serializing_none]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Party {
    pub id: Option<String>,
    pub size: Option<Vec<u32>>,
}

#[skip_serializing_none]
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct Assets {
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
}

#[skip_serializing_none]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Secrets {
    pub join: Option<String>,
}

#[skip_serializing_none]
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct Emoji {
    pub name: Option<String>,
    pub id: Option<String>,
    pub animated: Option<bool>,
}

// Important: https://docs.discord.sex/resources/presence#activity-metadata-object
#[skip_serializing_none]
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct Metadata {
    pub button_urls: Option<Vec<String>>,
    pub artist_ids: Option<Vec<String>>,
    pub album_id: Option<String>,
    pub context_uri: Option<String>,
}

#[skip_serializing_none]
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct Activity {
    pub id: Option<String>,
    pub name: Option<String>,
    pub buttons: Option<Vec<Value>>,
    #[serde(default)]
    pub r#type: u32,
    pub url: Option<String>,
    pub created_at: Option<u64>,
    pub session_id: Option<String>,
    pub platform: Option<String>,
    pub supported_platforms: Option<Vec<String>>,
    pub timestamps: Option<Timestamps>,
    pub application_id: Option<String>,
    pub details: Option<String>,
    pub state: Option<String>,
    pub sync_id: Option<String>,
    pub instance: Option<bool>,
    pub flags: Option<u32>,
    pub emoji: Option<Emoji>,
    pub party: Option<Party>,
    pub assets: Option<Assets>,
    pub secrets: Option<Secrets>,
    pub metadata: Option<Metadata>,
}

#[skip_serializing_none]
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct TimeoutValue(i64);

impl TimeoutValue {
    /// Wrap a millisecond timestamp.
    pub fn new(millis: i64) -> Self {
        Self(millis)
    }

    /// Read the wrapped millisecond timestamp.
    pub fn as_millis(&self) -> i64 {
        self.0
    }

    /// Read the wrapped millisecond timestamp.
    pub fn get(&self) -> i64 {
        self.0
    }
}

#[skip_serializing_none]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Timestamps {
    #[serde(default)]
    pub start: Option<TimeoutValue>,
    #[serde(default)]
    pub end: Option<TimeoutValue>,
}
