//! The default hotkey bindings as data, and the lone-Ctrl silence rule (ADR 0014, roadmap 8.6a).
//!
//! Platform-neutral. A backend maps `Key` to its own key codes and registers `DEFAULT_BINDINGS`;
//! the rule that turns Ctrl presses into `Action::Silence` is `SilenceKey`, which the backend
//! feeds from whatever it observes (raw input on Windows, 8.6b).

use crate::Action;

/// A key a binding can use. Neutral: the backend maps it to a virtual key or keycode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// An upper-case letter, `'A'` to `'Z'`.
    Letter(char),
    Up,
    Down,
    PageUp,
    PageDown,
}

impl Key {
    /// The name used in labels, for example `Z` or `PageUp`.
    pub fn name(self) -> String {
        match self {
            Key::Letter(c) => c.to_string(),
            Key::Up => "Up".to_string(),
            Key::Down => "Down".to_string(),
            Key::PageUp => "PageUp".to_string(),
            Key::PageDown => "PageDown".to_string(),
        }
    }
}

/// Ctrl+Alt+Shift plus one key. Every binding uses that modifier set (ADR 0005, 0014), so a
/// chord carries only its key; a Caps Lock layer or a key picker would add a field in a new ADR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
}

/// One hotkey: what it does and how to show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub chord: Chord,
    pub action: Action,
    /// The chord as shown to the user, for example `Ctrl+Alt+Shift+Z`.
    pub label: &'static str,
    /// A speech command: registered only when the backend is told to register speech keys
    /// (the `tts` feature).
    pub speech: bool,
}

const fn letter(c: char) -> Chord {
    Chord { key: Key::Letter(c) }
}

// The Win key is avoided: the shell owns those combinations (Win+= is Windows Magnifier).
// Silence is not here: it is a lone Ctrl, not a chord (`SilenceKey`).
pub const DEFAULT_BINDINGS: [Binding; 4] = [
    Binding { chord: letter('Z'), action: Action::Toggle, label: "Ctrl+Alt+Shift+Z", speech: false },
    Binding {
        chord: Chord { key: Key::Up },
        action: Action::ZoomIn,
        label: "Ctrl+Alt+Shift+Up",
        speech: false,
    },
    Binding {
        chord: Chord { key: Key::Down },
        action: Action::ZoomOut,
        label: "Ctrl+Alt+Shift+Down",
        speech: false,
    },
    Binding { chord: letter('S'), action: Action::ToggleSpeech, label: "Ctrl+Alt+Shift+S", speech: true },
];

/// The magnifier on/off binding, for the settings panel.
pub const TOGGLE_LABEL: &str = DEFAULT_BINDINGS[0].label;

/// A key the silence rule cares about. Everything but the two Ctrl keys is `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyId {
    LeftCtrl,
    RightCtrl,
    Other,
}

/// The silence rule (ADR 0014): a Ctrl going down while no Ctrl was held asks for silence once.
/// Auto-repeat of a held Ctrl, a second Ctrl pressed while one is held, a Ctrl release and every
/// other key ask for nothing. Observe only: the key is never swallowed.
#[derive(Debug, Default)]
pub struct SilenceKey {
    left: bool,
    right: bool,
}

impl SilenceKey {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one key event; returns true when speech should be silenced now.
    pub fn feed(&mut self, key: KeyId, down: bool) -> bool {
        let held = match key {
            KeyId::LeftCtrl => &mut self.left,
            KeyId::RightCtrl => &mut self.right,
            KeyId::Other => return false,
        };
        if !down {
            *held = false;
            return false;
        }
        let was_held = self.left || self.right;
        match key {
            KeyId::LeftCtrl => self.left = true,
            _ => self.right = true,
        }
        !was_held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_bindings_share_a_chord() {
        for (i, a) in DEFAULT_BINDINGS.iter().enumerate() {
            for b in &DEFAULT_BINDINGS[i + 1..] {
                assert_ne!(a.chord, b.chord, "{} and {}", a.label, b.label);
            }
        }
    }

    #[test]
    fn every_label_matches_its_chord() {
        for b in &DEFAULT_BINDINGS {
            assert_eq!(b.label, format!("Ctrl+Alt+Shift+{}", b.chord.key.name()));
        }
    }

    #[test]
    fn speech_bindings_are_marked_speech() {
        for b in &DEFAULT_BINDINGS {
            let is_speech = matches!(b.action, Action::Silence | Action::ToggleSpeech);
            assert_eq!(b.speech, is_speech, "{}", b.label);
        }
    }

    #[test]
    fn silence_is_not_a_chord() {
        assert!(DEFAULT_BINDINGS.iter().all(|b| b.action != Action::Silence));
    }

    #[test]
    fn letters_are_upper_case_a_to_z() {
        for b in &DEFAULT_BINDINGS {
            if let Key::Letter(c) = b.chord.key {
                assert!(c.is_ascii_uppercase(), "{}", b.label);
            }
        }
    }

    #[test]
    fn the_toggle_label_is_the_first_binding_and_is_the_toggle() {
        assert_eq!(DEFAULT_BINDINGS[0].action, Action::Toggle);
        assert_eq!(TOGGLE_LABEL, "Ctrl+Alt+Shift+Z");
    }

    #[test]
    fn magnifier_bindings_are_unchanged() {
        let magnifier: Vec<_> = DEFAULT_BINDINGS.iter().filter(|b| !b.speech).map(|b| (b.label, b.action)).collect();
        assert_eq!(
            magnifier,
            [
                ("Ctrl+Alt+Shift+Z", Action::Toggle),
                ("Ctrl+Alt+Shift+Up", Action::ZoomIn),
                ("Ctrl+Alt+Shift+Down", Action::ZoomOut),
            ]
        );
    }

    #[test]
    fn the_first_ctrl_down_silences() {
        let mut s = SilenceKey::new();
        assert!(s.feed(KeyId::LeftCtrl, true));
        let mut s = SilenceKey::new();
        assert!(s.feed(KeyId::RightCtrl, true));
    }

    #[test]
    fn repeats_while_held_do_nothing() {
        let mut s = SilenceKey::new();
        assert!(s.feed(KeyId::LeftCtrl, true));
        assert!(!s.feed(KeyId::LeftCtrl, true));
        assert!(!s.feed(KeyId::LeftCtrl, true));
    }

    #[test]
    fn a_second_tap_after_release_silences_again() {
        let mut s = SilenceKey::new();
        assert!(s.feed(KeyId::LeftCtrl, true));
        assert!(!s.feed(KeyId::LeftCtrl, false));
        assert!(s.feed(KeyId::LeftCtrl, true));
    }

    #[test]
    fn left_and_right_held_together_silence_once() {
        let mut s = SilenceKey::new();
        assert!(s.feed(KeyId::LeftCtrl, true));
        assert!(!s.feed(KeyId::RightCtrl, true));
        // Releasing one while the other is held does not arm a new silence.
        assert!(!s.feed(KeyId::LeftCtrl, false));
        assert!(!s.feed(KeyId::RightCtrl, true));
        assert!(!s.feed(KeyId::RightCtrl, false));
        assert!(s.feed(KeyId::RightCtrl, true));
    }

    #[test]
    fn an_up_without_a_down_does_nothing() {
        let mut s = SilenceKey::new();
        assert!(!s.feed(KeyId::LeftCtrl, false));
        assert!(s.feed(KeyId::LeftCtrl, true));
    }

    #[test]
    fn other_keys_do_nothing_and_do_not_disturb_the_rule() {
        let mut s = SilenceKey::new();
        assert!(!s.feed(KeyId::Other, true));
        assert!(s.feed(KeyId::LeftCtrl, true));
        assert!(!s.feed(KeyId::Other, true));
        assert!(!s.feed(KeyId::Other, false));
        assert!(!s.feed(KeyId::LeftCtrl, true));
    }
}
