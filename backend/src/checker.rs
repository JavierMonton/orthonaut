use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use zspell::Dictionary;

use crate::extractor::ExtractedToken;

const MAX_CLEAR_ERRORS_PER_PAGE: usize = 12;

pub struct SpellChecker {
    dict: Dictionary,
    cache: HashMap<String, bool>,
    suppressed_words: HashSet<String>,
    always_wrong_words: HashSet<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum CheckerError {
    #[error("failed to read dictionary file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to build hunspell dictionary: {0}")]
    DictBuild(String),
}

impl SpellChecker {
    pub fn new(dict_dir: &Path) -> Result<Self, CheckerError> {
        let aff = read_dictionary_text(&dict_dir.join("es_ES.aff"))?;
        let dic = read_dictionary_text(&dict_dir.join("es_ES.dic"))?;
        let suppressed_words = load_suppressed_words(dict_dir)?;
        let always_wrong_words = load_always_wrong_from_file(dict_dir)?;

        let dict = zspell::builder()
            .config_str(&aff)
            .dict_str(&dic)
            .build()
            .map_err(|e| CheckerError::DictBuild(e.to_string()))?;

        Ok(Self {
            dict,
            cache: HashMap::new(),
            suppressed_words,
            always_wrong_words,
        })
    }

    pub fn find_wrong_words_from_tokens(&mut self, tokens: &[ExtractedToken]) -> Vec<String> {
        let mut wrong = Vec::new();
        let casing_map = build_casing_map(tokens);

        for token in tokens {
            if token.is_link {
                continue;
            }
            if self.always_wrong_words.contains(&token.normalized) {
                wrong.push(token.normalized.clone());
                continue;
            }
            if self.should_suppress_unknown(token, &casing_map) {
                continue;
            }

            let is_valid = if let Some(cached) = self.cache.get(&token.normalized) {
                *cached
            } else {
                let valid = self.dict.check_word(&token.normalized);
                self.cache.insert(token.normalized.clone(), valid);
                valid
            };

            if !is_valid && is_high_confidence_error(&token.normalized) {
                wrong.push(token.normalized.clone());
            }
        }

        wrong.sort();
        wrong.dedup();
        if wrong.len() > MAX_CLEAR_ERRORS_PER_PAGE {
            return Vec::new();
        }
        wrong
    }

    pub fn add_ignored_word(&mut self, word: &str) -> bool {
        let normalized = normalize_ignored_word(word);
        if normalized.is_empty() {
            return false;
        }
        self.suppressed_words.insert(normalized)
    }

    pub fn add_ignored_words<I>(&mut self, words: I)
    where
        I: IntoIterator<Item = String>,
    {
        for word in words {
            self.add_ignored_word(&word);
        }
    }

    pub fn remove_ignored_word(&mut self, word: &str) -> bool {
        let normalized = normalize_ignored_word(word);
        if normalized.is_empty() {
            return false;
        }
        self.suppressed_words.remove(&normalized)
    }

    pub fn replace_ignored_words<I>(&mut self, words: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.suppressed_words.clear();
        self.add_ignored_words(words);
    }

    pub fn add_always_wrong_word(&mut self, word: &str) -> bool {
        let normalized = normalize_ignored_word(word);
        if normalized.is_empty() {
            return false;
        }
        self.always_wrong_words.insert(normalized)
    }

    pub fn add_always_wrong_words<I>(&mut self, words: I)
    where
        I: IntoIterator<Item = String>,
    {
        for word in words {
            self.add_always_wrong_word(&word);
        }
    }

    pub fn remove_always_wrong_word(&mut self, word: &str) -> bool {
        let normalized = normalize_ignored_word(word);
        if normalized.is_empty() {
            return false;
        }
        self.always_wrong_words.remove(&normalized)
    }

    pub fn replace_always_wrong_words<I>(&mut self, words: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.always_wrong_words.clear();
        self.add_always_wrong_words(words);
    }

    fn should_suppress_unknown(
        &self,
        token: &ExtractedToken,
        casing_map: &HashMap<String, CasingStats>,
    ) -> bool {
        if self.suppressed_words.contains(&token.normalized) {
            return true;
        }

        if let Some(stats) = casing_map.get(&token.normalized) {
            // High-precision mode favors avoiding false positives:
            // if we ever saw this token capitalized, treat it as ambiguous.
            if stats.has_uppercase {
                return true;
            }
        }

        matches!(
            token.normalized.as_str(),
            "url"
                | "http"
                | "https"
                | "www"
                | "org"
                | "com"
                | "net"
                | "isbn"
                | "issn"
                | "doi"
                | "wikidata"
                | "the"
                | "and"
                | "for"
                | "with"
                | "century"
                | "seventeenth"
                | "bibliografía"
        )
    }
}

fn read_dictionary_text(path: &Path) -> Result<String, CheckerError> {
    let bytes = fs::read(path)?;
    match String::from_utf8(bytes.clone()) {
        Ok(text) => Ok(text),
        Err(_) => {
            // Many Hunspell dictionaries are distributed in ISO-8859-1.
            let latin1_text: String = bytes.into_iter().map(char::from).collect();
            Ok(latin1_text)
        }
    }
}

fn load_suppressed_words(dict_dir: &Path) -> Result<HashSet<String>, CheckerError> {
    let path = suppressions_path(dict_dir);

    if !path.exists() {
        return Ok(HashSet::new());
    }

    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_lowercase())
        .collect())
}

pub fn suppressions_path(dict_dir: &Path) -> std::path::PathBuf {
    let from_env = std::env::var("ORTHONAUT_SUPPRESSIONS_PATH")
        .ok()
        .map(std::path::PathBuf::from);
    let default_path = dict_dir.join("suppressions.txt");
    from_env.unwrap_or(default_path)
}

pub fn always_wrong_path(dict_dir: &Path) -> std::path::PathBuf {
    let from_env = std::env::var("ORTHONAUT_ALWAYS_WRONG_PATH")
        .ok()
        .map(std::path::PathBuf::from);
    let default_path = dict_dir.join("always_wrong.txt");
    from_env.unwrap_or(default_path)
}

fn load_always_wrong_from_file(dict_dir: &Path) -> Result<HashSet<String>, CheckerError> {
    let path = always_wrong_path(dict_dir);
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_lowercase())
        .collect())
}

pub fn normalize_ignored_word(word: &str) -> String {
    word.trim().to_lowercase()
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct CasingStats {
    has_uppercase: bool,
    has_lowercase: bool,
}

fn build_casing_map(tokens: &[ExtractedToken]) -> HashMap<String, CasingStats> {
    let mut map: HashMap<String, CasingStats> = HashMap::new();

    for token in tokens {
        let entry = map.entry(token.normalized.clone()).or_default();
        if token.saw_uppercase {
            entry.has_uppercase = true;
        } else {
            entry.has_lowercase = true;
        }
    }

    map
}

fn is_high_confidence_error(word: &str) -> bool {
    if word.len() < 4 {
        return false;
    }

    if word.contains('\'') || word.contains('-') {
        return false;
    }

    word.chars().all(|c| c.is_alphabetic())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::extractor::extract_tokens_from_input;

    use super::{
        build_casing_map, is_high_confidence_error, normalize_ignored_word, CasingStats,
        ExtractedToken, SpellChecker,
    };

    #[test]
    fn keeps_only_high_confidence_error_shapes() {
        assert!(is_high_confidence_error("palabraa"));
        assert!(!is_high_confidence_error("url"));
        assert!(!is_high_confidence_error("abc"));
        assert!(!is_high_confidence_error("word-word"));
    }

    #[test]
    fn detects_title_case_only_tokens_as_proper_nouns() {
        let tokens = vec![
            ExtractedToken {
                normalized: "mustafá".to_string(),
                saw_uppercase: true,
                is_link: false,
            },
            ExtractedToken {
                normalized: "mustafá".to_string(),
                saw_uppercase: true,
                is_link: false,
            },
            ExtractedToken {
                normalized: "imperio".to_string(),
                saw_uppercase: true,
                is_link: false,
            },
            ExtractedToken {
                normalized: "imperio".to_string(),
                saw_uppercase: false,
                is_link: false,
            },
        ];

        let map = build_casing_map(&tokens);
        assert_eq!(
            map.get("mustafá"),
            Some(&CasingStats {
                has_uppercase: true,
                has_lowercase: false
            })
        );
        assert_eq!(
            map.get("imperio"),
            Some(&CasingStats {
                has_uppercase: true,
                has_lowercase: true
            })
        );
    }

    #[test]
    fn suppresses_proper_nouns_and_keeps_clear_typos() {
        let mut checker = SpellChecker::new(Path::new("dictionaries")).expect("dictionary available");
        let tokens = vec![
            ExtractedToken {
                normalized: "mustafá".to_string(),
                saw_uppercase: true,
                is_link: false,
            },
            ExtractedToken {
                normalized: "mustafá".to_string(),
                saw_uppercase: true,
                is_link: false,
            },
            ExtractedToken {
                normalized: "palabraa".to_string(),
                saw_uppercase: false,
                is_link: false,
            },
            ExtractedToken {
                normalized: "url".to_string(),
                saw_uppercase: false,
                is_link: false,
            },
        ];

        let wrong = checker.find_wrong_words_from_tokens(&tokens);
        assert!(wrong.contains(&"palabraa".to_string()));
        assert!(!wrong.contains(&"mustafá".to_string()));
        assert!(!wrong.contains(&"url".to_string()));
    }

    #[test]
    fn drops_results_when_unknown_set_is_too_large() {
        let mut checker = SpellChecker::new(Path::new("dictionaries")).expect("dictionary available");
        let tokens: Vec<ExtractedToken> = (0..20)
            .map(|idx| ExtractedToken {
                normalized: format!("palabrafalsa{idx}"),
                saw_uppercase: false,
                is_link: false,
            })
            .collect();

        let wrong = checker.find_wrong_words_from_tokens(&tokens);
        assert!(wrong.is_empty());
    }

    #[test]
    fn fazil_like_page_content_is_treated_as_ok_in_precision_mode() {
        let mut checker = SpellChecker::new(Path::new("dictionaries")).expect("dictionary available");
        let sample = r#"
        Fazil Mustafá fue un Gran Visir otomano. Ahmed II nombró a Fazil Mustafá.
        Carlos de Lorena venció en Slankamen. Véase también bibliografía y referencias.
        Venice, Austria, and the Turks in the seventeenth century.
        Britannica: url
        "#;
        let tokens = extract_tokens_from_input(sample);
        let wrong = checker.find_wrong_words_from_tokens(&tokens);
        assert!(wrong.is_empty());
    }

    #[test]
    fn can_add_and_remove_ignored_words_at_runtime() {
        let mut checker = SpellChecker::new(Path::new("dictionaries")).expect("dictionary available");
        let token = ExtractedToken {
            normalized: "palabrafalsa".to_string(),
            saw_uppercase: false,
            is_link: false,
        };

        let initial = checker.find_wrong_words_from_tokens(std::slice::from_ref(&token));
        assert!(initial.contains(&"palabrafalsa".to_string()));

        checker.add_ignored_word("palabrafalsa");
        let after_add = checker.find_wrong_words_from_tokens(std::slice::from_ref(&token));
        assert!(!after_add.contains(&"palabrafalsa".to_string()));

        checker.remove_ignored_word("palabrafalsa");
        let after_remove = checker.find_wrong_words_from_tokens(std::slice::from_ref(&token));
        assert!(after_remove.contains(&"palabrafalsa".to_string()));
    }

    #[test]
    fn skips_words_marked_as_links() {
        let mut checker = SpellChecker::new(Path::new("dictionaries")).expect("dictionary available");
        let tokens = vec![
            ExtractedToken {
                normalized: "palabrafalsa".to_string(),
                saw_uppercase: false,
                is_link: true,
            },
            ExtractedToken {
                normalized: "otrapalabra".to_string(),
                saw_uppercase: false,
                is_link: false,
            },
        ];
        let wrong = checker.find_wrong_words_from_tokens(&tokens);
        assert!(!wrong.contains(&"palabrafalsa".to_string()), "link word should be suppressed");
        assert!(wrong.contains(&"otrapalabra".to_string()), "non-link unknown word should be flagged");
    }

    #[test]
    fn normalizes_ignored_words_before_storing() {
        assert_eq!(normalize_ignored_word("  PaLabra  "), "palabra".to_string());
    }

}
