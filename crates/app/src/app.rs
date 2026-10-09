use std::sync::{Arc, Mutex, OnceLock};

use cv_core::{
    AppState, ColorFilter, DisplayMode, Edge, Interpolation, PANEL_SIZE_MAX, PANEL_SIZE_MIN,
    SharedState, ZOOM_MAX, ZOOM_MIN,
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
}

impl ClearViewApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        state: SharedState,
        repaint: RepaintSlot,
        hotkey_failures: HotkeyFailures,
    ) -> Self {
        // egui only repaints on input or an explicit request, so the panel does not poll.
        // Anything that changes displayed state from another thread must wake it via `repaint`.
        let _ = repaint.set(cc.egui_ctx.clone());
        Self { state, hotkey_failures }
    }
}

impl eframe::App for ClearViewApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
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

            #[cfg(feature = "tts")]
            {
                ui.add_space(8.0);
                ui.separator();
                ui.heading("Screen Reader");
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label("TTS");
                    let label = if s.tts_enabled { "On" } else { "Off" };
                    if ui.button(label).clicked() {
                        s.tts_enabled = !s.tts_enabled;
                    }
                });

                ui.add_space(4.0);

                ui.add_enabled(
                    s.tts_enabled,
                    egui::Checkbox::new(&mut s.tts_hover_enabled, "Hover echo"),
                );

                ui.add_space(4.0);

                ui.add_enabled(
                    s.tts_enabled,
                    egui::Slider::new(&mut s.tts_volume, 0..=100).text("Volume"),
                );
                ui.add_enabled(
                    s.tts_enabled,
                    egui::Slider::new(&mut s.tts_rate, -10..=10).text("Rate"),
                );
            }

            if s != before {
                apply_changes(&mut self.state.write(), &before, &s);
            }
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
        color_filter,
        tts_enabled,
        tts_hover_enabled,
        tts_volume,
        tts_rate,
    );
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
            display_mode: DisplayMode::Docked(Edge::Left),
            panel_size: 20,
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
    fn no_change_leaves_live_alone() {
        let before = AppState::default();
        let mut live = AppState { zoom: 7.0, enabled: true, ..before.clone() };
        let expected = live.clone();
        apply_changes(&mut live, &before, &before.clone());
        assert_eq!(live, expected);
    }
}
