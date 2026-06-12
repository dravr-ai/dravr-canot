// ABOUTME: HTML-subset rich text parser + per-channel native markup translation
// ABOUTME: Tolerates malformed tags (treats as literal text) so user content can't break rendering
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! Channel-agnostic rich text.
//!
//! Hosts express formatting in a constrained HTML subset (`<b>`, `<i>`,
//! `<code>`). The parser turns the input into a flat span tree, and
//! per-channel render functions translate the spans into each
//! platform's native markup. Channels that don't speak HTML (Slack
//! mrkdwn, WhatsApp text formatting, Discord markdown, Messenger
//! plaintext) get the right inline syntax instead of literal `<b>`.
//!
//! Malformed input — an unclosed `<b>`, a stray `<100 bpm`, a
//! `</b>` without a matching open — is treated as literal text. This
//! keeps coach replies safe to render verbatim: a user typing
//! "HR <100 bpm" must not be interpreted as a tag.
//!
//! v1 supports a single nesting level only. `<b>outer<i>inner</i></b>`
//! parses as `Bold("outer<i>inner")` with the inner tag treated as
//! literal text. Hosts that want true nested formatting can compose
//! spans manually if a future need arises.

use std::mem;

use html_escape::encode_text;

/// A span in the parsed rich text tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    /// Plain text content
    Text(String),
    /// Bold text
    Bold(String),
    /// Italic text
    Italic(String),
    /// Inline monospace / code text
    Code(String),
}

/// Parse a rich text string into a flat span list.
///
/// Recognized tags: `<b>`, `<i>`, `<code>` and their closing forms.
/// Anything else — including malformed tags — is preserved as literal text.
#[must_use]
pub fn parse(input: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut cursor = 0usize;

    while cursor < input.len() {
        if input.as_bytes()[cursor] == b'<' {
            if let Some(parsed) = try_match_tag(&input[cursor..]) {
                if !buf.is_empty() {
                    spans.push(Span::Text(mem::take(&mut buf)));
                }
                spans.push(parsed.span);
                cursor += parsed.consumed;
                continue;
            }
        }
        // Push the next char as literal text. The `else { break }` arm
        // is unreachable while `cursor < input.len()` holds; it exists
        // purely so the parser can never panic on malformed input.
        let Some(ch) = input[cursor..].chars().next() else {
            break;
        };
        buf.push(ch);
        cursor += ch.len_utf8();
    }

    if !buf.is_empty() {
        spans.push(Span::Text(buf));
    }
    spans
}

struct ParsedTag {
    span: Span,
    consumed: usize,
}

fn try_match_tag(rest: &str) -> Option<ParsedTag> {
    for (open, close, kind) in TAG_TABLE {
        if let Some(after_open) = rest.strip_prefix(open) {
            if let Some(end_idx) = after_open.find(close) {
                let body = after_open[..end_idx].to_owned();
                let consumed = open.len() + end_idx + close.len();
                let span = match *kind {
                    TagKind::Bold => Span::Bold(body),
                    TagKind::Italic => Span::Italic(body),
                    TagKind::Code => Span::Code(body),
                };
                return Some(ParsedTag { span, consumed });
            }
        }
    }
    None
}

#[derive(Clone, Copy)]
enum TagKind {
    Bold,
    Italic,
    Code,
}

const TAG_TABLE: &[(&str, &str, TagKind)] = &[
    ("<b>", "</b>", TagKind::Bold),
    ("<i>", "</i>", TagKind::Italic),
    ("<code>", "</code>", TagKind::Code),
];

/// Render spans for Telegram's `parse_mode: "HTML"`.
///
/// Tags pass through; text nodes are HTML-escaped so a literal `<`
/// from user content doesn't open an unclosed Telegram tag.
#[must_use]
pub fn render_telegram_html(spans: &[Span]) -> String {
    let mut out = String::new();
    for span in spans {
        match span {
            Span::Text(t) => out.push_str(&encode_text(t)),
            Span::Bold(t) => {
                out.push_str("<b>");
                out.push_str(&encode_text(t));
                out.push_str("</b>");
            }
            Span::Italic(t) => {
                out.push_str("<i>");
                out.push_str(&encode_text(t));
                out.push_str("</i>");
            }
            Span::Code(t) => {
                out.push_str("<code>");
                out.push_str(&encode_text(t));
                out.push_str("</code>");
            }
        }
    }
    out
}

/// Render spans for Slack `mrkdwn` (used in section blocks).
///
/// Slack's mrkdwn parser reserves `<`, `>`, `&` for `<url|label>`
/// and entity expansion; encode them in text nodes so literal angle
/// brackets in user content can't form a malformed link.
#[must_use]
pub fn render_slack_mrkdwn(spans: &[Span]) -> String {
    let mut out = String::new();
    for span in spans {
        match span {
            Span::Text(t) => out.push_str(&escape_slack(t)),
            Span::Bold(t) => {
                out.push('*');
                out.push_str(&escape_slack(t));
                out.push('*');
            }
            Span::Italic(t) => {
                out.push('_');
                out.push_str(&escape_slack(t));
                out.push('_');
            }
            Span::Code(t) => {
                out.push('`');
                out.push_str(&escape_slack(t));
                out.push('`');
            }
        }
    }
    out
}

fn escape_slack(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Render spans for `WhatsApp` Cloud API text formatting.
///
/// `WhatsApp` recognizes `*bold*`, `_italic_`, and triple-backtick
/// blocks. Inline monospace uses single backticks (renders verbatim
/// on clients that don't format it). Text nodes are passed through;
/// `WhatsApp` treats markup chars literally unless surrounded by word
/// boundaries, so coach replies containing a stray `*` stay visible.
#[must_use]
pub fn render_whatsapp_text(spans: &[Span]) -> String {
    let mut out = String::new();
    for span in spans {
        match span {
            Span::Text(t) => out.push_str(t),
            Span::Bold(t) => {
                out.push('*');
                out.push_str(t);
                out.push('*');
            }
            Span::Italic(t) => {
                out.push('_');
                out.push_str(t);
                out.push('_');
            }
            Span::Code(t) => {
                out.push('`');
                out.push_str(t);
                out.push('`');
            }
        }
    }
    out
}

/// Render spans for Discord markdown.
///
/// Bold is `**`, italic is `*` (single), inline code is single
/// backticks. Text nodes are backslash-escaped on the four markdown
/// metacharacters so a user-typed `_` can't accidentally start an
/// italic run that extends into the next sentence.
#[must_use]
pub fn render_discord_markdown(spans: &[Span]) -> String {
    let mut out = String::new();
    for span in spans {
        match span {
            Span::Text(t) => out.push_str(&escape_discord(t)),
            Span::Bold(t) => {
                out.push_str("**");
                out.push_str(&escape_discord(t));
                out.push_str("**");
            }
            Span::Italic(t) => {
                out.push('*');
                out.push_str(&escape_discord(t));
                out.push('*');
            }
            Span::Code(t) => {
                out.push('`');
                out.push_str(t);
                out.push('`');
            }
        }
    }
    out
}

fn escape_discord(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '*' | '_' | '`') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Render spans as plain text — drop all formatting markers.
///
/// Used by channels that don't speak any rich text format
/// (e.g., Messenger plaintext).
#[must_use]
pub fn render_plain(spans: &[Span]) -> String {
    let mut out = String::new();
    for span in spans {
        match span {
            Span::Text(t) | Span::Bold(t) | Span::Italic(t) | Span::Code(t) => out.push_str(t),
        }
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
mod tests {
    use super::{
        parse, render_discord_markdown, render_plain, render_slack_mrkdwn, render_telegram_html,
        render_whatsapp_text, Span,
    };

    #[test]
    fn parses_plain_text() {
        assert_eq!(parse("hello world"), vec![Span::Text("hello world".into())]);
    }

    #[test]
    fn parses_simple_bold() {
        assert_eq!(
            parse("status is <b>enabled</b>."),
            vec![
                Span::Text("status is ".into()),
                Span::Bold("enabled".into()),
                Span::Text(".".into()),
            ]
        );
    }

    #[test]
    fn parses_mixed_bold_and_code() {
        let spans = parse("Use <code>/privacy on</code> to switch to <b>enabled</b>.");
        assert_eq!(
            spans,
            vec![
                Span::Text("Use ".into()),
                Span::Code("/privacy on".into()),
                Span::Text(" to switch to ".into()),
                Span::Bold("enabled".into()),
                Span::Text(".".into()),
            ]
        );
    }

    #[test]
    fn unclosed_tag_is_literal_text() {
        // The "<b>" has no matching "</b>" so the whole thing stays
        // as plain text. This is the coach "<100 bpm" safety case.
        assert_eq!(parse("<b>unclosed"), vec![Span::Text("<b>unclosed".into())]);
    }

    #[test]
    fn lone_lt_is_literal() {
        assert_eq!(
            parse("HR <100 bpm & pace > threshold"),
            vec![Span::Text("HR <100 bpm & pace > threshold".into())]
        );
    }

    #[test]
    fn unknown_tag_is_literal() {
        assert_eq!(
            parse("see <strong>this</strong>"),
            vec![Span::Text("see <strong>this</strong>".into())]
        );
    }

    #[test]
    fn handles_multibyte_text() {
        // Accented characters must survive byte/char bookkeeping.
        let spans = parse("consentement <b>activé</b>.");
        assert_eq!(
            spans,
            vec![
                Span::Text("consentement ".into()),
                Span::Bold("activé".into()),
                Span::Text(".".into()),
            ]
        );
    }

    #[test]
    fn telegram_html_passes_subset_and_escapes_text() {
        let spans = parse("status is <b>enabled</b> with <code>HR<100</code>");
        let rendered = render_telegram_html(&spans);
        // Bold body is preserved; the literal "<" inside <code> body
        // was parsed as literal text inside the code span and must
        // come back HTML-escaped so Telegram doesn't choke on it.
        assert_eq!(
            rendered,
            "status is <b>enabled</b> with <code>HR&lt;100</code>"
        );
    }

    #[test]
    fn telegram_html_escapes_lone_specials() {
        let rendered = render_telegram_html(&parse("HR <100 & pace > threshold"));
        assert_eq!(rendered, "HR &lt;100 &amp; pace &gt; threshold");
    }

    #[test]
    fn slack_mrkdwn_translates_subset() {
        let spans = parse("Use <code>/privacy on</code> to be <b>enabled</b>.");
        assert_eq!(
            render_slack_mrkdwn(&spans),
            "Use `/privacy on` to be *enabled*."
        );
    }

    #[test]
    fn slack_mrkdwn_escapes_angles_in_text() {
        let spans = parse("at <100 bpm & faster");
        assert_eq!(render_slack_mrkdwn(&spans), "at &lt;100 bpm &amp; faster");
    }

    #[test]
    fn whatsapp_translates_subset() {
        let spans = parse("Use <code>/privacy on</code> to be <b>enabled</b>.");
        assert_eq!(
            render_whatsapp_text(&spans),
            "Use `/privacy on` to be *enabled*."
        );
    }

    #[test]
    fn discord_markdown_translates_subset_and_escapes_text() {
        let spans = parse("Use <code>/privacy on</code> to be <b>enabled</b>.");
        assert_eq!(
            render_discord_markdown(&spans),
            "Use `/privacy on` to be **enabled**."
        );
    }

    #[test]
    fn discord_escapes_markdown_metachars_in_text() {
        let spans = parse("use _foo_ or *bar*");
        assert_eq!(render_discord_markdown(&spans), r"use \_foo\_ or \*bar\*");
    }

    #[test]
    fn plain_strips_all_tags() {
        let spans = parse("Use <code>/privacy on</code> to be <b>enabled</b>.");
        assert_eq!(render_plain(&spans), "Use /privacy on to be enabled.");
    }
}
