//! Bounded, viewport-driven Unicode refinement shared by unified and split rows.

use std::{ops::Range, sync::Arc};

use similar::{ChangeTag, TextDiff};

const MAX_LINE_BYTES: usize = 400;
const CACHE_SLOTS: usize = 256;

#[derive(Debug, Default)]
pub(crate) struct WordChanges {
    pub old: Vec<Range<usize>>,
    pub new: Vec<Range<usize>>,
}

impl WordChanges {
    fn between(old: &str, new: &str) -> Self {
        let mut result = Self::default();
        let (mut left, mut right) = (1, 1);
        for change in TextDiff::configure()
            .diff_unicode_words(&old[1..], &new[1..])
            .iter_all_changes()
        {
            let len = change.value().len();
            match change.tag() {
                ChangeTag::Delete => {
                    result.old.push(left..left + len);
                    left += len;
                }
                ChangeTag::Insert => {
                    result.new.push(right..right + len);
                    right += len;
                }
                ChangeTag::Equal => {
                    left += len;
                    right += len;
                }
            }
        }
        result
    }
}

#[derive(Debug)]
struct Entry {
    raw: usize,
    old: String,
    new: String,
    changes: Arc<WordChanges>,
}

#[derive(Debug, Default)]
pub(super) struct Cache {
    slots: Vec<Option<Entry>>,
}

impl Cache {
    pub(super) fn get(&mut self, raw: usize, old: &str, new: &str) -> Option<Arc<WordChanges>> {
        if old.len().max(new.len()) > MAX_LINE_BYTES
            || !old.starts_with('-')
            || !new.starts_with('+')
        {
            return None;
        }
        if self.slots.is_empty() {
            self.slots.resize_with(CACHE_SLOTS, || None);
        }
        let slot = &mut self.slots[raw % CACHE_SLOTS];
        if let Some(entry) = slot
            && entry.raw == raw
            && entry.old == old
            && entry.new == new
        {
            return Some(Arc::clone(&entry.changes));
        }
        let changes = Arc::new(WordChanges::between(old, new));
        *slot = Some(Entry {
            raw,
            old: old.to_owned(),
            new: new.to_owned(),
            changes: Arc::clone(&changes),
        });
        Some(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refines_both_sides_and_reuses_only_unchanged_text() {
        let mut cache = Cache::default();
        let old = "-let café = 100; // old value";
        let new = "+let café = 250; // new value";
        let first = cache.get(4, old, new).unwrap();
        let text = |line: &str, ranges: &[Range<usize>]| -> Vec<String> {
            ranges
                .iter()
                .map(|range| line[range.clone()].into())
                .collect()
        };
        assert_eq!(text(old, &first.old), ["100", "old"]);
        assert_eq!(text(new, &first.new), ["250", "new"]);
        assert!(Arc::ptr_eq(&first, &cache.get(4, old, new).unwrap()));
        assert!(!Arc::ptr_eq(
            &first,
            &cache.get(4, old, "+different").unwrap()
        ));
        assert!(
            cache
                .get(4, &format!("-{}", "a".repeat(MAX_LINE_BYTES)), new)
                .is_none()
        );
    }
}
