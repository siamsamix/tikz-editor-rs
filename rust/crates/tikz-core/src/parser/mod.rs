//! `parse_tex(source) -> [`ParsedDocument`] facade.
//!
//! Port of `packages/core/src/parser/index.ts` (figure scan → lex → CST-to-AST).
//!
//! This slice parses the document into figures and, within each figure, the
//! path statements the semantic layer already renders
//! (`\draw`/`\path`/`\fill`/`\filldraw`/`\shade`/`\shadedraw`) into
//! [`Statement::Path`] items. Every other statement head becomes
//! [`Statement::Unknown`] with its span, so nothing is dropped silently — the
//! node/style/foreach statements that Phase 2 renders are intentionally
//! deferred here rather than mis-parsed.
//!
//! Spans follow the legacy dump contract: a statement span covers the command
//! through its terminating `;`, and item spans cover their source text
//! (coordinate spans include the wrapping parens).

use crate::figure_scan::{scan_tikz_figures, ScannedFigure};
use crate::ids::path_statement_id;
use crate::item_types::{
    CoordinateForm, CoordinateItem, PathCommentItem, PathKeywordItem, PathOptionItem,
    RelativeCoordinatePrefix, UnknownPathItem,
};
use crate::items::PathItem;
use crate::items_ops::GroupOptionSource;
use crate::items_ops::NodeItem;
use crate::options::{
    parse_option_list_raw, FlagEntry, KvEntry, OptionEntry, OptionListAst, UnknownEntry,
};
use crate::semantic::pathscan::matching;
use crate::span::Span;
use serde::Serialize;

/// Commands whose bodies are path statements this slice parses into items.
const PATH_COMMANDS: &[&str] = &[
    "draw",
    "path",
    "fill",
    "filldraw",
    "shade",
    "shadedraw",
    "node",
];

/// The parsed document: one figure per `tikzpicture`-like environment.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedDocument {
    /// Figures in document order.
    pub figures: Vec<ParsedFigure>,
    /// Document-level diagnostics (e.g. no/multiple figures).
    pub diagnostics: Vec<ParsedDiagnostic>,
}

/// One scanned figure plus its parsed statement list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedFigure {
    /// Span covering the whole environment.
    pub span: Span,
    /// Environment name, e.g. `tikzpicture`.
    pub environment: String,
    /// Span of the `\begin{…}` token.
    pub begin_span: Span,
    /// Span of the `\end{…}` token.
    pub end_span: Span,
    /// Figure-level option list (`[..]` after `\begin`).
    pub options: Option<ParsedOptionList>,
    /// Top-level statements in order.
    pub statements: Vec<ParsedStatement>,
}

/// One `;`-terminated statement.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedStatement {
    /// `Path` or `Unknown`.
    pub kind: String,
    /// Statement id (`path:N` or `unknown-statement:N`).
    pub id: String,
    /// Span covering the statement through its terminating `;`.
    pub span: Span,
    /// The command keyword (lowercased) for path statements.
    pub command: Option<String>,
    /// Path items in order (empty for unknown statements).
    pub items: Vec<ParsedPathItem>,
}

/// A parsed option list (`[..]`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedOptionList {
    /// Span covering the whole option list including brackets.
    pub span: Span,
    /// Trimmed raw text.
    pub raw: String,
    /// Entries in order.
    pub entries: Vec<ParsedOptionEntry>,
}

/// One option-list entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedOptionEntry {
    /// `kv`, `flag` or `unknown`.
    pub kind: String,
    /// Span covering the token.
    pub span: Span,
    /// Lower-cased key (kv/flag).
    pub key: Option<String>,
    /// Trimmed value (kv only).
    pub value_raw: Option<String>,
    /// Raw token text.
    pub raw: String,
}

/// One item inside a path statement.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedPathItem {
    /// `Coordinate`, `PathKeyword`, `PathOption`, `PathComment`, `Node` or `Unknown`.
    pub kind: String,
    /// Item id.
    pub id: String,
    /// Span covering the item.
    pub span: Span,
    /// Raw source text.
    pub raw: String,
    /// X component text (coordinate).
    pub x: Option<String>,
    /// Y component text (coordinate).
    pub y: Option<String>,
    /// Coordinate form (`cartesian`, `polar`, `named`).
    pub form: Option<String>,
    /// Keyword text (path-keyword).
    pub keyword: Option<String>,
    /// Parsed options (path-option).
    pub options: Option<ParsedOptionList>,
    /// Node text (node item).
    pub text: Option<String>,
    /// Span of the node text inner group (node item).
    pub text_span: Option<Span>,
    /// Node name from the leading named coordinate (node item).
    pub name: Option<String>,
    /// Full raw node payload echoed (node item).
    pub template_raw: Option<String>,
}

/// One diagnostic with severity and span.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedDiagnostic {
    /// `error` or `warning`.
    pub severity: String,
    /// Span covering the offending region.
    pub span: Span,
    /// Human-readable reason.
    pub message: String,
}

/// Parse `source` into figures plus diagnostics.
pub fn parse_tex(source: &str) -> ParsedDocument {
    let figures = scan_tikz_figures(source);
    let mut out = Vec::with_capacity(figures.len());
    let mut diagnostics = Vec::new();

    match figures.as_slice() {
        [] => {
            diagnostics.push(ParsedDiagnostic {
                severity: "error".to_string(),
                span: Span::new(0, source.len()),
                message: "no tikzpicture found".to_string(),
            });
        }
        [_, ..] => {
            for figure in &figures {
                out.push(parse_figure(source, figure, &mut diagnostics));
            }
        }
    }

    ParsedDocument {
        figures: out,
        diagnostics,
    }
}

fn parse_figure(
    source: &str,
    figure: &ScannedFigure,
    diagnostics: &mut Vec<ParsedDiagnostic>,
) -> ParsedFigure {
    let begin_to = figure.begin_span.to;
    let end_from = figure.end_span.from;
    let mut cursor = skip_trivia(source, begin_to, end_from);

    let options = if source[cursor..end_from].starts_with('[') {
        let close = match matching(source, cursor, end_from, b'[', b']') {
            Some(close) => close,
            None => {
                diagnostics.push(ParsedDiagnostic {
                    severity: "error".to_string(),
                    span: Span::new(cursor, end_from),
                    message: "unterminated figure options".to_string(),
                });
                end_from
            }
        };
        let list = parse_option_list_raw(&source[cursor..=close], cursor);
        cursor = close + 1;
        Some(convert_option_list(list))
    } else {
        None
    };

    let mut statements = Vec::new();
    for (index, (from, to, terminated)) in split_statements(source, cursor, end_from)
        .into_iter()
        .enumerate()
    {
        let statement_index = index;
        let id = path_statement_id(statement_index);
        if !terminated {
            diagnostics.push(ParsedDiagnostic {
                severity: "error".to_string(),
                span: Span::new(from, to),
                message: "statement is missing its terminating `;`".to_string(),
            });
            continue;
        }
        statements.push(parse_statement(source, statement_index, id, from, to));
    }

    ParsedFigure {
        span: figure.span,
        environment: figure.environment.clone(),
        begin_span: figure.begin_span,
        end_span: figure.end_span,
        options,
        statements,
    }
}

/// Split `[from, end)` at top-level `;` into `(from_inclusive, to_inclusive, terminated)`.
///
/// `to_inclusive` is one past the terminating `;` when present, matching the
/// legacy statement-dump contract. `from_inclusive` skips leading trivia so
/// statement spans start at their command word, not at leftover whitespace.
///
/// `\begin{scope}...\end{scope}` bodies are treated as atomic: `;` inside them
/// does not split statements, matching the legacy splitter.
fn split_statements(src: &str, from: usize, end: usize) -> Vec<(usize, usize, bool)> {
    let bytes = src.as_bytes();
    let (mut out, mut start, mut depth, mut scope_depth, mut at) =
        (Vec::new(), from, 0i32, 0i32, from);
    while at < end {
        // Skip bytes that fall inside a multi-byte UTF-8 sequence (fuzz input).
        while at < end && !src.is_char_boundary(at) {
            at += 1;
        }
        if at >= end {
            break;
        }
        match bytes[at] {
            b'%' => {
                while at < end && bytes[at] != b'\n' {
                    at += 1;
                }
                continue;
            }
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b';' if depth <= 0 && scope_depth == 0 => {
                out.push((skip_trivia(src, start, at + 1), at + 1, true));
                start = at + 1;
            }
            _ => {}
        }
        // Detect scope boundaries outside braces. A scope statement terminates
        // at its matching `\end{scope}`, not at an internal `;`.
        if bytes[at] == b'\\' && src.is_char_boundary(at) {
            let mut scan_end = end.min(at + 16);
            while scan_end > at && !src.is_char_boundary(scan_end) {
                scan_end -= 1;
            }
            if scan_end <= at {
                at += 1;
                continue;
            }
            let rest = &src[at..scan_end];
            if rest.starts_with("\\begin{scope}") {
                scope_depth += 1;
                at += "\\begin{scope}".len() - 1;
                continue;
            }
            if rest.starts_with("\\end{scope}") && scope_depth > 0 {
                scope_depth -= 1;
                // Emit the scope statement as terminated right after \end{scope}.
                out.push((
                    skip_trivia(src, start, at + "\\end{scope}".len()),
                    at + "\\end{scope}".len(),
                    true,
                ));
                start = at + "\\end{scope}".len();
                at += "\\end{scope}".len() - 1;
                continue;
            }
        }
        at += 1;
    }
    if skip_trivia(src, start, end) < end {
        out.push((skip_trivia(src, start, end), end, false));
    }
    out
}

/// Parse one statement spanning `[from, to_inclusive)` (the `;`, if any, is at
/// `to_inclusive - 1`).
fn parse_statement(
    source: &str,
    statement_index: usize,
    id: String,
    from: usize,
    to_inclusive: usize,
) -> ParsedStatement {
    let body_end = if to_inclusive > from && source.as_bytes()[to_inclusive - 1] == b';' {
        to_inclusive - 1
    } else {
        to_inclusive
    };
    let at = skip_trivia(source, from, body_end);
    if at >= body_end {
        return ParsedStatement {
            kind: "Path".to_string(),
            id,
            span: Span::new(from, to_inclusive),
            command: None,
            items: Vec::new(),
        };
    }

    let (command, after) = read_command(source, at, body_end);

    // Scope statement: `\begin{scope}[…] … \end{scope}`. `split_statements`
    // already terminates the span at the matching `\end{scope}`, so emit it
    // with no items (scope bodies are evaluated separately, Phase 2).
    if command == "begin" {
        let head = source[after..to_inclusive].trim_start();
        if head.starts_with("{scope}") {
            return ParsedStatement {
                kind: "Scope".to_string(),
                id,
                span: Span::new(from, to_inclusive),
                command: None,
                items: Vec::new(),
            };
        }
    }

    // `\foreach … {…}` statement: spans to the statement terminator already
    // captured by `split_statements`; body parsing is deferred to Phase 2.
    if command == "foreach" {
        return ParsedStatement {
            kind: "Foreach".to_string(),
            id,
            span: Span::new(from, to_inclusive),
            command: None,
            items: Vec::new(),
        };
    }

    let is_path = PATH_COMMANDS.contains(&command);

    if !is_path {
        return ParsedStatement {
            kind: "Path".to_string(),
            id,
            span: Span::new(from, to_inclusive),
            command: Some(command.to_string()),
            items: Vec::new(),
        };
    }

    let mut cursor = skip_trivia(source, after, body_end);
    // Statement-level option lists are re-parsed inline as PathOption items
    // while scanning items, so no separate options binding survives here.
    let mut items = Vec::new();
    let mut item_index = 0usize;
    // For `\node` statements: the first `[options]` start anchors the Node item
    // span, the leading named coordinate yields `name`, and the trailing
    // `{text}` group becomes the Node item.
    let mut options_start: Option<usize> = None;
    let mut name: Option<String> = None;
    let mut text_group: Option<Span> = None;
    while cursor < body_end {
        // Skip bytes that are not char boundaries (invalid UTF-8 from fuzz input).
        while cursor < body_end && !source.is_char_boundary(cursor) {
            cursor += 1;
        }
        if cursor >= body_end {
            break;
        }
        let byte = source.as_bytes()[cursor];
        if byte == b'%' {
            let line_end = source[cursor..body_end]
                .find('\n')
                .map_or(body_end, |n| cursor + n);
            items.push(path_item(
                statement_index,
                &mut item_index,
                "PathComment",
                Span::new(cursor, line_end),
                &source[cursor..line_end],
                None,
                None,
                None,
                None,
                None,
            ));
            cursor = line_end;
            continue;
        }
        if byte.is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        if byte == b'[' {
            let close = matching(source, cursor, body_end, b'[', b']').unwrap_or(body_end);
            if command == "node" && options_start.is_none() {
                options_start = Some(cursor);
            }
            let raw = &source[cursor..=close];
            let list = parse_option_list_raw(raw, cursor);
            items.push(path_item(
                statement_index,
                &mut item_index,
                "PathOption",
                Span::new(cursor, close + 1),
                raw,
                None,
                None,
                None,
                Some(convert_option_list(list)),
                None,
            ));
            cursor = close + 1;
            continue;
        }
        if byte == b'(' {
            let close = matching(source, cursor, body_end, b'(', b')').unwrap_or(body_end);
            let raw = &source[cursor..=close];
            let inner = &source[cursor + 1..close];
            let (x, y, form) = parse_coordinate_text(inner);
            if form == "named" && name.is_none() {
                name = Some(x.clone());
            }
            items.push(path_item(
                statement_index,
                &mut item_index,
                "Coordinate",
                Span::new(cursor, close + 1),
                raw,
                Some(x),
                Some(y),
                Some(form),
                None,
                None,
            ));
            cursor = close + 1;
            continue;
        }
        if byte == b'{' && command == "node" {
            // The trailing `{text}` group becomes the Node item; record its
            // span and stop scanning (nothing meaningful follows it).
            if let Some(close) = matching(source, cursor, body_end, b'{', b'}') {
                text_group = Some(Span::new(cursor, close + 1));
                cursor = close + 1;
            } else {
                cursor += 1;
            }
            continue;
        }
        // Path operators `--`, `|-`, `-|`, `..`.
        if source[cursor..].starts_with("--")
            || source[cursor..].starts_with("|-")
            || source[cursor..].starts_with("-|")
            || source[cursor..].starts_with("..")
        {
            let op = &source[cursor..cursor + 2];
            items.push(path_item(
                statement_index,
                &mut item_index,
                "PathKeyword",
                Span::new(cursor, cursor + 2),
                op,
                None,
                None,
                None,
                None,
                Some(op.to_string()),
            ));
            cursor += 2;
            continue;
        }
        let (word, after) = read_word(source, cursor, body_end);
        if word.is_empty() {
            cursor = (cursor + 1).min(body_end);
            continue;
        }
        let keyword_span = Span::new(cursor, after);
        let keyword = match word {
            "--" | "|-" | "-|" | ".." => (word.to_string(), keyword_span),
            "at" | "bend" | "cycle" | "circle" | "controls" | "and" | "rectangle" | "ellipse"
            | "arc" | "grid" | "plot" | "coordinates" | "parabola" | "sin" | "cos" => {
                (word.to_string(), keyword_span)
            }
            "node" | "pic" => {
                // Inline `node`/`pic` within a path statement may carry optional
                // `[options]` and a trailing `{text}` group. Consume them as part
                // of this keyword item so the evaluator sees one atomic item.
                let item_start = cursor;
                let mut item_end = after;
                while item_end < body_end {
                    item_end = skip_trivia(source, item_end, body_end);
                    if source[item_end..].starts_with('[') {
                        if let Some(close) = matching(source, item_end, body_end, b'[', b']') {
                            item_end = close + 1;
                            continue;
                        }
                    }
                    if source[item_end..].starts_with('{') {
                        if let Some(close) = matching(source, item_end, body_end, b'{', b'}') {
                            item_end = close + 1;
                            continue;
                        }
                    }
                    break;
                }
                let span = Span::new(item_start, item_end);
                (word.to_string(), span)
            }
            _ => {
                items.push(path_item(
                    statement_index,
                    &mut item_index,
                    "Unknown",
                    keyword_span,
                    word,
                    None,
                    None,
                    None,
                    None,
                    None,
                ));
                cursor = after;
                continue;
            }
        };
        items.push(path_item(
            statement_index,
            &mut item_index,
            "PathKeyword",
            keyword.1,
            &keyword.0,
            None,
            None,
            None,
            None,
            Some(keyword.0.clone()),
        ));
        // store keyword text
        if let Some(last) = items.last_mut() {
            last.keyword = Some(keyword.0.clone());
        }
        cursor = keyword.1.to;
    }

    // Emit the trailing `{text}` group as a Node item for `\node` statements.
    if command == "node" {
        if let Some(text_span) = text_group {
            let node_from = options_start.unwrap_or(text_span.from);
            let node_to = text_span.to;
            let raw = &source[node_from..node_to];
            let inner_from = text_span.from + 1;
            let inner_to = text_span.to.saturating_sub(1);
            let text = source[inner_from..inner_to].to_string();
            let text_span = Span::new(inner_from, inner_to);
            items.push(ParsedPathItem {
                kind: "Node".to_string(),
                id: format!("node:{statement_index}:{item_index}"),
                span: Span::new(node_from, node_to),
                raw: raw.to_string(),
                x: None,
                y: None,
                form: None,
                keyword: None,
                options: None,
                text: Some(text),
                text_span: Some(text_span),
                name,
                template_raw: Some(raw.to_string()),
            });
        }
    }

    ParsedStatement {
        kind: "Path".to_string(),
        id,
        span: Span::new(from, to_inclusive),
        command: Some(command.to_string()),
        items,
    }
}

/// Read a `\name` command, returning its name (without backslash) and the index
/// just past the command word.
fn read_command(source: &str, at: usize, to: usize) -> (&str, usize) {
    let bytes = source.as_bytes();
    if bytes.get(at) != Some(&b'\\') {
        return ("", at);
    }
    let mut end = at + 1;
    while end < to && (bytes[end].is_ascii_alphabetic() || bytes[end] == b'*') {
        end += 1;
    }
    (&source[at + 1..end], end)
}

/// Read a bare word of alphabetic/underscore characters.
fn read_word(source: &str, at: usize, to: usize) -> (&str, usize) {
    let bytes = source.as_bytes();
    let mut end = at;
    while end < to && (bytes[end].is_ascii_alphabetic() || bytes[end] == b'_') {
        end += 1;
    }
    (&source[at..end], end)
}

/// Parse coordinate inner text into (x, y, form).
fn parse_coordinate_text(inner: &str) -> (String, String, String) {
    let text = inner.trim();
    if let Some((angle, radius)) = text.split_once(':') {
        return (
            angle.trim().to_string(),
            radius.trim().to_string(),
            "polar".to_string(),
        );
    }
    if let Some((x, y)) = text.split_once(',') {
        return (
            x.trim().to_string(),
            y.trim().to_string(),
            "cartesian".to_string(),
        );
    }
    if text.is_empty() {
        return (String::new(), String::new(), "unknown".to_string());
    }
    (text.to_string(), String::new(), "named".to_string())
}

#[allow(clippy::too_many_arguments)]
fn path_item(
    statement_index: usize,
    index: &mut usize,
    kind: &str,
    span: Span,
    raw: &str,
    x: Option<String>,
    y: Option<String>,
    form: Option<String>,
    options: Option<ParsedOptionList>,
    keyword: Option<String>,
) -> ParsedPathItem {
    let id = format!("{kind}:{statement_index}:{index}");
    *index += 1;
    ParsedPathItem {
        kind: kind.to_string(),
        id,
        span,
        raw: raw.to_string(),
        x,
        y,
        form,
        keyword,
        options,
        text: None,
        text_span: None,
        name: None,
        template_raw: None,
    }
}

/// Convert an [`crate::options::OptionListAst`] into the serializable
/// [`ParsedOptionList`] shape.
fn convert_option_list(list: crate::options::OptionListAst) -> ParsedOptionList {
    ParsedOptionList {
        span: list.span,
        raw: list.raw,
        entries: list
            .entries
            .into_iter()
            .map(|entry| match entry {
                crate::options::OptionEntry::KV(kv) => ParsedOptionEntry {
                    kind: "kv".to_string(),
                    span: kv.span,
                    key: Some(kv.key),
                    value_raw: Some(kv.value_raw),
                    raw: kv.raw,
                },
                crate::options::OptionEntry::Flag(flag) => ParsedOptionEntry {
                    kind: "flag".to_string(),
                    span: flag.span,
                    key: Some(flag.key),
                    value_raw: None,
                    raw: flag.raw,
                },
                crate::options::OptionEntry::Unknown(unknown) => ParsedOptionEntry {
                    kind: "unknown".to_string(),
                    span: unknown.span,
                    key: None,
                    value_raw: None,
                    raw: unknown.raw,
                },
            })
            .collect(),
    }
}

/// Convert a [`ParsedOptionList`] into an [`OptionListAst`] for the AST path item.
pub(crate) fn convert_parsed_option_list(list: ParsedOptionList) -> OptionListAst {
    OptionListAst {
        span: list.span,
        raw: list.raw,
        entries: list
            .entries
            .into_iter()
            .map(|entry| {
                let ParsedOptionEntry {
                    kind,
                    span,
                    key,
                    value_raw,
                    raw,
                } = entry;
                if kind == "kv" {
                    OptionEntry::KV(KvEntry {
                        key: key.unwrap_or_default(),
                        value_raw: value_raw.unwrap_or_default(),
                        span,
                        key_span: span,
                        value_span: None,
                        raw,
                    })
                } else if kind == "flag" {
                    OptionEntry::Flag(FlagEntry {
                        key: key.unwrap_or_default(),
                        span,
                        key_span: span,
                        raw,
                    })
                } else {
                    OptionEntry::Unknown(UnknownEntry { span, raw })
                }
            })
            .collect(),
    }
}

/// Detect the `+`/`++` relative prefix immediately before a coordinate span.
fn relative_prefix_for(source: &str, span: Span) -> Option<RelativeCoordinatePrefix> {
    if span.from >= 2 {
        let start = span.from - 2;
        if source.is_char_boundary(start)
            && source.is_char_boundary(span.from)
            && &source[start..span.from] == "++"
        {
            return Some(RelativeCoordinatePrefix::PlusPlus);
        }
    }
    if span.from >= 1
        && source.is_char_boundary(span.from - 1)
        && source.is_char_boundary(span.from)
        && source.as_bytes()[span.from - 1] == b'+'
    {
        return Some(RelativeCoordinatePrefix::Plus);
    }
    None
}

/// Map a parsed coordinate form string to [`CoordinateForm`].
fn map_coordinate_form(form: Option<&str>) -> CoordinateForm {
    match form {
        Some("cartesian") => CoordinateForm::Cartesian,
        Some("polar") => CoordinateForm::Polar,
        Some("named") => CoordinateForm::Named,
        _ => CoordinateForm::Unknown,
    }
}

/// Convert one [`ParsedPathItem`] into a [`PathItem`] AST node.
fn parsed_path_item_to_path_item(
    source: &str,
    item: &ParsedPathItem,
    index: &mut usize,
) -> PathItem {
    let id = item.id.clone();
    let span = item.span;
    *index += 1;
    match item.kind.as_str() {
        "Coordinate" => PathItem::Coordinate(CoordinateItem {
            id,
            span,
            options_span: None,
            options: None,
            relative_prefix: relative_prefix_for(source, span),
            x: item.x.clone().unwrap_or_default(),
            y: item.y.clone().unwrap_or_default(),
            z: None,
            raw: item.raw.clone(),
            form: map_coordinate_form(item.form.as_deref()),
        }),
        "PathKeyword" => PathItem::PathKeyword(PathKeywordItem {
            id,
            span,
            keyword: item.keyword.clone().unwrap_or_default(),
        }),
        "PathOption" => PathItem::PathOption(PathOptionItem {
            id,
            span,
            raw: item.raw.clone(),
            options: item
                .options
                .as_ref()
                .map(|list| convert_parsed_option_list(list.clone()))
                .unwrap_or_else(|| OptionListAst {
                    span,
                    raw: String::new(),
                    entries: Vec::new(),
                }),
        }),
        "PathComment" => PathItem::PathComment(PathCommentItem {
            id,
            span,
            raw: item.raw.clone(),
        }),
        "Node" => PathItem::Node(NodeItem {
            id,
            span,
            raw: item.raw.clone(),
            template_raw: item.template_raw.clone().unwrap_or_default(),
            name: item.name.clone(),
            aliases: Vec::new(),
            options_span: None,
            options: item
                .options
                .as_ref()
                .map(|list| convert_parsed_option_list(list.clone())),
            foreach_clauses: Vec::new(),
            at_span: None,
            at_raw: None,
            at_relative_prefix: None,
            text_source: GroupOptionSource::Group,
            text_span: item.text_span.unwrap_or(span),
            text: item.text.clone().unwrap_or_default(),
            structural_text: None,
            adornment: None,
        }),
        _ => PathItem::Unknown(UnknownPathItem {
            id,
            span,
            raw: item.raw.clone(),
        }),
    }
}

/// Convert a [`ParsedStatement`] into the command name and [`PathItem`] AST nodes.
///
/// This bridges the JSON-friendly [`ParsedStatement`] (used by `tikz-cli parse --json`)
/// and the rich [`PathItem`] AST consumed by the semantic evaluator.
/// Source spans are preserved exactly.
pub fn parsed_statement_to_ast(source: &str, stmt: &ParsedStatement) -> (String, Vec<PathItem>) {
    let command = stmt.command.clone().unwrap_or_default();
    let mut index = 0usize;
    let items = stmt
        .items
        .iter()
        .map(|item| parsed_path_item_to_path_item(source, item, &mut index))
        .collect();
    (command, items)
}

fn skip_trivia(src: &str, mut at: usize, to: usize) -> usize {
    while at < to {
        let byte = src.as_bytes()[at];
        if byte.is_ascii_whitespace() {
            at += 1;
        } else if byte == b'%' {
            while at < to && src.as_bytes()[at] != b'\n' {
                at += 1;
            }
        } else {
            break;
        }
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_parses_one_path_statement() {
        let doc = parse_tex("\\begin{tikzpicture}\\draw (0,0) -- (1,1);\\end{tikzpicture}");
        assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
        assert_eq!(doc.figures.len(), 1);
        let fig = &doc.figures[0];
        assert_eq!(fig.environment, "tikzpicture");
        assert_eq!(fig.statements.len(), 1);
        let stmt = &fig.statements[0];
        assert_eq!(stmt.kind, "Path");
        assert_eq!(stmt.command, Some("draw".to_string()));
        assert_eq!(stmt.items.len(), 3);
        assert_eq!(stmt.items[0].kind, "Coordinate");
        assert_eq!(stmt.items[0].raw, "(0,0)");
        assert_eq!(stmt.items[0].x, Some("0".to_string()));
        assert_eq!(stmt.items[0].y, Some("0".to_string()));
        assert_eq!(stmt.items[1].kind, "PathKeyword");
        assert_eq!(stmt.items[1].raw, "--");
        assert_eq!(stmt.items[2].kind, "Coordinate");
        assert_eq!(stmt.items[2].raw, "(1,1)");
    }

    #[test]
    fn unknown_statement_head_is_not_dropped() {
        let doc = parse_tex(
            "\\begin{tikzpicture}\\node at (0,0) {A};\\draw (0,0) -- (1,1);\\end{tikzpicture}",
        );
        assert_eq!(doc.figures[0].statements.len(), 2);
        assert_eq!(doc.figures[0].statements[0].kind, "Path");
        assert_eq!(
            doc.figures[0].statements[0].command,
            Some("node".to_string())
        );
        assert_eq!(doc.figures[0].statements[1].kind, "Path");
        assert_eq!(
            doc.figures[0].statements[1].command,
            Some("draw".to_string())
        );
    }

    #[test]
    fn no_figure_is_a_diagnostic() {
        let doc = parse_tex("hello world");
        assert_eq!(doc.figures.len(), 0);
        assert!(!doc.diagnostics.is_empty());
    }
}
