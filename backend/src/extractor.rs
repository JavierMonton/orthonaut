use regex::Regex;
use scraper::{node::Node, ElementRef, Html, Selector};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedToken {
    pub normalized: String,
    pub saw_uppercase: bool,
    pub is_link: bool,
}

pub fn extract_tokens(html: &str) -> Vec<ExtractedToken> {
    let document = Html::parse_document(html);

    // Rendered Wikipedia HTML wraps article content in #mw-content-text.
    let rendered_selector = Selector::parse("#mw-content-text p").expect("valid rendered selector");

    // Parsoid HTML (returned by the Wikipedia REST API) places content in <section>
    // elements directly inside <body>, with no #mw-content-text wrapper at all.
    let parsoid_selector = Selector::parse("body section p, body p").expect("valid parsoid selector");

    let mut nodes: Vec<_> = document.select(&rendered_selector).collect();
    if nodes.is_empty() {
        nodes = document.select(&parsoid_selector).collect();
    }

    let splitter = Regex::new(r"[^\p{L}\p{Mn}\p{Pd}']+").expect("valid split regex");

    let mut tokens = Vec::new();

    for node in nodes {
        if should_skip_node(&node) {
            continue;
        }
        extract_tokens_from_node(node, &splitter, false, &mut tokens);
    }

    tokens
}

fn extract_tokens_from_node(
    node: ElementRef<'_>,
    splitter: &Regex,
    in_link: bool,
    tokens: &mut Vec<ExtractedToken>,
) {
    for child in node.children() {
        match child.value() {
            Node::Text(text) => {
                for raw in splitter.split(text.as_ref()) {
                    if let Some(mut token) = normalize_token(raw) {
                        if !should_skip(&token) {
                            token.is_link = in_link;
                            tokens.push(token);
                        }
                    }
                }
            }
            Node::Element(_) => {
                if let Some(element_ref) = ElementRef::wrap(child) {
                    let is_anchor = element_ref.value().name() == "a";
                    extract_tokens_from_node(element_ref, splitter, in_link || is_anchor, tokens);
                }
            }
            _ => {}
        }
    }
}

/// Returns up to 10 paragraph texts from `html` that contain `word` as a whole token.
pub fn extract_paragraphs_for_word(html: &str, word: &str) -> Vec<String> {
    let document = Html::parse_document(html);

    let rendered_selector = Selector::parse("#mw-content-text p").expect("valid rendered selector");
    let parsoid_selector = Selector::parse("body section p, body p").expect("valid parsoid selector");

    let nodes: Vec<_> = {
        let rendered: Vec<_> = document.select(&rendered_selector).collect();
        if rendered.is_empty() {
            document.select(&parsoid_selector).collect()
        } else {
            rendered
        }
    };

    let splitter = Regex::new(r"[^\p{L}\p{Mn}\p{Pd}']+").expect("valid split regex");
    let word_lower = word.to_lowercase();
    let mut paragraphs = Vec::new();

    for node in &nodes {
        if should_skip_node(node) {
            continue;
        }
        let text = node.text().collect::<String>();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let contains_word = splitter.split(trimmed).any(|token| {
            let t = token.trim_matches(|c: char| !c.is_alphabetic() && c != '\'' && c != '-');
            !t.is_empty() && t.to_lowercase() == word_lower
        });
        if contains_word {
            paragraphs.push(trimmed.to_string());
            if paragraphs.len() >= 10 {
                break;
            }
        }
    }

    paragraphs
}

pub fn extract_tokens_from_input(input: &str) -> Vec<ExtractedToken> {
    if looks_like_html(input) {
        return extract_tokens(input);
    }
    extract_tokens_from_text(input)
}

fn extract_tokens_from_text(text: &str) -> Vec<ExtractedToken> {
    let splitter = Regex::new(r"[^\p{L}\p{Mn}\p{Pd}']+").expect("valid split regex");
    splitter
        .split(text)
        .map(normalize_token)
        .flatten()
        .filter(|token| !should_skip(token))
        .collect()
}

fn looks_like_html(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    lower.contains("<html")
        || lower.contains("<body")
        || lower.contains("<p")
        || lower.contains("<div")
        || lower.contains("</")
}

fn normalize_token(token: &str) -> Option<ExtractedToken> {
    let trimmed = token.trim_matches(|c: char| !c.is_alphabetic() && c != '\'' && c != '-');
    if trimmed.is_empty() {
        return None;
    }

    Some(ExtractedToken {
        normalized: trimmed.to_lowercase(),
        saw_uppercase: trimmed.chars().any(|c| c.is_uppercase()),
        is_link: false,
    })
}

fn should_skip(token: &ExtractedToken) -> bool {
    if token.normalized.len() < 2 {
        return true;
    }
    if token.normalized.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    if token.normalized == "url" {
        return true;
    }
    if token.normalized.contains("http") || token.normalized.contains("www") {
        return true;
    }
    if token.normalized.contains('\'') || token.normalized.contains('-') {
        return true;
    }
    // Skip tokens containing characters outside Latin script (CJK, Arabic, Cyrillic, etc.).
    // U+024F is the last codepoint of Latin Extended-B, which covers all Spanish characters.
    if token.normalized.chars().any(|c| c as u32 > 0x024F) {
        return true;
    }
    false
}

fn should_skip_node(node: &ElementRef<'_>) -> bool {
    const EXCLUDED_CLASSES: &[&str] = &[
        "reflist",
        "references",
        "mw-references-wrap",
        "infobox",
        "navbox",
        "metadata",
        "catlinks",
        "toc",
        "mw-editsection",
        "coordinates",
        "authority-control",
        "mw-footer",
    ];

    const EXCLUDED_IDS: &[&str] = &["toc", "catlinks", "footer", "mw-navigation"];

    for ancestor in node.ancestors().filter_map(ElementRef::wrap) {
        if let Some(id) = ancestor.value().id() {
            if EXCLUDED_IDS.contains(&id) {
                return true;
            }
        }

        for class_name in ancestor.value().classes() {
            if EXCLUDED_CLASSES.contains(&class_name) {
                return true;
            }
        }

        if let Some(role) = ancestor.value().attr("role") {
            if role == "navigation" || role == "note" {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::extract_tokens_from_input;

    #[test]
    fn marks_anchor_text_words_as_links() {
        let html = r#"
        <html>
          <body>
            <div id="mw-content-text">
              <p>Hablan <a href="/wiki/Cebuano">cebuano</a> en esa región.</p>
              <p>También hay hablantes de tagalo fuera de la región.</p>
            </div>
          </body>
        </html>
        "#;

        let tokens = extract_tokens_from_input(html);
        let cebuano = tokens.iter().find(|t| t.normalized == "cebuano");
        let tagalo = tokens.iter().find(|t| t.normalized == "tagalo");
        assert!(cebuano.is_some_and(|t| t.is_link), "cebuano should be marked as a link");
        assert!(tagalo.is_some_and(|t| !t.is_link), "tagalo should not be marked as a link");
    }

    #[test]
    fn ignores_reference_and_navigation_content() {
        let html = r#"
        <html>
          <body>
            <div id="mw-content-text">
              <p>Fazil Mustafá fue visir otomano.</p>
              <div class="reflist"><p>the url and reference words</p></div>
              <div class="navbox"><p>navigation footer content</p></div>
            </div>
          </body>
        </html>
        "#;

        let tokens = extract_tokens_from_input(html);
        let words: Vec<String> = tokens.into_iter().map(|token| token.normalized).collect();
        assert!(words.contains(&"fazil".to_string()));
        assert!(words.contains(&"mustafá".to_string()));
        assert!(!words.contains(&"the".to_string()));
        assert!(!words.contains(&"url".to_string()));
        assert!(!words.contains(&"navigation".to_string()));
    }
}
