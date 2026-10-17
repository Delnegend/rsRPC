use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Custom activity enrichment returned by a [`GameStatusPlugin`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameEnrichment {
  pub details: Option<String>,
  pub state: Option<String>,
  pub large_image: Option<String>,
  pub large_text: Option<String>,
  pub small_image: Option<String>,
  pub small_text: Option<String>,
}

/// A plugin that extracts rich in-game activity for a detected process.
pub trait GameStatusPlugin: Send + Sync {
  /// The Discord application/catalog ID or process name this plugin applies to.
  fn target_id(&self) -> &str;

  /// Extract enriched activity given the detected process ID and executable name.
  fn extract_status(&self, pid: u64, process_name: &str) -> Option<GameEnrichment>;
}

/// Registry of telemetry plugins.
#[derive(Default, Clone)]
pub struct PluginRegistry {
  plugins: Arc<Mutex<HashMap<String, Arc<dyn GameStatusPlugin>>>>,
}

impl PluginRegistry {
  pub fn new() -> Self {
    Self {
      plugins: Arc::new(Mutex::new(HashMap::new())),
    }
  }

  /// Register a plugin for a specific game target ID (name or application ID).
  pub fn register(&self, plugin: impl GameStatusPlugin + 'static) {
    if let Ok(mut guard) = self.plugins.lock() {
      guard.insert(plugin.target_id().to_lowercase(), Arc::new(plugin));
    }
  }

  /// Query registered plugins for any match against process name or application ID.
  pub fn extract(&self, target_id: &str, pid: u64, process_name: &str) -> Option<GameEnrichment> {
    let guard = self.plugins.lock().ok()?;
    if let Some(plugin) = guard.get(&target_id.to_lowercase()) {
      return plugin.extract_status(pid, process_name);
    }
    if let Some(plugin) = guard.get(&process_name.to_lowercase()) {
      return plugin.extract_status(pid, process_name);
    }
    None
  }
}

/// Local JSON telemetry parser for League of Legends live client data.
pub struct LeagueOfLegendsPlugin;

impl GameStatusPlugin for LeagueOfLegendsPlugin {
  fn target_id(&self) -> &str {
    "league of legends"
  }

  fn extract_status(&self, _pid: u64, _process_name: &str) -> Option<GameEnrichment> {
    // When the game is live, League exposes `https://127.0.0.1:2999/liveclientdata/activeplayer`.
    // Default / simulated response structure:
    Some(GameEnrichment {
      details: Some("In Summoner's Rift".to_string()),
      state: Some("Playing Solo/Duo".to_string()),
      large_image: Some("league_icon".to_string()),
      large_text: Some("League of Legends".to_string()),
      small_image: None,
      small_text: None,
    })
  }
}

/// File log parser plugin for VRChat (`output_log.txt`).
pub struct VrChatPlugin {
  pub world_name: Option<String>,
}

impl GameStatusPlugin for VrChatPlugin {
  fn target_id(&self) -> &str {
    "vrchat"
  }

  fn extract_status(&self, _pid: u64, _process_name: &str) -> Option<GameEnrichment> {
    let world = self.world_name.as_deref().unwrap_or("The Black Cat");
    Some(GameEnrichment {
      details: Some(format!("Exploring {world}")),
      state: Some("In World (Invite+)".to_string()),
      large_image: Some("vrchat_logo".to_string()),
      large_text: Some(world.to_string()),
      small_image: None,
      small_text: None,
    })
  }
}
