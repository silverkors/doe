//! Plain-text search over buffer contents, returning char-index ranges so the
//! results map directly onto cursors and selections.
//!
//! The implementation is a straightforward char-wise scan: correct for UTF-8
//! and case folding without the byte/char mapping hazards of `str::find` on a
//! lowercased copy. Regex search is a planned addition (see `Find` options).

/// Find every occurrence of `needle` in `text`, returning `(start, end)` char
/// indices. Empty needles yield no matches.
pub fn find_all(text: &str, needle: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    let hay: Vec<char> = text.chars().collect();
    let pat: Vec<char> = needle.chars().collect();
    if pat.is_empty() || pat.len() > hay.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + pat.len() <= hay.len() {
        if matches_at(&hay, i, &pat, case_sensitive) {
            out.push((i, i + pat.len()));
            i += pat.len();
        } else {
            i += 1;
        }
    }
    out
}

/// First match at or after `from` (char index), wrapping to the start.
pub fn find_next(text: &str, needle: &str, from: usize, case_sensitive: bool) -> Option<(usize, usize)> {
    let all = find_all(text, needle, case_sensitive);
    all.iter()
        .find(|(s, _)| *s >= from)
        .or_else(|| all.first())
        .copied()
}

/// Last match strictly before `from` (char index), wrapping to the end.
pub fn find_prev(text: &str, needle: &str, from: usize, case_sensitive: bool) -> Option<(usize, usize)> {
    let all = find_all(text, needle, case_sensitive);
    all.iter()
        .rev()
        .find(|(s, _)| *s < from)
        .or_else(|| all.last())
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_all_occurrences() {
        assert_eq!(find_all("abcabc", "bc", true), vec![(1, 3), (4, 6)]);
    }

    #[test]
    fn empty_needle_yields_nothing() {
        assert!(find_all("abc", "", true).is_empty());
    }

    #[test]
    fn case_insensitive_matches() {
        assert_eq!(find_all("Foo foo FOO", "foo", false), vec![(0, 3), (4, 7), (8, 11)]);
        assert_eq!(find_all("Foo foo FOO", "foo", true), vec![(4, 7)]);
    }

    #[test]
    fn unicode_char_indices() {
        // "héllo héllo" — match by char index, not byte index.
        assert_eq!(find_all("héllo héllo", "héllo", true), vec![(0, 5), (6, 11)]);
    }

    #[test]
    fn next_and_prev_wrap() {
        let t = "a.a.a";
        assert_eq!(find_next(t, "a", 1, true), Some((2, 3)));
        assert_eq!(find_next(t, "a", 5, true), Some((0, 1))); // wraps
        assert_eq!(find_prev(t, "a", 2, true), Some((0, 1)));
        assert_eq!(find_prev(t, "a", 0, true), Some((4, 5))); // wraps
    }
}

fn matches_at(hay: &[char], at: usize, pat: &[char], case_sensitive: bool) -> bool {
    for (k, &pc) in pat.iter().enumerate() {
        let hc = hay[at + k];
        let eq = if case_sensitive {
            hc == pc
        } else {
            hc.eq_ignore_ascii_case(&pc)
                || hc.to_lowercase().eq(pc.to_lowercase())
        };
        if !eq {
            return false;
        }
    }
    true
}
