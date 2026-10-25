use crate::syntax::highlight;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

pub fn should_render_markdown(
    terminal: bool,
    force: bool,
    disabled: bool,
    no_color: bool,
    dumb: bool,
    command_only: bool,
) -> bool {
    !command_only && !disabled && !no_color && (force || (terminal && !dumb))
}

/// Render CommonMark as terminal text. Only renderer-generated ANSI sequences
/// reach the terminal; control sequences supplied by the model are discarded.
pub fn render_markdown(markdown: &str) -> String {
    let clean = terminal_text(markdown);
    let mut output = String::new();
    let mut styles: Vec<&str> = Vec::new();
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut links = Vec::new();
    let mut code: Option<(String, String)> = None;
    for event in Parser::new_ext(
        &clean,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    ) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((language, String::new()));
            }
            Event::Text(text) if code.is_some() => code.as_mut().unwrap().1.push_str(&text),
            Event::End(TagEnd::CodeBlock) => {
                if let Some((language, text)) = code.take() {
                    output.push_str(&highlight(&language, &text));
                    output.push_str("\x1b[0m\n");
                }
            }
            Event::Start(Tag::Heading { .. } | Tag::Strong) => {
                styles.push("\x1b[1m");
                output.push_str("\x1b[1m");
            }
            Event::Start(Tag::Emphasis) => {
                styles.push("\x1b[3m");
                output.push_str("\x1b[3m");
            }
            Event::Start(Tag::Strikethrough) => {
                styles.push("\x1b[9m");
                output.push_str("\x1b[9m");
            }
            Event::End(
                TagEnd::Heading(_) | TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough,
            ) => {
                styles.pop();
                output.push_str("\x1b[0m");
                for style in &styles {
                    output.push_str(style);
                }
                if matches!(event, Event::End(TagEnd::Heading(_))) {
                    output.push_str("\n\n");
                }
            }
            Event::Code(text) => {
                output.push_str("\x1b[36m");
                output.push_str(&text);
                output.push_str("\x1b[0m");
                for style in &styles {
                    output.push_str(style);
                }
            }
            Event::Start(Tag::List(start)) => lists.push(start),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
                output.push('\n');
            }
            Event::Start(Tag::Item) => {
                output.push_str(&"  ".repeat(lists.len().saturating_sub(1)));
                if let Some(Some(number)) = lists.last_mut() {
                    output.push_str(&format!("{number}. "));
                    *number = number.saturating_add(1);
                } else {
                    output.push_str("• ");
                }
            }
            Event::End(TagEnd::Item | TagEnd::TableHead | TagEnd::TableRow) => output.push('\n'),
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                links.push(terminal_text(&dest_url))
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if let Some(url) = links.pop() {
                    output.push_str(&format!(" ({url})"));
                }
            }
            Event::Start(Tag::BlockQuote(_)) => output.push_str("│ "),
            Event::End(TagEnd::Paragraph | TagEnd::BlockQuote(_)) => output.push_str("\n\n"),
            Event::End(TagEnd::TableCell) => output.push_str(" | "),
            Event::Text(text)
            | Event::Html(text)
            | Event::InlineHtml(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text)
            | Event::FootnoteReference(text) => output.push_str(&terminal_text(&text)),
            Event::SoftBreak | Event::HardBreak => output.push('\n'),
            Event::Rule => output.push_str("────────────────────\n"),
            Event::TaskListMarker(done) => output.push_str(if done { "[x] " } else { "[ ] " }),
            _ => {}
        }
    }
    output
}

fn terminal_text(text: &str) -> String {
    text.chars().filter(|c| {
        (!c.is_control() || matches!(c, '\n' | '\t'))
            && !matches!(c, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    }).collect()
}
