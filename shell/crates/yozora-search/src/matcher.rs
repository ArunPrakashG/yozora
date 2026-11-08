//! Fuzzy matching with nucleo, plus the things a launcher wants on top: initials (`vsc` for
//! Visual Studio Code), and a bonus when the name starts with what was typed.

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};

/// Reused across keystrokes: nucleo's matcher keeps scratch buffers.
pub struct Fuzzy {
    matcher: Matcher,
    pattern: Pattern,
    needle: String,
    buf: Vec<char>,
    indices: Vec<u32>,
}

impl Default for Fuzzy {
    fn default() -> Self {
        Self::new()
    }
}

/// Scores so different fields and bonuses compare on one scale.
pub const PREFIX_BONUS: u32 = 200;
pub const INITIALS_BONUS: u32 = 150;

impl Fuzzy {
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            pattern: Pattern::default(),
            needle: String::new(),
            buf: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// Sets what's being searched for.
    pub fn set(&mut self, needle: &str) {
        if needle != self.needle {
            self.needle = needle.to_lowercase();
            self.pattern = Pattern::new(needle, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
        }
    }

    /// The score for `hay`, or `None` if it doesn't match.
    pub fn score(&mut self, hay: &str) -> Option<u32> {
        if self.needle.is_empty() {
            return Some(0);
        }
        let s = self.pattern.score(Utf32Str::new(hay, &mut self.buf), &mut self.matcher)?;
        Some(s + self.bonus(hay))
    }

    /// Like [`Fuzzy::score`], but only when every typed word starts a word in `hay`, so
    /// keywords and descriptions match "audio" or "web brow" and not letters scattered
    /// through a sentence.
    pub fn score_words(&mut self, hay: &str) -> Option<u32> {
        let lower = hay.to_lowercase();
        let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        let all = self.needle.split_whitespace().all(|n| words.iter().any(|w| w.starts_with(n)));
        if all { self.score(hay) } else { None }
    }

    /// The char indices in `hay` that matched, sorted, for highlighting.
    pub fn indices(&mut self, hay: &str) -> Vec<usize> {
        if self.needle.is_empty() {
            return Vec::new();
        }
        if let Some(run) = substring(hay, &self.needle) {
            return run;
        }
        if let Some(initials) = initial_indices(hay, &self.needle) {
            return initials;
        }
        self.indices.clear();
        if self.pattern.indices(Utf32Str::new(hay, &mut self.buf), &mut self.matcher, &mut self.indices).is_none() {
            return Vec::new();
        }
        self.indices.sort_unstable();
        self.indices.dedup();
        self.indices.iter().map(|i| *i as usize).collect()
    }

    fn bonus(&self, hay: &str) -> u32 {
        let lower = hay.to_lowercase();
        if lower.starts_with(&self.needle) {
            PREFIX_BONUS
        } else if self.needle.chars().count() >= 2 && initial_indices(hay, &self.needle).is_some() {
            INITIALS_BONUS
        } else if lower.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(&self.needle)) {
            // A later word starts with it: "code" in "Visual Studio Code".
            PREFIX_BONUS / 2
        } else {
            0
        }
    }
}

/// The char indices of the first place `needle` (lowercase) appears in `hay`, preferring
/// the start of a word, so "fox" lights up the end of "Firefox" rather than F, o, x.
fn substring(hay: &str, needle: &str) -> Option<Vec<usize>> {
    let hay: Vec<char> = hay.chars().flat_map(char::to_lowercase).collect();
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    let starts: Vec<usize> =
        (0..=hay.len() - needle.len()).filter(|&i| hay[i..i + needle.len()] == needle[..]).collect();
    let at = starts.iter().find(|&&i| i == 0 || !hay[i - 1].is_alphanumeric()).or(starts.first()).copied()?;
    Some((at..at + needle.len()).collect())
}

/// Where each word starts: after a space or punctuation, or at a lower-to-upper change.
fn word_starts(hay: &str) -> Vec<(usize, char)> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    for (i, c) in hay.chars().enumerate() {
        let start = match prev {
            None => c.is_alphanumeric(),
            Some(p) => c.is_alphanumeric() && (!p.is_alphanumeric() || (p.is_lowercase() && c.is_uppercase())),
        };
        if start {
            out.push((i, c));
        }
        prev = Some(c);
    }
    out
}

/// The char indices of word initials spelling `needle` (already lowercase), in order.
/// `vsc` → Visual Studio Code; `gimp` → GNU Image Manipulation Program.
pub fn initial_indices(hay: &str, needle: &str) -> Option<Vec<usize>> {
    let starts = word_starts(hay);
    let needle: Vec<char> = needle.chars().filter(|c| !c.is_whitespace()).collect();
    if needle.len() < 2 || needle.len() > starts.len() {
        return None;
    }
    let mut out = Vec::with_capacity(needle.len());
    let mut it = starts.iter();
    for n in &needle {
        let (i, _) = it.find(|(_, c)| c.to_lowercase().eq(n.to_lowercase()))?;
        out.push(*i);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials() {
        assert_eq!(initial_indices("Visual Studio Code", "vsc"), Some(vec![0, 7, 14]));
        assert_eq!(initial_indices("GNU Image Manipulation Program", "gimp"), Some(vec![0, 4, 10, 23]));
        assert_eq!(initial_indices("LibreOffice Writer", "low"), Some(vec![0, 5, 12]));
        assert_eq!(initial_indices("Firefox", "ff"), None);
        assert_eq!(initial_indices("Visual Studio Code", "v"), None);
    }

    #[test]
    fn ranks_prefixes_and_initials_first() {
        let mut f = Fuzzy::new();
        f.set("vsc");
        let code = f.score("Visual Studio Code").unwrap();
        let other = f.score("Volume Settings Control panel").unwrap_or(0);
        assert!(code >= other);
        let unrelated = f.score("Disk Usage Analyzer");
        assert!(unrelated.is_none());

        f.set("fire");
        assert!(f.score("Firefox").unwrap() > f.score("Bonfire Notes").unwrap());
        f.set("code");
        assert!(f.score("Visual Studio Code").unwrap() > f.score("Kdenlive Encoder").unwrap_or(0));
    }

    #[test]
    fn highlights() {
        let mut f = Fuzzy::new();
        f.set("vsc");
        assert_eq!(f.indices("Visual Studio Code"), vec![0, 7, 14]);
        f.set("fox");
        assert_eq!(f.indices("Firefox"), vec![4, 5, 6]);
        f.set("ffx");
        assert_eq!(f.indices("Firefox"), vec![0, 4, 6]);
    }
}
