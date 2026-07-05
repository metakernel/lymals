use serde::Serialize;
use tower_lsp::lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensLegend,
};

use crate::{position::LineIndex, syntax::FileId};

pub const TOKEN_TYPES: &[&str] = &[
    "keyword",
    "property",
    "string",
    "number",
    "comment",
    "operator",
    "decorator",
    "variable",
    "macro",
    "boolean",
    "null",
    "lua",
];

pub const TOKEN_MODIFIERS: &[&str] = &[
    "declaration",
    "block",
    "import",
    "include",
    "deprecated",
    "invalid",
];

const KEYWORD: u32 = 0;
const PROPERTY: u32 = 1;
const STRING: u32 = 2;
const NUMBER: u32 = 3;
const COMMENT: u32 = 4;
const OPERATOR: u32 = 5;
const DECORATOR: u32 = 6;
const VARIABLE: u32 = 7;
const MACRO: u32 = 8;
const BOOLEAN: u32 = 9;
const NULL: u32 = 10;
const LUA: u32 = 11;

const DECLARATION: u32 = 1 << 0;
const BLOCK: u32 = 1 << 1;
const IMPORT: u32 = 1 << 2;
const INCLUDE: u32 = 1 << 3;
const DEPRECATED: u32 = 1 << 4;
const INVALID: u32 = 1 << 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EncodedTokenDebug {
    pub delta_line: u32,
    pub delta_start: u32,
    pub length: u32,
    pub token_type: &'static str,
    pub modifiers: Vec<&'static str>,
    pub lexeme: String,
}

#[must_use]
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES
            .iter()
            .copied()
            .map(token_type_from_name)
            .collect(),
        token_modifiers: TOKEN_MODIFIERS
            .iter()
            .copied()
            .map(token_modifier_from_name)
            .collect(),
    }
}

#[must_use]
pub fn tokenize(file_id: FileId, name: &str, text: &str) -> SemanticTokens {
    let line_index = LineIndex::new(text);
    let mut collector = Collector::new(file_id, name, text);
    collector.collect();
    collector.encode(&line_index)
}

#[must_use]
pub fn debug_render(text: &str, tokens: &SemanticTokens) -> Vec<EncodedTokenDebug> {
    let line_index = LineIndex::new(text);
    let mut rendered = Vec::new();
    let mut line = 0u32;
    let mut start = 0u32;

    for token in &tokens.data {
        line += token.delta_line;
        if token.delta_line == 0 {
            start += token.delta_start;
        } else {
            start = token.delta_start;
        }

        let start_offset = offset_for_position(&line_index, text, line, start);
        let end_offset = nth_utf16_offset(text, start_offset, token.length);
        rendered.push(EncodedTokenDebug {
            delta_line: token.delta_line,
            delta_start: token.delta_start,
            length: token.length,
            token_type: TOKEN_TYPES[token.token_type as usize],
            modifiers: modifier_names(token.token_modifiers_bitset),
            lexeme: text
                .get(start_offset..end_offset)
                .unwrap_or_default()
                .to_string(),
        });
    }

    rendered
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct RawToken {
    start: usize,
    end: usize,
    token_type: u32,
    modifiers: u32,
}

struct Collector<'a> {
    file_id: FileId,
    name: &'a str,
    text: &'a str,
    tokens: Vec<RawToken>,
}

impl<'a> Collector<'a> {
    fn new(file_id: FileId, name: &'a str, text: &'a str) -> Self {
        Self {
            file_id,
            name,
            text,
            tokens: Vec::new(),
        }
    }

    fn collect(&mut self) {
        let lines = lines_with_offsets(self.text);
        let mut block_indent: Option<usize> = None;

        for line in &lines {
            let trimmed = line.trimmed(self.text);

            if let Some(expected_indent) = block_indent {
                if trimmed.is_empty() || line.indent >= expected_indent {
                    let start = (line.start + expected_indent).min(line.end);
                    let block_trimmed_start = skip_spaces(self.text, start, line.end);
                    if self.text[block_trimmed_start..line.end].starts_with("${")
                        || self.text[block_trimmed_start..line.end].starts_with("lua{")
                    {
                        self.tokenize_inline(block_trimmed_start, line.end, BLOCK);
                    } else {
                        self.push(start, line.end, STRING, BLOCK);
                    }
                    continue;
                }
                block_indent = None;
            }

            if trimmed.is_empty() {
                continue;
            }

            if trimmed == "---" || trimmed == "..." {
                self.push(line.content_start, line.end, OPERATOR, 0);
                continue;
            }

            if trimmed.starts_with('#') {
                self.push(line.content_start, line.end, COMMENT, 0);
                continue;
            }

            let mut cursor = line.content_start;

            if self.text[cursor..line.end].starts_with('-')
                && self.text[cursor + 1..line.end]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
            {
                self.push(cursor, cursor + 1, OPERATOR, 0);
                cursor = skip_spaces(self.text, cursor + 1, line.end);
            }

            if cursor >= line.end {
                continue;
            }

            let rest = &self.text[cursor..line.end];

            if rest.starts_with('@') {
                self.tokenize_directive(line, cursor);
                continue;
            }

            if rest.starts_with("let ") {
                self.tokenize_let(line, cursor);
                continue;
            }

            if rest.starts_with("...") {
                self.push(cursor, cursor + 3, OPERATOR, 0);
                self.tokenize_inline(skip_spaces(self.text, cursor + 3, line.end), line.end, 0);
                continue;
            }

            if rest.starts_with("?if") || rest.starts_with("?elif") || rest.starts_with("?else") {
                let end = word_end(self.text, cursor + 1, line.end);
                self.push(cursor, end, KEYWORD, 0);
                self.tokenize_inline(skip_spaces(self.text, end, line.end), line.end, 0);
                continue;
            }

            if rest.starts_with("*for") {
                let end = word_end(self.text, cursor + 1, line.end);
                self.push(cursor, end, KEYWORD, 0);
                self.tokenize_inline(skip_spaces(self.text, end, line.end), line.end, 0);
                continue;
            }

            if let Some((key_start, key_end, colon)) =
                find_mapping_colon(self.text, cursor, line.end)
            {
                self.push(key_start, key_end, PROPERTY, 0);
                self.push(colon, colon + 1, OPERATOR, 0);
                let value_start = skip_spaces(self.text, colon + 1, line.end);
                if value_start < line.end {
                    let header = self.text.as_bytes()[value_start] as char;
                    if matches!(header, '|' | '>') {
                        self.push(value_start, value_start + 1, OPERATOR, 0);
                        block_indent = Some(line.indent + 2);
                        self.tokenize_inline(value_start + 1, line.end, 0);
                    } else {
                        self.tokenize_inline(value_start, line.end, 0);
                    }
                }
                continue;
            }

            if rest.starts_with('!') {
                let end = tag_end(self.text, cursor + 1, line.end);
                self.push(cursor, end, MACRO, 0);
                self.tokenize_inline(skip_spaces(self.text, end, line.end), line.end, 0);
                continue;
            }

            self.tokenize_inline(cursor, line.end, 0);
        }

        self.tokens.sort_unstable();
        self.tokens.dedup();
    }

    fn tokenize_directive(&mut self, line: &Line, start: usize) {
        let name_end = tag_end(self.text, start + 1, line.end);
        let name = &self.text[start..name_end];
        let deprecated = u32::from(name == "@use") * DEPRECATED;
        self.push(start, name_end, DECORATOR, deprecated);

        let mut cursor = skip_spaces(self.text, name_end, line.end);
        if cursor >= line.end {
            return;
        }

        if matches!(name, "@import" | "@include" | "@use") {
            let target_end = self.scan_value_end(cursor, line.end);
            let target_mod = deprecated | if name == "@include" { INCLUDE } else { IMPORT };
            if target_end > cursor {
                self.push(cursor, target_end, STRING, target_mod);
            }
            cursor = skip_spaces(self.text, target_end, line.end);
            if self.text[cursor..line.end].starts_with("as")
                && self.text[cursor + 2..line.end]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
            {
                self.push(cursor, cursor + 2, KEYWORD, 0);
                let alias_start = skip_spaces(self.text, cursor + 2, line.end);
                let alias_end = word_end(self.text, alias_start, line.end);
                self.push(alias_start, alias_end, VARIABLE, DECLARATION | deprecated);
                self.tokenize_inline(skip_spaces(self.text, alias_end, line.end), line.end, 0);
            } else {
                self.tokenize_inline(cursor, line.end, 0);
            }
            return;
        }

        self.tokenize_inline(cursor, line.end, 0);
    }

    fn tokenize_let(&mut self, line: &Line, start: usize) {
        self.push(start, start + 3, KEYWORD, 0);
        let name_start = skip_spaces(self.text, start + 3, line.end);
        let equals = self.text[name_start..line.end]
            .find('=')
            .map(|idx| name_start + idx);
        match equals {
            Some(eq) => {
                let name_end = trim_end_ascii_whitespace(self.text, name_start, eq);
                self.push(name_start, name_end, VARIABLE, DECLARATION);
                self.push(eq, eq + 1, OPERATOR, 0);
                self.tokenize_inline(skip_spaces(self.text, eq + 1, line.end), line.end, 0);
            }
            None => {
                self.push(name_start, line.end, VARIABLE, DECLARATION | INVALID);
            }
        }
    }

    fn tokenize_inline(&mut self, mut start: usize, end: usize, inherited_modifiers: u32) {
        start = skip_spaces(self.text, start, end);
        while start < end {
            let ch = self.text.as_bytes()[start] as char;
            match ch {
                ' ' | '\t' => start += 1,
                '#' => {
                    self.push(start, end, COMMENT, 0);
                    break;
                }
                '"' | '\'' => {
                    let string_end = scan_quoted(self.text, start, end);
                    let invalid = if string_end <= end
                        && self.text.as_bytes()[string_end - 1] as char == ch
                    {
                        0
                    } else {
                        INVALID
                    };
                    self.push(start, string_end, STRING, inherited_modifiers | invalid);
                    start = string_end;
                }
                '$' if self.text[start..end].starts_with("${") => {
                    let token_end = scan_braced(self.text, start + 1, end);
                    let invalid = if token_end <= end && self.text.as_bytes()[token_end - 1] == b'}'
                    {
                        0
                    } else {
                        INVALID
                    };
                    self.push(start, token_end, LUA, inherited_modifiers | invalid);
                    start = token_end;
                }
                '=' => {
                    self.push(start, end, LUA, inherited_modifiers);
                    break;
                }
                'l' if self.text[start..end].starts_with("lua{") => {
                    let token_end = scan_braced(self.text, start + 3, end);
                    let invalid = if token_end <= end && self.text.as_bytes()[token_end - 1] == b'}'
                    {
                        0
                    } else {
                        INVALID
                    };
                    self.push(start, token_end, LUA, inherited_modifiers | invalid);
                    start = token_end;
                }
                '!' => {
                    let token_end = tag_end(self.text, start + 1, end);
                    self.push(start, token_end, MACRO, inherited_modifiers);
                    start = token_end;
                }
                '@' => {
                    let token_end = tag_end(self.text, start + 1, end);
                    let invalid = u32::from(token_end == start + 1) * INVALID;
                    self.push(start, token_end, DECORATOR, inherited_modifiers | invalid);
                    start = token_end;
                }
                ':' | '[' | ']' | '{' | '}' | ',' => {
                    self.push(start, start + 1, OPERATOR, inherited_modifiers);
                    start += 1;
                }
                '.' if self.text[start..end].starts_with("...") => {
                    self.push(start, start + 3, OPERATOR, inherited_modifiers);
                    start += 3;
                }
                '.' => {
                    self.push(start, start + 1, OPERATOR, inherited_modifiers | INVALID);
                    start += 1;
                }
                '+' | '*' | '/' | '%' => {
                    self.push(start, start + 1, OPERATOR, inherited_modifiers);
                    start += 1;
                }
                '-' => {
                    if start + 1 < end && (self.text.as_bytes()[start + 1] as char).is_ascii_digit()
                    {
                        let token_end = scan_number(self.text, start, end);
                        self.push(start, token_end, NUMBER, inherited_modifiers);
                        start = token_end;
                    } else {
                        self.push(start, start + 1, OPERATOR, inherited_modifiers);
                        start += 1;
                    }
                }
                c if c.is_ascii_digit() => {
                    let token_end = scan_number(self.text, start, end);
                    self.push(start, token_end, NUMBER, inherited_modifiers);
                    start = token_end;
                }
                _ => {
                    let token_end = word_like_end(self.text, start, end);
                    if token_end == start {
                        self.push(start, start + 1, OPERATOR, inherited_modifiers | INVALID);
                        start += 1;
                        continue;
                    }

                    let value = &self.text[start..token_end];
                    let (token_type, extra_modifiers) = match value {
                        "true" | "false" => (BOOLEAN, 0),
                        "null" | "nil" => (NULL, 0),
                        "as" | "in" | "let" | "for" | "if" | "elif" | "else" => (KEYWORD, 0),
                        _ => (STRING, 0),
                    };
                    self.push(
                        start,
                        token_end,
                        token_type,
                        inherited_modifiers | extra_modifiers,
                    );
                    start = token_end;
                }
            }

            start = skip_spaces(self.text, start, end);
        }
    }

    fn scan_value_end(&self, start: usize, end: usize) -> usize {
        if start >= end {
            return start;
        }
        match self.text.as_bytes()[start] as char {
            '"' | '\'' => scan_quoted(self.text, start, end),
            _ => {
                let mut cursor = start;
                while cursor < end {
                    let ch = self.text.as_bytes()[cursor] as char;
                    if ch.is_ascii_whitespace() || ch == '#' {
                        break;
                    }
                    cursor += 1;
                }
                cursor
            }
        }
    }

    fn push(&mut self, start: usize, end: usize, token_type: u32, modifiers: u32) {
        if start >= end || end > self.text.len() {
            return;
        }
        let _ = self.file_id;
        let _ = self.name;
        self.tokens.push(RawToken {
            start,
            end,
            token_type,
            modifiers,
        });
    }

    fn encode(self, line_index: &LineIndex) -> SemanticTokens {
        let mut data = Vec::with_capacity(self.tokens.len());
        let mut previous_line = 0u32;
        let mut previous_start = 0u32;
        let mut first = true;

        for token in self.tokens {
            let start = line_index
                .offset_to_position(self.text, token.start)
                .expect("semantic token start offset is valid");
            let end = line_index
                .offset_to_position(self.text, token.end)
                .expect("semantic token end offset is valid");
            let delta_line = if first {
                start.line
            } else {
                start.line - previous_line
            };
            let delta_start = if first || delta_line > 0 {
                start.character
            } else {
                start.character - previous_start
            };
            let length = utf16_len(&self.text[token.start..token.end]);

            data.push(SemanticToken {
                delta_line,
                delta_start,
                length,
                token_type: token.token_type,
                token_modifiers_bitset: token.modifiers,
            });

            previous_line = start.line;
            previous_start = start.character;
            let _ = end;
            first = false;
        }

        SemanticTokens {
            result_id: None,
            data,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Line {
    start: usize,
    end: usize,
    indent: usize,
    content_start: usize,
}

impl Line {
    fn trimmed<'a>(&self, text: &'a str) -> &'a str {
        text[self.content_start..self.end].trim_end()
    }
}

fn lines_with_offsets(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    for segment in text.split_inclusive('\n') {
        let body = segment.strip_suffix('\n').unwrap_or(segment);
        let end = start + body.len();
        let indent = body.chars().take_while(|ch| *ch == ' ').count();
        lines.push(Line {
            start,
            end,
            indent,
            content_start: (start + indent).min(end),
        });
        start += segment.len();
    }
    if text.is_empty()
        || (!text.ends_with('\n') && lines.last().is_none_or(|line| line.end != text.len()))
    {
        let body = &text[start..];
        let indent = body.chars().take_while(|ch| *ch == ' ').count();
        lines.push(Line {
            start,
            end: text.len(),
            indent,
            content_start: start + indent,
        });
    }
    lines
}

fn find_mapping_colon(text: &str, start: usize, end: usize) -> Option<(usize, usize, usize)> {
    let mut cursor = start;
    let mut quote = None;
    while cursor < end {
        let ch = text.as_bytes()[cursor] as char;
        match (quote, ch) {
            (Some(active), c) if c == active => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, '#') => break,
            (None, ':') => {
                let key_end = trim_end_ascii_whitespace(text, start, cursor);
                if key_end > start {
                    return Some((start, key_end, cursor));
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn scan_quoted(text: &str, start: usize, end: usize) -> usize {
    let quote = text.as_bytes()[start];
    let mut cursor = start + 1;
    while cursor < end {
        match text.as_bytes()[cursor] {
            b'\\' => cursor = (cursor + 2).min(end),
            byte if byte == quote => return cursor + 1,
            _ => cursor += 1,
        }
    }
    end
}

fn scan_braced(text: &str, brace_start: usize, end: usize) -> usize {
    let mut cursor = brace_start;
    let mut depth = 0usize;
    while cursor < end {
        match text.as_bytes()[cursor] {
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return cursor + 1;
                }
                depth -= 1;
                if depth == 0 {
                    return cursor + 1;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    end
}

fn scan_number(text: &str, start: usize, end: usize) -> usize {
    let mut cursor = start;
    if text.as_bytes()[cursor] as char == '-' {
        cursor += 1;
    }
    while cursor < end {
        let ch = text.as_bytes()[cursor] as char;
        if ch.is_ascii_digit() || matches!(ch, '.' | '_') {
            cursor += 1;
        } else {
            break;
        }
    }
    cursor
}

fn skip_spaces(text: &str, mut start: usize, end: usize) -> usize {
    while start < end && matches!(text.as_bytes()[start] as char, ' ' | '\t') {
        start += 1;
    }
    start
}

fn trim_end_ascii_whitespace(text: &str, start: usize, mut end: usize) -> usize {
    while end > start && (text.as_bytes()[end - 1] as char).is_ascii_whitespace() {
        end -= 1;
    }
    end
}

fn word_end(text: &str, mut start: usize, end: usize) -> usize {
    while start < end {
        let ch = text.as_bytes()[start] as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '?') {
            start += 1;
        } else {
            break;
        }
    }
    start
}

fn tag_end(text: &str, start: usize, end: usize) -> usize {
    word_end(text, start, end)
}

fn word_like_end(text: &str, mut start: usize, end: usize) -> usize {
    while start < end {
        let ch = text.as_bytes()[start] as char;
        if ch.is_ascii_whitespace()
            || matches!(
                ch,
                '#' | ':' | '[' | ']' | '{' | '}' | ',' | '+' | '*' | '/' | '%' | '='
            )
        {
            break;
        }
        start += 1;
    }
    start
}

fn utf16_len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

fn offset_for_position(line_index: &LineIndex, text: &str, line: u32, character: u32) -> usize {
    line_index
        .position_to_offset(text, tower_lsp::lsp_types::Position::new(line, character))
        .expect("semantic token position is valid")
}

fn nth_utf16_offset(text: &str, start: usize, length: u32) -> usize {
    let mut utf16 = 0u32;
    let mut offset = start;
    for ch in text[start..].chars() {
        if utf16 >= length {
            break;
        }
        utf16 += ch.len_utf16() as u32;
        offset += ch.len_utf8();
    }
    offset
}

fn modifier_names(bitset: u32) -> Vec<&'static str> {
    TOKEN_MODIFIERS
        .iter()
        .enumerate()
        .filter_map(|(index, name)| ((bitset & (1 << index)) != 0).then_some(*name))
        .collect()
}

fn token_type_from_name(name: &'static str) -> SemanticTokenType {
    match name {
        "keyword" => SemanticTokenType::KEYWORD,
        "property" => SemanticTokenType::PROPERTY,
        "string" => SemanticTokenType::STRING,
        "number" => SemanticTokenType::NUMBER,
        "comment" => SemanticTokenType::COMMENT,
        "operator" => SemanticTokenType::OPERATOR,
        "decorator" => SemanticTokenType::DECORATOR,
        "variable" => SemanticTokenType::VARIABLE,
        "macro" => SemanticTokenType::MACRO,
        custom => SemanticTokenType::new(custom),
    }
}

fn token_modifier_from_name(name: &'static str) -> SemanticTokenModifier {
    match name {
        "declaration" => SemanticTokenModifier::DECLARATION,
        custom => SemanticTokenModifier::new(custom),
    }
}
