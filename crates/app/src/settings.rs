//! Settings persistence: JSON in the user config dir.
//!
//! `cv-core` owns the shape of the data (serde derives, `sanitize`); this module owns the file.
//! `enabled` is never written (see `AppState`), so the magnifier always starts off.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread::JoinHandle;
use std::time::Duration;

use cv_core::{AppState, SharedState};

const DIR_NAME: &str = "clear-view";
const FILE_NAME: &str = "settings.json";
const POLL: Duration = Duration::from_millis(250);

/// `%APPDATA%\clear-view\settings.json` on Windows. `None` if the config dir cannot be found.
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(DIR_NAME).join(FILE_NAME))
}

/// Load settings, falling back to defaults. Never fails: problems are logged to stderr.
pub fn load() -> AppState {
    match config_path() {
        Some(path) => load_from(&path),
        None => {
            eprintln!("[settings] no config dir; settings will not be saved");
            AppState::default()
        }
    }
}

fn load_from(path: &Path) -> AppState {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return AppState::default(),
        Err(e) => {
            eprintln!("[settings] cannot read {}: {e}; using defaults", path.display());
            return AppState::default();
        }
    };

    match serde_json::from_str::<AppState>(&text) {
        Ok(mut state) => {
            state.sanitize();
            state
        }
        Err(e) => {
            // Keep the unreadable file instead of overwriting it with defaults.
            let bad = path.with_extension("json.bad");
            match std::fs::rename(path, &bad) {
                Ok(()) => eprintln!(
                    "[settings] {} is invalid ({e}); moved to {}, using defaults",
                    path.display(),
                    bad.display()
                ),
                Err(re) => eprintln!(
                    "[settings] {} is invalid ({e}) and could not be moved ({re}); using defaults",
                    path.display()
                ),
            }
            AppState::default()
        }
    }
}

/// Write via a temp file and rename, so a kill mid-write cannot leave a half-written file.
fn save_to(path: &Path, state: &AppState) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// The state as it would be saved: `enabled` is not persisted, so ignore it when comparing.
fn persisted_view(state: &SharedState) -> AppState {
    let mut s = state.read().clone();
    s.enabled = false;
    s
}

/// Saver thread: every `POLL`, writes the state if it differs from what was last written,
/// and once more after `shutdown` is set. Join it from `main` after the UI returns.
pub fn spawn_saver(state: SharedState, shutdown: Arc<AtomicBool>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let Some(path) = config_path() else { return };

        // If there is no file yet, the first pass creates it. Otherwise the state we hold is
        // what was just loaded, so there is nothing to write until it changes.
        let mut last: Option<AppState> = path.exists().then(|| persisted_view(&state));

        loop {
            let stop = shutdown.load(Ordering::Relaxed);
            let now = persisted_view(&state);
            if last.as_ref() != Some(&now) {
                if let Err(e) = save_to(&path, &now) {
                    eprintln!("[settings] cannot save {}: {e}", path.display());
                }
                // Recorded even on failure, so a persistent error logs once per change, not per tick.
                last = Some(now);
            }
            if stop {
                break;
            }
            std::thread::sleep(POLL);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_core::{ColorFilter, DisplayMode, Edge};

    /// A fresh empty directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("clear-view-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn file(&self) -> PathBuf {
            self.0.join(FILE_NAME)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = TempDir::new("missing");
        assert_eq!(load_from(&dir.file()), AppState::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new("roundtrip");
        let state = AppState {
            zoom: 3.5,
            color_filter: ColorFilter::Greyscale,
            display_mode: DisplayMode::Docked(Edge::Bottom),
            tts_rate: 4,
            ..AppState::default()
        };
        save_to(&dir.file(), &state).unwrap();
        assert_eq!(load_from(&dir.file()), state);
        assert!(!dir.0.join("settings.json.tmp").exists());
    }

    #[test]
    fn save_creates_missing_directory() {
        let dir = TempDir::new("mkdir");
        let path = dir.0.join("nested").join(FILE_NAME);
        save_to(&path, &AppState::default()).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn corrupt_file_is_kept_as_bad_and_defaults_used() {
        let dir = TempDir::new("corrupt");
        std::fs::write(dir.file(), "{ this is not json").unwrap();
        assert_eq!(load_from(&dir.file()), AppState::default());
        assert!(!dir.file().exists());
        assert_eq!(
            std::fs::read_to_string(dir.0.join("settings.json.bad")).unwrap(),
            "{ this is not json"
        );
    }

    #[test]
    fn out_of_range_values_are_clamped_on_load() {
        let dir = TempDir::new("clamp");
        std::fs::write(dir.file(), r#"{"zoom": 99, "panel_size": 0}"#).unwrap();
        let s = load_from(&dir.file());
        assert_eq!(s.zoom, 20.0);
        assert_eq!(s.panel_size, 10);
    }

    #[test]
    fn enabled_is_not_saved_or_loaded() {
        let dir = TempDir::new("enabled");
        save_to(&dir.file(), &AppState { enabled: true, ..AppState::default() }).unwrap();
        // Exact key: "tts_enabled" legitimately appears in the file.
        assert!(!std::fs::read_to_string(dir.file()).unwrap().contains("\"enabled\""));
        assert!(!load_from(&dir.file()).enabled);
    }
}
