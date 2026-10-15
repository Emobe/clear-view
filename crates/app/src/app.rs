use std::sync::{Arc, Mutex, OnceLock};

use cv_core::{
    AppState, ColorFilter, DisplayMode, Edge, Interpolation, PANEL_SIZE_MAX, PANEL_SIZE_MIN,
    POINTER_OUTLINE_MAX, POINTER_SIZE_MAX, POINTER_SIZE_MIN, SharedState, ZOOM_MAX, ZOOM_MIN,
};
use eframe::egui;

use crate::platform::TOGGLE_LABEL;

/// Filled with the egui context once the window exists. Threads that change state the panel
/// shows (the hotkey thread flips `enabled`) call `request_repaint()` on it afterwards.
pub type RepaintSlot = Arc<OnceLock<egui::Context>>;

/// One line per binding that failed to register, for the settings panel. Filled once at startup
/// by the hotkey thread; never saved.
pub type HotkeyFailures = Arc<Mutex<Vec<String>>>;

pub struct ClearViewApp {
    state: SharedState,
    hotkey_failures: HotkeyFailures,
    /// A changed `tts_*` field wakes the speech thread (ADR 0013).
    #[cfg(feature = "tts")]
    speech: cv_tts::SpeechHandle,
    /// The voice dropdown was open last frame, so opening it reads the voice list again.
    #[cfg(feature = "tts")]
    voice_list_open: bool,
}

/// What "Test voice" says: long enough to try Stop and the silence key on.
#[cfg(feature = "tts")]
const TEST_SENTENCE: &str = "This is a test of the clear view voice. It goes on for a few seconds so that there is time to press Stop, or the Control key, and hear it end at once.";

impl ClearViewApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        state: SharedState,
        repaint: RepaintSlot,
        hotkey_failures: HotkeyFailures,
        #[cfg(feature = "tts")] speech: cv_tts::SpeechHandle,
    ) -> Self {
        // egui only repaints on input or an explicit request, so the panel does not poll.
        // Anything that changes displayed state from another thread must wake it via `repaint`.
        let _ = repaint.set(cc.egui_ctx.clone());
        Self {
            state,
            hotkey_failures,
            #[cfg(feature = "tts")]
            speech,
            #[cfg(feature = "tts")]
            voice_list_open: false,
        }
    }

    /// The voice dropdown and its note (ADR 0012). Usable only while speech is on, because the
    /// engines (and so the list) exist only then. A pick is saved in `tts_voice`.
    #[cfg(feature = "tts")]
    fn voice_picker(&mut self, ui: &mut egui::Ui, s: &mut AppState) {
        use cv_core::voice::{SpeechState, voice_label, voice_note};

        let status = self.speech.status();
        let speaking = status
            .speaking_with
            .as_ref()
            .and_then(|id| status.voices.iter().find(|v| &v.id == id));
        let current = speaking.map_or_else(|| "Default voice".to_string(), voice_label);
        let mut open = false;
        ui.add_enabled_ui(s.tts_enabled && status.state == SpeechState::On, |ui| {
            let shown = egui::ComboBox::from_id_salt("tts_voice")
                .selected_text(current)
                .width(220.0)
                .show_ui(ui, |ui| {
                    for voice in &status.voices {
                        let picked = speaking.is_some_and(|v| v.id == voice.id);
                        if ui.selectable_label(picked, voice_label(voice)).clicked() {
                            s.tts_voice = Some(voice.id.clone());
                        }
                    }
                });
            open = shown.inner.is_some();
        });
        // Rising edge only: the list is read when it is opened, not polled.
        if open && !self.voice_list_open {
            self.speech.refresh_voices();
        }
        self.voice_list_open = open;
        // With speech switched off the thread publishes Off, whose note says to turn it on.
        if let Some(note) = voice_note(&status) {
            ui.colored_label(egui::Color32::GRAY, note);
        }
    }
}

impl eframe::App for ClearViewApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            // The panel is taller than a small window once everything is shown (docked, smooth
            // edges, pointer settings), so it scrolls instead of cutting controls off.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.heading("clear-view");
                ui.separator();

                // Draw against a local copy so no lock is held while egui runs. Only fields the
                // panel changed are written back, so a hotkey toggle of `enabled` is never overwritten.
                let mut s = self.state.read().clone();
                let before = s.clone();

                // Enable / disable toggle
                ui.horizontal(|ui| {
                    ui.label("Magnifier");
                    let label = if s.enabled { "On" } else { "Off" };
                    if ui.button(label).clicked() {
                        s.enabled = !s.enabled;
                    }
                    ui.label(format!("  ({TOGGLE_LABEL} to toggle)"));
                });

                // Set once at startup by the hotkey thread; a lock failure just hides the lines.
                if let Ok(failures) = self.hotkey_failures.lock() {
                    for line in failures.iter() {
                        ui.colored_label(egui::Color32::LIGHT_RED, format!("Hotkey {line}"));
                    }
                }

                ui.add_space(8.0);

                // Zoom slider
                ui.add(
                    egui::Slider::new(&mut s.zoom, ZOOM_MIN..=ZOOM_MAX)
                        .step_by(0.1)
                        .text("Zoom"),
                );

                ui.add_space(4.0);

                // Smooth follow speed
                ui.add(
                    egui::Slider::new(&mut s.smooth_speed, 0.01..=1.0)
                        .step_by(0.01)
                        .text("Follow speed"),
                );

                ui.add_space(8.0);

                // Display mode
                ui.label("Display mode");
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut s.display_mode, DisplayMode::Fullscreen,        "Fullscreen");
                    ui.radio_value(&mut s.display_mode, DisplayMode::Docked(Edge::Top),    "Top");
                    ui.radio_value(&mut s.display_mode, DisplayMode::Docked(Edge::Bottom), "Bottom");
                    ui.radio_value(&mut s.display_mode, DisplayMode::Docked(Edge::Left),   "Left");
                    ui.radio_value(&mut s.display_mode, DisplayMode::Docked(Edge::Right),  "Right");
                });

                // Panel size — only shown when docked
                if s.display_mode != DisplayMode::Fullscreen {
                    ui.add_space(4.0);
                    ui.add(
                        egui::Slider::new(&mut s.panel_size, PANEL_SIZE_MIN..=PANEL_SIZE_MAX)
                            .text("Panel size (%)"),
                    );
                    // Off: the panel stays on the primary monitor (roadmap 5.2).
                    ui.checkbox(
                        &mut s.panel_follows_monitor,
                        "Move the panel to the monitor being magnified",
                    );
                }

                ui.add_space(8.0);

                // Colour filter
                ui.label("Colour filter");
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut s.color_filter, ColorFilter::None,              "None");
                    ui.radio_value(&mut s.color_filter, ColorFilter::Inverted,          "Inverted");
                    ui.radio_value(&mut s.color_filter, ColorFilter::Greyscale,         "Greyscale");
                    ui.radio_value(&mut s.color_filter, ColorFilter::GreyscaleInverted, "Grey+Inv");
                });

                ui.add_space(8.0);

                // Interpolation selector
                ui.label("Interpolation");
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut s.interpolation, Interpolation::Bilinear,  "Bilinear");
                    ui.radio_value(&mut s.interpolation, Interpolation::Bicubic,   "Bicubic");
                    ui.radio_value(&mut s.interpolation, Interpolation::Sharp,     "Sharp");
                    ui.radio_value(&mut s.interpolation, Interpolation::CleanEdge, "Smooth edges");
                });

                // cleanEdge similarity threshold — only shown for that mode
                if s.interpolation == Interpolation::CleanEdge {
                    ui.add_space(4.0);
                    ui.add(
                        egui::Slider::new(&mut s.edge_threshold, 0.0..=1.0)
                            .step_by(0.01)
                            .text("Edge threshold"),
                    );
                }

                ui.add_space(8.0);

                // Pointer in the magnified view (ADR 0010). Off: the circle, which has no settings.
                ui.label("Pointer");
                ui.checkbox(&mut s.pointer_shape, "Draw the real pointer shape (off: circle)");
                ui.add_enabled_ui(s.pointer_shape, |ui| {
                    ui.add(
                        egui::Slider::new(&mut s.pointer_size, POINTER_SIZE_MIN..=POINTER_SIZE_MAX)
                            .step_by(0.5)
                            .text("Pointer size"),
                    );
                    ui.add(
                        egui::Slider::new(&mut s.pointer_outline, 0.0..=POINTER_OUTLINE_MAX)
                            .step_by(0.5)
                            .text("Outline width (px)"),
                    );
                    ui.horizontal(|ui| {
                        ui.color_edit_button_srgb(&mut s.pointer_fill);
                        ui.label("Fill colour");
                    });
                    ui.horizontal(|ui| {
                        ui.color_edit_button_srgb(&mut s.pointer_outline_colour);
                        ui.label("Outline colour");
                    });
                });

                #[cfg(feature = "tts")]
                {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.heading("Speech");
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label("Speech");
                        let label = if s.tts_enabled { "On" } else { "Off" };
                        if ui.button(label).clicked() {
                            s.tts_enabled = !s.tts_enabled;
                        }
                    });

                    ui.add_space(4.0);
                    self.voice_picker(ui, &mut s);

                    ui.add_space(4.0);

                    ui.add_enabled(
                        s.tts_enabled,
                        egui::Slider::new(&mut s.tts_volume, 0..=100).text("Volume"),
                    );
                    ui.add_enabled(
                        s.tts_enabled,
                        egui::Slider::new(&mut s.tts_rate, -10..=10).text("Rate"),
                    );

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(s.tts_enabled, |ui| {
                            if ui.button("Test voice").clicked() {
                                self.speech.say(cv_core::Utterance::new(cv_core::Kind::Test, TEST_SENTENCE));
                            }
                            if ui.button("Stop").clicked() {
                                self.speech.silence();
                            }
                        });
                    });
                }

                if s != before {
                    apply_changes(&mut self.state.write(), &before, &s);
                    // After the lock is released: the thread reads the fields again.
                    #[cfg(feature = "tts")]
                    if s.tts_enabled != before.tts_enabled
                        || s.tts_volume != before.tts_volume
                        || s.tts_rate != before.tts_rate
                        || s.tts_voice != before.tts_voice
                    {
                        self.speech.wake();
                    }
                }
            });
        });
    }
}

/// Copies into `live` every field where `after` differs from `before`; other fields keep
/// whatever other threads wrote since `before` was taken.
fn apply_changes(live: &mut AppState, before: &AppState, after: &AppState) {
    macro_rules! merge {
        ($($field:ident),+ $(,)?) => {
            $(if after.$field != before.$field { live.$field = after.$field; })+
        };
    }
    merge!(
        enabled,
        zoom,
        smooth_speed,
        interpolation,
        edge_threshold,
        display_mode,
        panel_size,
        panel_follows_monitor,
        color_filter,
        pointer_shape,
        pointer_size,
        pointer_outline,
        pointer_fill,
        pointer_outline_colour,
        tts_enabled,
        tts_hover_enabled,
        tts_volume,
        tts_rate,
    );
    // Not `Copy`, so outside the macro.
    if after.tts_voice != before.tts_voice {
        live.tts_voice = after.tts_voice.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_field_is_copied() {
        let before = AppState::default();
        let after = AppState { zoom: 5.0, ..before.clone() };
        let mut live = before.clone();
        apply_changes(&mut live, &before, &after);
        assert_eq!(live, after);
    }

    #[test]
    fn unchanged_fields_keep_the_live_value() {
        // The hotkey flipped `enabled` after the panel took its snapshot.
        let before = AppState::default();
        let after = AppState { zoom: 5.0, ..before.clone() };
        let mut live = AppState { enabled: true, ..before.clone() };
        apply_changes(&mut live, &before, &after);
        assert!(live.enabled);
        assert_eq!(live.zoom, 5.0);
    }

    #[test]
    fn several_fields_at_once() {
        let before = AppState::default();
        let after = AppState {
            enabled: true,
            tts_voice: Some(cv_core::VoiceId { engine: cv_core::Engine::Sapi, id: "v".into() }),
            display_mode: DisplayMode::Docked(Edge::Left),
            panel_size: 20,
            panel_follows_monitor: true,
            color_filter: ColorFilter::Inverted,
            interpolation: Interpolation::CleanEdge,
            edge_threshold: 0.4,
            tts_rate: 3,
            ..before.clone()
        };
        let mut live = before.clone();
        apply_changes(&mut live, &before, &after);
        assert_eq!(live, after);
    }

    #[test]
    fn the_pointer_settings_are_copied() {
        let before = AppState::default();
        let after = AppState {
            pointer_shape: false,
            pointer_size: 4.0,
            pointer_outline: 5.5,
            pointer_fill: [1, 2, 3],
            pointer_outline_colour: [4, 5, 6],
            ..before.clone()
        };
        let mut live = AppState { enabled: true, ..before.clone() };
        apply_changes(&mut live, &before, &after);
        assert_eq!(live, AppState { enabled: true, ..after });
    }

    #[test]
    fn one_pointer_setting_leaves_the_others_live() {
        let before = AppState::default();
        let after = AppState { pointer_outline: 7.0, ..before.clone() };
        let mut live = AppState { pointer_size: 3.0, ..before.clone() };
        apply_changes(&mut live, &before, &after);
        assert_eq!(live.pointer_outline, 7.0);
        assert_eq!(live.pointer_size, 3.0);
    }

    #[test]
    fn no_change_leaves_live_alone() {
        let before = AppState::default();
        let mut live = AppState { zoom: 7.0, enabled: true, ..before.clone() };
        let expected = live.clone();
        apply_changes(&mut live, &before, &before.clone());
        assert_eq!(live, expected);
    }
}
