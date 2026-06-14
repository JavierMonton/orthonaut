//! Parsing and serialization for the Wikipedia-hosted word lists.
//!
//! In Wikipedia mode (when `wordlist_page` is configured) Orthonaut reads its two
//! word lists from a single wiki page. Each list lives between a pair of HTML-comment
//! sentinel markers, one word per line. Orthonaut only ever rewrites the text *between*
//! a pair of markers, so any human-added words and any surrounding documentation on the
//! page are preserved untouched.

use crate::checker::normalize_ignored_word;

pub const VALIDAS_START: &str = "<!-- ORTHONAUT:VALIDAS:START -->";
pub const VALIDAS_END: &str = "<!-- ORTHONAUT:VALIDAS:END -->";
pub const INCORRECTAS_START: &str = "<!-- ORTHONAUT:INCORRECTAS:START -->";
pub const INCORRECTAS_END: &str = "<!-- ORTHONAUT:INCORRECTAS:END -->";

/// Return the normalized words found strictly between `start` and `end` markers.
///
/// Lines that are empty (after trimming) are skipped. Returns an empty `Vec` if either
/// marker is missing or they appear out of order.
pub fn parse_block(wikitext: &str, start: &str, end: &str) -> Vec<String> {
    let Some(inner) = slice_between(wikitext, start, end) else {
        return Vec::new();
    };
    inner
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(normalize_ignored_word)
        .filter(|word| !word.is_empty())
        .collect()
}

/// Replace the content between `start` and `end` markers with `words` (one per line).
///
/// If the markers are present, only the text between them is replaced; everything else on
/// the page (including the other block and any documentation) is preserved byte-for-byte.
/// If the markers are absent, a fresh block is appended at the end of the page.
pub fn replace_block(wikitext: &str, start: &str, end: &str, words: &[String]) -> String {
    let body = render_words(words);
    match (wikitext.find(start), wikitext.find(end)) {
        (Some(s), Some(e)) if s < e => {
            let block_start = s + start.len();
            let mut out = String::with_capacity(wikitext.len() + body.len());
            out.push_str(&wikitext[..block_start]);
            out.push('\n');
            out.push_str(&body);
            out.push_str(&wikitext[e..]);
            out
        }
        _ => {
            let mut out = String::with_capacity(wikitext.len() + body.len() + 64);
            out.push_str(wikitext);
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(start);
            out.push('\n');
            out.push_str(&body);
            out.push_str(end);
            out.push('\n');
            out
        }
    }
}

fn render_words(words: &[String]) -> String {
    if words.is_empty() {
        return String::new();
    }
    let mut body = words.join("\n");
    body.push('\n');
    body
}

fn slice_between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let s = text.find(start)? + start.len();
    let e = text[s..].find(end)? + s;
    Some(&text[s..e])
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "Texto introductorio para humanos.\n\n\
<!-- ORTHONAUT:VALIDAS:START -->\n\
abaco\n\
mustafá\n\
<!-- ORTHONAUT:VALIDAS:END -->\n\n\
Más documentación.\n\n\
<!-- ORTHONAUT:INCORRECTAS:START -->\n\
haiga\n\
concecuencia\n\
<!-- ORTHONAUT:INCORRECTAS:END -->\n";

    #[test]
    fn parses_both_blocks() {
        let validas = parse_block(PAGE, VALIDAS_START, VALIDAS_END);
        assert_eq!(validas, vec!["abaco".to_string(), "mustafá".to_string()]);
        let incorrectas = parse_block(PAGE, INCORRECTAS_START, INCORRECTAS_END);
        assert_eq!(
            incorrectas,
            vec!["haiga".to_string(), "concecuencia".to_string()]
        );
    }

    #[test]
    fn parse_normalizes_and_skips_blank_lines() {
        let page = format!(
            "{VALIDAS_START}\n  HOLA  \n\n  Mundo\n{VALIDAS_END}\n"
        );
        assert_eq!(
            parse_block(&page, VALIDAS_START, VALIDAS_END),
            vec!["hola".to_string(), "mundo".to_string()]
        );
    }

    #[test]
    fn parse_missing_markers_returns_empty() {
        assert!(parse_block("no markers here", VALIDAS_START, VALIDAS_END).is_empty());
    }

    #[test]
    fn replace_preserves_surrounding_text_and_other_block() {
        let words = vec!["abaco".to_string(), "nuevo".to_string()];
        let updated = replace_block(PAGE, VALIDAS_START, VALIDAS_END, &words);

        // The valid words block now reflects the new set...
        assert_eq!(
            parse_block(&updated, VALIDAS_START, VALIDAS_END),
            words
        );
        // ...the incorrect words block is untouched...
        assert_eq!(
            parse_block(&updated, INCORRECTAS_START, INCORRECTAS_END),
            vec!["haiga".to_string(), "concecuencia".to_string()]
        );
        // ...and the surrounding documentation survives.
        assert!(updated.contains("Texto introductorio para humanos."));
        assert!(updated.contains("Más documentación."));
    }

    #[test]
    fn replace_round_trips_a_merged_set() {
        // Simulate the export merge: existing on-page words ∪ a new word.
        let mut merged: std::collections::BTreeSet<String> =
            parse_block(PAGE, VALIDAS_START, VALIDAS_END).into_iter().collect();
        merged.insert("recienllegada".to_string());
        let words: Vec<String> = merged.into_iter().collect();

        let updated = replace_block(PAGE, VALIDAS_START, VALIDAS_END, &words);
        let reparsed = parse_block(&updated, VALIDAS_START, VALIDAS_END);
        assert!(reparsed.contains(&"abaco".to_string()));
        assert!(reparsed.contains(&"mustafá".to_string()));
        assert!(reparsed.contains(&"recienllegada".to_string()));
    }

    #[test]
    fn replace_appends_block_when_markers_absent() {
        let page = "Sólo documentación, sin marcadores.\n";
        let words = vec!["palabra".to_string()];
        let updated = replace_block(page, VALIDAS_START, VALIDAS_END, &words);

        assert!(updated.starts_with("Sólo documentación, sin marcadores.\n"));
        assert!(updated.contains(VALIDAS_START));
        assert!(updated.contains(VALIDAS_END));
        assert_eq!(
            parse_block(&updated, VALIDAS_START, VALIDAS_END),
            words
        );
    }

    #[test]
    fn replace_with_empty_words_clears_block_but_keeps_markers() {
        let updated = replace_block(PAGE, VALIDAS_START, VALIDAS_END, &[]);
        assert!(updated.contains(VALIDAS_START));
        assert!(updated.contains(VALIDAS_END));
        assert!(parse_block(&updated, VALIDAS_START, VALIDAS_END).is_empty());
    }
}
