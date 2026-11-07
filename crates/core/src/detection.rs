use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub struct DetectableActivity {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub executables: Option<Vec<Executable>>,
    #[serde(default)]
    pub hook: bool,
    pub pid: Option<u64>,
    pub timestamp: Option<String>,
    pub details: Option<String>,
    pub state: Option<String>,
}

/// Activity id used by synthetic "no activity" markers: the process scanner
/// forwards one when nothing is detected, and connectors treat it as "clear
/// the current presence".
pub const NULL_ACTIVITY_ID: &str = "null";

/// Build the synthetic "no activity" [`DetectableActivity`].
pub fn null_activity() -> DetectableActivity {
    DetectableActivity {
        id: NULL_ACTIVITY_ID.to_string(),
        name: String::new(),
        executables: None,
        hook: false,
        pid: None,
        timestamp: None,
        details: None,
        state: None,
    }
}

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub struct Executable {
    #[serde(default)]
    pub is_launcher: bool,
    pub name: String,
    pub os: String,
    pub arguments: Option<String>,
}
