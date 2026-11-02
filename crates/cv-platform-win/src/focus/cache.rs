//! The dialog text cache (ADR 0018): the text found for each (foreground window, inner dialog)
//! pair, so a later read in the same dialog carries the same text, returning to a property page
//! does not walk it again, and the duplicate rule in cv-core's `FocusFilter` holds. Pure, tested
//! here.

/// How many pairs are kept; the oldest goes first. Starting value (ADR 0018).
pub(super) const PAIRS_MAX: usize = 8;

/// One pair's text. Empty for a window or page that is no dialog, says nothing or could not be
/// read; that answer is kept too, so the check runs once.
type Entry = (u64, Option<u64>, Vec<String>);

/// The kept pairs, oldest first.
#[derive(Debug, Default)]
pub(super) struct DialogCache {
    entries: Vec<Entry>,
}

impl DialogCache {
    /// The text kept for this pair.
    pub fn get(&self, window: u64, dialog: Option<u64>) -> Option<&[String]> {
        self.entries.iter().find(|e| e.0 == window && e.1 == dialog).map(|e| e.2.as_slice())
    }

    /// Keeps the text for this pair, replacing an older answer for it; the oldest pair goes when
    /// there are more than [`PAIRS_MAX`].
    pub fn put(&mut self, window: u64, dialog: Option<u64>, text: Vec<String>) {
        self.entries.retain(|e| !(e.0 == window && e.1 == dialog));
        self.entries.push((window, dialog, text));
        if self.entries.len() > PAIRS_MAX {
            self.entries.remove(0);
        }
    }

    /// Forgets every pair of another window. A window handle can be reused by a different dialog
    /// later, so only the pairs of the window being read stay.
    pub fn keep_window(&mut self, window: u64) {
        self.entries.retain(|e| e.0 == window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Vec<String> {
        vec![s.to_string()]
    }

    #[test]
    fn a_pair_gets_back_what_was_kept_for_it() {
        let mut cache = DialogCache::default();
        assert_eq!(cache.get(1, None), None);
        cache.put(1, None, text("sheet"));
        cache.put(1, Some(10), text("General"));
        cache.put(1, Some(11), text("Security"));
        assert_eq!(cache.get(1, None), Some(&text("sheet")[..]));
        assert_eq!(cache.get(1, Some(10)), Some(&text("General")[..]));
        assert_eq!(cache.get(1, Some(11)), Some(&text("Security")[..]));
        assert_eq!(cache.get(1, Some(12)), None);
        assert_eq!(cache.get(2, Some(10)), None);
    }

    #[test]
    fn an_empty_answer_is_kept_too_so_the_check_runs_once() {
        let mut cache = DialogCache::default();
        cache.put(1, None, Vec::new());
        assert_eq!(cache.get(1, None), Some(&[][..]));
    }

    #[test]
    fn putting_a_pair_again_replaces_it() {
        let mut cache = DialogCache::default();
        cache.put(1, Some(10), text("old"));
        cache.put(1, Some(10), text("new"));
        assert_eq!(cache.get(1, Some(10)), Some(&text("new")[..]));
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn at_most_eight_pairs_are_kept_and_the_oldest_goes() {
        let mut cache = DialogCache::default();
        for i in 0..PAIRS_MAX as u64 + 1 {
            cache.put(1, Some(i), text(&i.to_string()));
        }
        assert_eq!(cache.entries.len(), PAIRS_MAX);
        assert_eq!(cache.get(1, Some(0)), None);
        assert_eq!(cache.get(1, Some(1)), Some(&text("1")[..]));
        assert_eq!(cache.get(1, Some(PAIRS_MAX as u64)), Some(&text(&PAIRS_MAX.to_string())[..]));
    }

    #[test]
    fn another_window_forgets_the_pairs_of_the_old_one() {
        let mut cache = DialogCache::default();
        cache.put(1, None, text("a"));
        cache.put(1, Some(10), text("b"));
        cache.put(2, None, text("c"));
        cache.keep_window(2);
        assert_eq!(cache.get(1, None), None);
        assert_eq!(cache.get(1, Some(10)), None);
        assert_eq!(cache.get(2, None), Some(&text("c")[..]));
    }
}
