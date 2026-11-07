//! Static custom games catalog and dynamic activity enrichers.

use crate::detection::{DetectableActivity, Executable, null_activity};

mod dragnwash;

/// Create a custom detectable game with executable names per operating system.
pub fn custom_game(name: &str, app_id: &str, executables: &[(&str, &str)]) -> DetectableActivity {
    DetectableActivity {
        id: app_id.to_string(),
        name: name.to_string(),
        hook: false,
        executables: Some(
            executables
                .iter()
                .map(|(os, exe)| Executable {
                    is_launcher: false,
                    os: os.to_string(),
                    name: exe.to_string(),
                    arguments: None,
                })
                .collect(),
        ),
        ..null_activity()
    }
}

/// Create a custom detectable game with optional CLI argument matching.
pub fn custom_game_with_args(
    name: &str,
    app_id: &str,
    executables: &[(&str, &str, Option<&str>)],
) -> DetectableActivity {
    DetectableActivity {
        id: app_id.to_string(),
        name: name.to_string(),
        hook: false,
        executables: Some(
            executables
                .iter()
                .map(|(os, exe, args)| Executable {
                    is_launcher: false,
                    os: os.to_string(),
                    name: exe.to_string(),
                    arguments: args.map(ToString::to_string),
                })
                .collect(),
        ),
        ..null_activity()
    }
}

/// Returns the static list of custom games embedded in the binary.
pub fn builtin_custom_games() -> Vec<DetectableActivity> {
    vec![dragnwash::game()]
}

/// Dynamically enrich recognized games with live in-game status (e.g. level, map, score).
pub fn enrich_custom_game(activity: &mut DetectableActivity) {
    dragnwash::enrich(activity);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_game_helper_populates_expected_fields() {
        let game = custom_game(
            "Test Game",
            "999888777",
            &[("linux", "test-game-bin"), ("win32", "test-game.exe")],
        );

        assert_eq!(game.name, "Test Game");
        assert_eq!(game.id, "999888777");
        assert!(!game.hook);

        let exes = game.executables.expect("executables should be present");
        assert_eq!(exes.len(), 2);
        assert_eq!(exes[0].os, "linux");
        assert_eq!(exes[0].name, "test-game-bin");
        assert_eq!(exes[0].arguments, None);
        assert!(!exes[0].is_launcher);
        assert_eq!(exes[1].os, "win32");
        assert_eq!(exes[1].name, "test-game.exe");
    }

    #[test]
    fn custom_game_with_args_helper() {
        let game = custom_game_with_args(
            "Game With Args",
            "111222333",
            &[("linux", "launcher", Some("--game main"))],
        );

        let exes = game.executables.expect("executables should be present");
        assert_eq!(exes[0].arguments.as_deref(), Some("--game main"));
    }
}
