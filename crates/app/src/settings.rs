//! Settings persistence: JSON in the user config dir.
//!
//! `cv-core` owns the shape of the data (serde derives, `sanitize`); this module owns the file.
//! `enabled` is never written (see `AppState`), so the magnifier always starts off.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread::JoinHandle;
use std::time::Duration;

use cv_core::voice::neutral_rate_from_legacy;
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

    match parse(&text) {
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

/// Reads a settings file, moving values written by older versions to where they live now.
///
/// Before roadmap 8.9 the speech rate was `tts_rate`, SAPI's -10 to 10. It is now the neutral
/// `tts_rate_neutral`, 0 to 100 (ADR 0012): the old value becomes `(old + 10) × 5`. A file that
/// already has the new key keeps it. The old key is dropped either way, so it is never written
/// again. A `tts_rate` that is not a whole number is ignored and the default applies.
fn parse(text: &str) -> serde_json::Result<AppState> {
    let mut value: serde_json::Value = serde_json::from_str(text)?;
    if let Some(map) = value.as_object_mut()
        && let Some(old) = map.remove("tts_rate")
        && !map.contains_key("tts_rate_neutral")
        && let Some(old) = old.as_i64()
    {
        let old = old.clamp(-10, 10) as i32;
        map.insert("tts_rate_neutral".into(), neutral_rate_from_legacy(old).into());
    }
    serde_json::from_value(value)
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
            panel_follows_monitor: true,
            tts_rate_neutral: 70,
            tts_volume: 30,
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
    fn pointer_settings_are_kept_across_a_save_and_load() {
        let dir = TempDir::new("pointer");
        let state = AppState {
            pointer_shape: false,
            pointer_size: 4.5,
            pointer_outline: 0.0,
            pointer_fill: [200, 10, 30],
            pointer_outline_colour: [0, 0, 0],
            ..AppState::default()
        };
        save_to(&dir.file(), &state).unwrap();
        assert_eq!(load_from(&dir.file()), state);
    }

    #[test]
    fn a_file_from_before_the_pointer_settings_loads_with_their_defaults() {
        let dir = TempDir::new("oldpointer");
        std::fs::write(dir.file(), r#"{"zoom": 6.0}"#).unwrap();
        let s = load_from(&dir.file());
        assert_eq!(s, AppState { zoom: 6.0, ..AppState::default() });
        assert!(s.pointer_shape);
    }

    #[test]
    fn out_of_range_pointer_values_are_clamped_on_load() {
        let dir = TempDir::new("pointerclamp");
        std::fs::write(dir.file(), r#"{"pointer_size": 99, "pointer_outline": -4}"#).unwrap();
        let s = load_from(&dir.file());
        assert_eq!(s.pointer_size, cv_core::POINTER_SIZE_MAX);
        assert_eq!(s.pointer_outline, 0.0);
    }

    fn load_text(name: &str, text: &str) -> AppState {
        let dir = TempDir::new(name);
        std::fs::write(dir.file(), text).unwrap();
        load_from(&dir.file())
    }

    #[test]
    fn an_old_rate_is_moved_to_the_neutral_scale() {
        // ADR 0012: (old + 10) x 5.
        for (old, neutral) in [(-10, 0), (0, 50), (4, 70), (10, 100)] {
            let s = load_text("oldrate", &format!(r#"{{"tts_rate": {old}}}"#));
            assert_eq!(s.tts_rate_neutral, neutral, "old rate {old}");
        }
    }

    #[test]
    fn an_out_of_range_or_odd_old_rate_does_not_stop_the_file_loading() {
        assert_eq!(load_text("rate-hi", r#"{"tts_rate": 99}"#).tts_rate_neutral, 100);
        assert_eq!(load_text("rate-lo", r#"{"tts_rate": -99}"#).tts_rate_neutral, 0);
        let default = AppState::default().tts_rate_neutral;
        assert_eq!(load_text("rate-text", r#"{"tts_rate": "fast"}"#).tts_rate_neutral, default);
        assert_eq!(load_text("rate-float", r#"{"tts_rate": 2.5}"#).tts_rate_neutral, default);
        assert_eq!(load_text("rate-null", r#"{"tts_rate": null}"#).tts_rate_neutral, default);
    }

    #[test]
    fn a_new_rate_wins_over_an_old_one() {
        let s = load_text("bothrates", r#"{"tts_rate": 10, "tts_rate_neutral": 20}"#);
        assert_eq!(s.tts_rate_neutral, 20);
    }

    #[test]
    fn a_new_rate_above_the_range_is_clamped_on_load() {
        assert_eq!(load_text("rate-new-hi", r#"{"tts_rate_neutral": 500}"#).tts_rate_neutral, 100);
    }

    #[test]
    fn a_file_without_a_rate_gets_the_normal_rate() {
        assert_eq!(load_text("norate", r#"{"zoom": 3.0}"#).tts_rate_neutral, 50);
    }

    #[test]
    fn a_file_with_the_removed_hover_setting_still_loads() {
        let s = load_text("hover", r#"{"zoom": 3.0, "tts_hover_enabled": false, "tts_enabled": true}"#);
        assert_eq!(s, AppState { zoom: 3.0, tts_enabled: true, ..AppState::default() });
    }

    #[test]
    fn the_old_keys_are_not_written_back() {
        let dir = TempDir::new("oldkeys");
        std::fs::write(dir.file(), r#"{"tts_rate": 4, "tts_hover_enabled": true}"#).unwrap();
        let state = load_from(&dir.file());
        save_to(&dir.file(), &state).unwrap();
        let text = std::fs::read_to_string(dir.file()).unwrap();
        assert!(!text.contains("\"tts_rate\""), "{text}");
        assert!(!text.contains("tts_hover_enabled"), "{text}");
        assert_eq!(load_from(&dir.file()).tts_rate_neutral, 70);
    }

    #[test]
    fn the_speech_settings_are_kept_across_a_save_and_load() {
        let dir = TempDir::new("speech");
        let state = AppState {
            tts_enabled: true,
            tts_volume: 35,
            tts_rate_neutral: 85,
            tts_voice: Some(cv_core::VoiceId { engine: cv_core::Engine::OneCore, id: "v".into() }),
            ..AppState::default()
        };
        save_to(&dir.file(), &state).unwrap();
        assert_eq!(load_from(&dir.file()), state);
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
