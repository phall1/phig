//! Deterministic, case-insensitive discovery matching, independent of the TUI.
//!
//! Literal matches outrank subsequences; ties retain the source order. Positions
//! refer to original Unicode characters, never UTF-8 bytes or terminal columns.

type Score = (u8, usize, usize, usize); // match tier, gaps, start, candidate length

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Match {
    score: Score,
    pub positions: Vec<usize>,
}

pub(crate) struct Query(String);

impl Query {
    pub fn new(query: &str) -> Self {
        Self(normalize(query.trim()))
    }

    pub fn find(&self, text: &str) -> Option<Match> {
        let (folded, origins) = fold(text);
        let (score, indices) = self.alignment(&folded)?;
        let mut positions: Vec<_> = indices.iter().map(|index| origins[*index]).collect();
        positions.dedup();
        Some(Match { score, positions })
    }

    fn score(&self, text: &str) -> Option<Score> {
        self.alignment(&normalize(text)).map(|(score, _)| score)
    }

    fn alignment(&self, text: &str) -> Option<(Score, Vec<usize>)> {
        if self.0.is_empty() {
            return Some(((0, 0, 0, 0), Vec::new()));
        }
        let (tier, indices) = self
            .literal_match(text)
            .or_else(|| subsequence(text, &self.0).map(|indices| (3, indices)))?;
        let start = *indices.first()?;
        let gaps = indices.last()? - start + 1 - indices.len();
        Some(((tier, gaps, start, text.chars().count()), indices))
    }

    fn literal_match(&self, text: &str) -> Option<(u8, Vec<usize>)> {
        let (tier, byte) = best_literal(text, &self.0)?;
        let start = text[..byte].chars().count();
        let count = self.0.chars().count();
        Some((tier, (start..start + count).collect()))
    }
}

fn best_literal(text: &str, query: &str) -> Option<(u8, usize)> {
    let mut offset = 0;
    let mut first = None;
    while let Some(relative) = text[offset..].find(query) {
        let byte = offset + relative;
        let tier = literal_tier(text, byte, query);
        if tier <= 1 {
            return Some((tier, byte));
        }
        first.get_or_insert((tier, byte));
        // Advance one character, not one match: path queries can overlap.
        offset = byte + text[byte..].chars().next()?.len_utf8();
    }
    first
}

fn literal_tier(text: &str, byte: usize, query: &str) -> u8 {
    if text == query {
        0
    } else if text[..byte]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric())
    {
        1
    } else {
        2
    }
}

/// Lowercase expansions and both Greek sigma forms retain their original index.
fn normalized_chars(text: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    text.chars().enumerate().flat_map(|(index, character)| {
        character.to_lowercase().map(move |lower| {
            let lower = match lower {
                'ς' => 'σ',
                other => other,
            };
            (index, lower)
        })
    })
}

fn normalize(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    normalized_chars(text)
        .map(|(_, character)| character)
        .collect()
}

fn fold(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::new();
    let mut origins = Vec::new();
    for (index, character) in normalized_chars(text) {
        folded.push(character);
        origins.push(index);
    }
    (folded, origins)
}

fn subsequence(text: &str, query: &str) -> Option<Vec<usize>> {
    // Reject nonmatches with a linear scan before considering alternate alignments.
    let mut remaining = text.chars();
    if !query.chars().all(|needle| remaining.any(|c| c == needle)) {
        return None;
    }
    let characters: Vec<_> = text.chars().collect();
    let needle: Vec<_> = query.chars().collect();
    let (start, end) = shortest_window(&characters, &needle)?;
    let mut remaining = characters.iter().enumerate().take(end + 1).skip(start);
    needle
        .iter()
        .map(|needle| {
            remaining
                .by_ref()
                .find(|(_, c)| *c == needle)
                .map(|(i, _)| i)
        })
        .collect()
}

/// Track the latest viable start of every query prefix. This finds the smallest
/// window in O(text × query) time and O(query) space, without enumerating paths.
fn shortest_window(text: &[char], query: &[char]) -> Option<(usize, usize)> {
    let mut starts = vec![None; query.len()];
    let mut best = None;
    for (end, character) in text.iter().enumerate() {
        for (index, needle) in query.iter().enumerate().rev() {
            if character != needle {
                continue;
            }
            starts[index] = if index == 0 {
                Some(end)
            } else {
                starts[index - 1]
            };
        }
        if let Some(start) = starts.last().copied().flatten() {
            let candidate = (end - start, start, end);
            best = Some(best.map_or(candidate, |previous| candidate.min(previous)));
        }
    }
    best.map(|(_, start, end)| (start, end))
}

pub(crate) fn ranked<T>(
    items: impl IntoIterator<Item = T>,
    query: &str,
    label: impl Fn(&T) -> &str,
) -> Vec<T> {
    let query = Query::new(query);
    if query.0.is_empty() {
        return items.into_iter().collect();
    }
    let mut matches: Vec<_> = items
        .into_iter()
        .filter_map(|item| query.score(label(&item)).map(|score| (score, item)))
        .collect();
    matches.sort_by_key(|(score, _)| *score);
    matches.into_iter().map(|(_, item)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_exact_boundary_literal_and_abbreviations_in_that_order() {
        let items = [
            "show something",
            "fooshow",
            "View show",
            "SHOW",
            "show help",
        ];
        assert_eq!(
            ranked(items, "show", |item| *item),
            [
                "SHOW",
                "show help",
                "show something",
                "View show",
                "fooshow"
            ]
        );
        assert_eq!(
            ranked(["s---h", "s-h", "sh"], "sh", |item| *item),
            ["sh", "s-h", "s---h"]
        );
        assert!(Query::new("tglpr").find("Toggle preview").is_some());
        assert!(Query::new("srcrndr").find("src/tui/render.rs").is_some());
        assert!(Query::new("zxy").find("xyz").is_none());
    }

    #[test]
    fn shortest_later_subsequence_wins_and_repeated_letters_remain_distinct() {
        assert_eq!(
            ranked(["a---b", "a/long/path/a-b"], "ab", |item| *item),
            ["a/long/path/a-b", "a---b"]
        );
        assert_eq!(
            Query::new("ab").find("a/long/path/a-b").unwrap().positions,
            [12, 14]
        );
        assert!(Query::new("aa").find("a").is_none());
        assert_eq!(Query::new("aa").find("a--a-a").unwrap().positions, [3, 5]);
    }

    #[test]
    fn greek_sigma_matching_preserves_case_insensitive_path_search() {
        let query = Query::new("ος/test.rs");
        assert_eq!(
            query.find("ΟΣ/test.rs").unwrap().positions,
            (0..10).collect::<Vec<_>>()
        );
        assert!(query.score("ΟΣ/test.rs").is_some());
        assert!(Query::new("ΟΣ").find("ος").is_some());
    }

    #[test]
    fn overlapping_literals_can_reveal_a_better_path_boundary() {
        assert_eq!(
            Query::new("a/a").find("ba/a/a").unwrap().positions,
            [3, 4, 5]
        );
        assert_eq!(
            ranked(["ba/a/a", "za/ab"], "a/a", |s| *s),
            ["ba/a/a", "za/ab"]
        );
        assert_eq!(
            Query::new("界/界").find("b界/界/界").unwrap().positions,
            [3, 4, 5]
        );
    }

    #[test]
    fn later_path_boundary_beats_an_earlier_interior_occurrence() {
        let matched = Query::new("ref").find("src/preferences/refs.rs").unwrap();
        assert_eq!(matched.positions, [16, 17, 18]);
        assert_eq!(
            ranked(
                ["preferences.rs", "src/preferences/refs.rs"],
                "ref",
                |item| *item
            ),
            ["src/preferences/refs.rs", "preferences.rs"]
        );
    }

    #[test]
    fn empty_queries_and_equal_scores_preserve_source_identity_and_order() {
        let items = [("z", 12), ("a", 3), ("a", 8)];
        assert_eq!(ranked(items, "  ", |item| item.0), items);
        assert_eq!(ranked(items, "a", |item| item.0), [("a", 3), ("a", 8)]);
        assert!(Query::new("longer").find("short").is_none());
        assert!(Query::new("x").find("").is_none());
    }

    #[test]
    fn unicode_matches_point_back_to_original_characters() {
        let matched = Query::new(" É界 ").find("aÉ/界.rs").unwrap();
        assert_eq!(matched.positions, [1, 3]);
        assert_eq!(Query::new("i\u{307}").find("İ.rs").unwrap().positions, [0]);
        assert_eq!(Query::new("rs").find("İ/界.rs").unwrap().positions, [4, 5]);
        assert_eq!(
            Query::new("e\u{301}")
                .find("cafe\u{301}")
                .unwrap()
                .positions,
            [3, 4]
        );
    }
}
