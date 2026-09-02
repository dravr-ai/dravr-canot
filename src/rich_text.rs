// ABOUTME: HTML-subset rich text parser, its inline-markdown twin, and per-channel native markup translation
// ABOUTME: Tolerates malformed tags and stray markdown (treated as literal text) so user content can't break rendering
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
//!
//! A host that authors its strings in inline markdown instead — `**bold**`,
//! `*italic*`, `` `code` `` — reads them with [`parse_markdown`] and hands
//! the channels the dialect they translate with [`render_rich_text`]:
//! `render_rich_text(&parse_markdown(text))`. The two notations describe the
//! same span tree, and [`render_discord_markdown`] is the inverse direction.

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
            Span::Text(t) => out.push_str(&escape_markdown(t)),
            Span::Bold(t) => {
                out.push_str("**");
                out.push_str(&escape_markdown(t));
                out.push_str("**");
            }
            Span::Italic(t) => {
                out.push('*');
                out.push_str(&escape_markdown(t));
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

/// Backslash-escape the four characters the markdown subset reads.
///
/// For free text a host interpolates into a markdown template — an athlete's
/// display name inside `**{name}**` — so a `*` or `_` they typed stays a
/// character instead of opening a run. [`parse_markdown`] reads the escapes
/// back, and [`render_discord_markdown`] applies the same rule to its text
/// nodes.
#[must_use]
pub fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if MARKDOWN_ESCAPABLE.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The characters a backslash escapes in the markdown subset.
const MARKDOWN_ESCAPABLE: [char; 4] = ['\\', '*', '_', '`'];

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

/// Parse the inline-markdown subset into the same span tree [`parse`] builds.
///
/// Recognized: `**bold**`, `*italic*`, `` `code` ``, and a backslash before
/// `\\`, `*`, `_` or `` ` `` standing for that character itself — exactly
/// what [`render_discord_markdown`] emits, so the two are inverses for a
/// well-formed tree. An opener with no closer, a lone `*`, an empty pair
/// (`****`) all stay literal text: an athlete's `5 x 400m*` cannot break a
/// message. Block syntax — a `- ` list, a `#` heading, `_underscore_`
/// emphasis — is text too, so a listing stays a readable plain list on every
/// channel. One nesting level, like [`parse`]: `**outer *inner* outer**`
/// keeps the inner asterisks as text. Code bodies are taken verbatim; bold
/// and italic bodies have their backslash escapes resolved.
#[must_use]
pub fn parse_markdown(input: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut rest = input;

    while let Some(ch) = rest.chars().next() {
        if ch == '\\' {
            if let Some(escaped) = rest[1..]
                .chars()
                .next()
                .filter(|c| MARKDOWN_ESCAPABLE.contains(c))
            {
                buf.push(escaped);
                rest = &rest[2..];
                continue;
            }
        }
        if let Some((span, consumed)) = try_match_markdown(rest) {
            if !buf.is_empty() {
                spans.push(Span::Text(mem::take(&mut buf)));
            }
            spans.push(span);
            rest = &rest[consumed..];
            continue;
        }
        buf.push(ch);
        rest = &rest[ch.len_utf8()..];
    }

    if !buf.is_empty() {
        spans.push(Span::Text(buf));
    }
    spans
}

/// The delimiters of the markdown subset, longest first so `**` is never
/// read as two italic openers.
const MARKDOWN_TABLE: &[(&str, TagKind)] = &[
    ("**", TagKind::Bold),
    ("*", TagKind::Italic),
    ("`", TagKind::Code),
];

/// One delimited run at the head of `rest`, with the bytes it consumed.
///
/// Emphasis follows `CommonMark`'s flanking rule in its simplest form: the
/// opener must touch the text it opens and the closer the text it closes, so
/// `400m* then 2*3` has no italic run in it — the first `*` is followed by a
/// space and the second is never closed. A delimiter is also read whole: a
/// `*` that is part of a longer run (`***`, `****`) opens or closes nothing,
/// so a stray run stays text instead of becoming an italic asterisk. A code
/// span has neither rule.
fn try_match_markdown(rest: &str) -> Option<(Span, usize)> {
    for (delim, kind) in MARKDOWN_TABLE {
        let Some(after_open) = rest.strip_prefix(*delim) else {
            continue;
        };
        let flanking = !matches!(kind, TagKind::Code);
        if flanking && after_open.starts_with(|c: char| c.is_whitespace() || c == '*') {
            continue;
        }
        let Some(end) = find_closer(after_open, delim, flanking) else {
            continue;
        };
        let body = &after_open[..end];
        let span = match kind {
            TagKind::Bold => Span::Bold(unescape_markdown(body)),
            TagKind::Italic => Span::Italic(unescape_markdown(body)),
            TagKind::Code => Span::Code(body.to_owned()),
        };
        return Some((span, delim.len() + end + delim.len()));
    }
    None
}

/// The first `delim` in `hay` that closes a non-empty run: not preceded by a
/// backslash and, for emphasis, touching the text before it and not part of
/// a longer asterisk run. `delim` is ASCII, so every index this returns is a
/// char boundary.
fn find_closer(hay: &str, delim: &str, flanking: bool) -> Option<usize> {
    let mut from = 0;
    while let Some(pos) = hay[from..].find(delim) {
        let idx = from + pos;
        let escaped = idx > 0 && hay.as_bytes()[idx - 1] == b'\\';
        let before = &hay[..idx];
        let after = &hay[idx + delim.len()..];
        let not_flanking = flanking
            && (before.ends_with(char::is_whitespace)
                || before.ends_with('*')
                || after.starts_with('*'));
        if idx > 0 && !escaped && !not_flanking {
            return Some(idx);
        }
        from = idx + delim.len();
    }
    None
}

/// Resolve the backslash escapes inside an emphasis body.
fn unescape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars
                .peek()
                .copied()
                .filter(|c| MARKDOWN_ESCAPABLE.contains(c))
            {
                out.push(next);
                chars.next();
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// Render spans in the HTML-subset dialect [`parse`] reads.
///
/// The string every channel translator accepts, so a host that parsed
/// markdown hands the channels `render_rich_text(&parse_markdown(text))`.
/// Text nodes are emitted verbatim: the dialect has no escape sequence, so a
/// text node that itself spells a complete `<b>…</b>` re-parses as one —
/// the same latitude the dialect gives every author of it.
#[must_use]
pub fn render_rich_text(spans: &[Span]) -> String {
    let mut out = String::new();
    for span in spans {
        match span {
            Span::Text(t) => out.push_str(t),
            Span::Bold(t) => {
                out.push_str("<b>");
                out.push_str(t);
                out.push_str("</b>");
            }
            Span::Italic(t) => {
                out.push_str("<i>");
                out.push_str(t);
                out.push_str("</i>");
            }
            Span::Code(t) => {
                out.push_str("<code>");
                out.push_str(t);
                out.push_str("</code>");
            }
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
        escape_markdown, parse, parse_markdown, render_discord_markdown, render_plain,
        render_rich_text, render_slack_mrkdwn, render_telegram_html, render_whatsapp_text, Span,
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

    #[test]
    fn markdown_parses_bold_italic_and_code() {
        assert_eq!(
            parse_markdown("Use `/privacy on` to be **enabled**, *soon*."),
            vec![
                Span::Text("Use ".into()),
                Span::Code("/privacy on".into()),
                Span::Text(" to be ".into()),
                Span::Bold("enabled".into()),
                Span::Text(", ".into()),
                Span::Italic("soon".into()),
                Span::Text(".".into()),
            ]
        );
    }

    #[test]
    fn markdown_reads_the_same_tree_as_the_html_subset() {
        assert_eq!(
            parse_markdown("status is **enabled** with `HR<100`"),
            parse("status is <b>enabled</b> with <code>HR<100</code>")
        );
    }

    #[test]
    fn markdown_unclosed_and_lone_markers_are_literal() {
        assert_eq!(
            parse_markdown("**unclosed"),
            vec![Span::Text("**unclosed".into())]
        );
        assert_eq!(
            parse_markdown("5 x 400m* then 2*3"),
            vec![Span::Text("5 x 400m* then 2*3".into())]
        );
        assert_eq!(parse_markdown("****"), vec![Span::Text("****".into())]);
        assert_eq!(parse_markdown("``"), vec![Span::Text("``".into())]);
    }

    #[test]
    fn markdown_emphasis_must_touch_its_text() {
        assert_eq!(
            parse_markdown("a * b * c and ** d **"),
            vec![Span::Text("a * b * c and ** d **".into())]
        );
        assert_eq!(
            parse_markdown("x *y * z*"),
            vec![Span::Text("x ".into()), Span::Italic("y * z".into())]
        );
    }

    #[test]
    fn markdown_block_syntax_and_underscores_stay_text() {
        let listing = "**Account**\n- /privacy — consent\n- /timezone — clock\n_snake_case_";
        assert_eq!(
            parse_markdown(listing),
            vec![
                Span::Bold("Account".into()),
                Span::Text("\n- /privacy — consent\n- /timezone — clock\n_snake_case_".into()),
            ]
        );
    }

    #[test]
    fn markdown_honours_backslash_escapes() {
        assert_eq!(
            parse_markdown(r"a \*literal\* star, **bold \* inside** and \\ back"),
            vec![
                Span::Text("a *literal* star, ".into()),
                Span::Bold("bold * inside".into()),
                Span::Text(" and \\ back".into()),
            ]
        );
    }

    #[test]
    fn markdown_single_nesting_level() {
        assert_eq!(
            parse_markdown("**outer *inner* outer**"),
            vec![Span::Bold("outer *inner* outer".into())]
        );
    }

    #[test]
    fn markdown_handles_multibyte_text() {
        assert_eq!(
            parse_markdown("consentement **activé**."),
            vec![
                Span::Text("consentement ".into()),
                Span::Bold("activé".into()),
                Span::Text(".".into()),
            ]
        );
    }

    #[test]
    fn rich_text_render_is_the_inverse_of_parse() {
        let dialect = "Use <code>/privacy on</code> to be <b>enabled</b>, <i>soon</i>.";
        assert_eq!(render_rich_text(&parse(dialect)), dialect);
        assert_eq!(
            render_rich_text(&parse_markdown(
                "Use `/privacy on` to be **enabled**, *soon*."
            )),
            dialect
        );
    }

    #[test]
    fn markdown_round_trips_through_the_discord_renderer() {
        let spans = vec![
            Span::Text("use _foo_ or *bar* with ".into()),
            Span::Bold("Marc <3 vélo & co".into()),
            Span::Code("/plan".into()),
        ];
        assert_eq!(parse_markdown(&render_discord_markdown(&spans)), spans);
    }

    #[test]
    fn escape_markdown_neutralises_the_four_metacharacters() {
        assert_eq!(escape_markdown(r"a*b_c`d\e"), r"a\*b\_c\`d\\e");
        assert_eq!(
            parse_markdown(&format!("**{}**", escape_markdown("Marc *le* rapide"))),
            vec![Span::Bold("Marc *le* rapide".into())]
        );
    }
}
