use std::sync::OnceLock;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

const NAMES: [&str; 10] = [
    "keyword", "string", "comment", "function", "constant", "type", "number", "operator",
    "property", "variable",
];
const COLORS: [&str; 10] = [
    "\x1b[38;2;198;120;221m",
    "\x1b[38;2;152;195;121m",
    "\x1b[38;2;127;132;142m",
    "\x1b[38;2;97;175;239m",
    "\x1b[38;2;209;154;102m",
    "\x1b[38;2;229;192;123m",
    "\x1b[38;2;209;154;102m",
    "\x1b[38;2;86;182;194m",
    "\x1b[38;2;224;108;117m",
    "\x1b[38;2;171;178;191m",
];

fn configuration(language: &str) -> Option<&'static HighlightConfiguration> {
    static RUST: OnceLock<Option<HighlightConfiguration>> = OnceLock::new();
    static BASH: OnceLock<Option<HighlightConfiguration>> = OnceLock::new();
    static PYTHON: OnceLock<Option<HighlightConfiguration>> = OnceLock::new();
    static JAVASCRIPT: OnceLock<Option<HighlightConfiguration>> = OnceLock::new();
    static JSON: OnceLock<Option<HighlightConfiguration>> = OnceLock::new();
    let (cache, grammar, query) = match language.to_ascii_lowercase().as_str() {
        "rust" | "rs" => (
            &RUST,
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY,
        ),
        "bash" | "sh" | "shell" | "zsh" => (
            &BASH,
            tree_sitter_bash::LANGUAGE.into(),
            tree_sitter_bash::HIGHLIGHT_QUERY,
        ),
        "python" | "py" => (
            &PYTHON,
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY,
        ),
        "javascript" | "js" => (
            &JAVASCRIPT,
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY,
        ),
        "json" => (
            &JSON,
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY,
        ),
        _ => return None,
    };
    cache
        .get_or_init(|| {
            HighlightConfiguration::new(grammar, language, query, "", "")
                .ok()
                .map(|mut config| {
                    config.configure(&NAMES);
                    config
                })
        })
        .as_ref()
}

pub(crate) fn highlight(language: &str, code: &str) -> String {
    try_highlight(language, code).unwrap_or_else(|| code.to_owned())
}

fn try_highlight(language: &str, code: &str) -> Option<String> {
    let config = configuration(language)?;
    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(config, code.as_bytes(), None, None, |_| None)
        .ok()?;
    let mut output = String::new();
    let mut styles = Vec::new();
    for event in events {
        match event.ok()? {
            HighlightEvent::Source { start, end } => output.push_str(code.get(start..end)?),
            HighlightEvent::HighlightStart(style) => {
                let color = *COLORS.get(style.0)?;
                styles.push(color);
                output.push_str(color);
            }
            HighlightEvent::HighlightEnd => {
                styles.pop();
                output.push_str(styles.last().copied().unwrap_or("\x1b[39m"));
            }
        }
    }
    Some(output)
}
