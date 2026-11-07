//! Drag'n Wash custom game integration and live state detection.

use std::path::PathBuf;

use super::custom_game;
use crate::detection::DetectableActivity;

const DISCORD_APP_ID: &str = "1568491392664346654";
const STEAM_APP_ID: &str = "4739660";
const GAME_NAME: &str = "Drag'n Wash";

/// Returns the detectable activity definition for Drag'n Wash.
pub(super) fn game() -> DetectableActivity {
    custom_game(GAME_NAME, DISCORD_APP_ID, &[("win32", "DragNWash.exe")])
}

/// Dynamically enriches Drag'n Wash presence with "In-game" or "Main Menu".
pub(super) fn enrich(activity: &mut DetectableActivity) {
    if activity.name == GAME_NAME
        || activity.id == DISCORD_APP_ID
        || activity.id == STEAM_APP_ID
    {
        activity.details = Some(detect_status().to_string());
    }
}

pub(super) fn detect_status() -> &'static str {
    let Some(log_path) = find_player_log() else {
        return "Main Menu";
    };

    let Ok(content) = std::fs::read_to_string(log_path) else {
        return "Main Menu";
    };

    let mut status = "Main Menu";

    for line in content.lines() {
        if line.contains("Starting Dragon Walk Routine")
            || line.contains("Dragon state changed:")
            || line.contains("Running node ")
        {
            status = "In-game";
        } else if line.contains("Unloading 4 Unused Serialized files")
            || line.contains("Cleanup current backend")
            || is_scene_unload(line)
        {
            status = "Main Menu";
        }
    }

    status
}

fn is_scene_unload(line: &str) -> bool {
    if let Some(rest) = line.strip_prefix("Unloading ")
        && let Some(idx) = rest.find(" unused Assets")
        && let Ok(count) = rest[..idx].parse::<u32>()
    {
        return count >= 500;
    }
    false
}

fn find_player_log() -> Option<PathBuf> {
    // Windows standard path
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let p = PathBuf::from(profile)
            .join("AppData")
            .join("LocalLow")
            .join("Gator Dragon Games")
            .join("DragNWash")
            .join("Player.log");
        if p.is_file() {
            return Some(p);
        }
    }

    // Linux Proton / Wine paths via Steam library folders
    if let Ok(home) = std::env::var("HOME") {
        let home_path = PathBuf::from(home);
        let steam_roots = [
            home_path.join(".local/share/Steam"),
            home_path.join(".steam/steam"),
            home_path.join(".steam/root"),
        ];

        let mut steam_libs = Vec::new();
        for root in &steam_roots {
            if root.is_dir() && !steam_libs.contains(root) {
                steam_libs.push(root.clone());
            }
            let vdf = root.join("steamapps").join("libraryfolders.vdf");
            if let Ok(content) = std::fs::read_to_string(&vdf) {
                for line in content.lines() {
                    let parts: Vec<&str> = line.split('"').collect();
                    if parts.len() >= 4 && parts[1] == "path" {
                        let lib_dir = PathBuf::from(parts[3]);
                        if lib_dir.is_dir() && !steam_libs.contains(&lib_dir) {
                            steam_libs.push(lib_dir);
                        }
                    }
                }
            }
        }

        for lib in steam_libs {
            let log = lib
                .join("steamapps")
                .join("compatdata")
                .join(STEAM_APP_ID)
                .join("pfx")
                .join("drive_c")
                .join("users")
                .join("steamuser")
                .join("AppData")
                .join("LocalLow")
                .join("Gator Dragon Games")
                .join("DragNWash")
                .join("Player.log");
            if log.is_file() {
                return Some(log);
            }
        }
    }

    None
}
