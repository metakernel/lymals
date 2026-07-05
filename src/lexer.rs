use crate::syntax::{SourceSpan, SourceText, Token, TokenKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexResult {
    pub tokens: Vec<Token>,
}

#[must_use]
pub fn lex(source: &SourceText) -> LexResult {
    let mut tokens = Vec::new();
    let text = source.as_str();
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            ' ' | '\t' => i += 1,
            '\n' => {
                tokens.push(token(source, TokenKind::LineBreak, i, i + 1));
                i += 1;
            }
            '#' => {
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                tokens.push(token(source, TokenKind::Comment, start, i));
            }
            ':' => {
                tokens.push(token(source, TokenKind::Colon, i, i + 1));
                i += 1;
            }
            '-' => {
                if text[i..].starts_with("---") && boundary(text, i + 3) {
                    tokens.push(token(source, TokenKind::DocumentSeparator, i, i + 3));
                    i += 3;
                } else {
                    tokens.push(token(source, TokenKind::Dash, i, i + 1));
                    i += 1;
                }
            }
            '.' => {
                if text[i..].starts_with("...") {
                    let kind = if boundary(text, i + 3) {
                        TokenKind::DocumentTerminator
                    } else {
                        TokenKind::Spread
                    };
                    tokens.push(token(source, kind, i, i + 3));
                    i += 3;
                } else {
                    let start = i;
                    i += 1;
                    tokens.push(token(source, TokenKind::Unknown, start, i));
                }
            }
            '=' => {
                tokens.push(token(source, TokenKind::Equals, i, i + 1));
                i += 1;
            }
            '[' => {
                tokens.push(token(source, TokenKind::LeftBracket, i, i + 1));
                i += 1;
            }
            ']' => {
                tokens.push(token(source, TokenKind::RightBracket, i, i + 1));
                i += 1;
            }
            '{' => {
                tokens.push(token(source, TokenKind::LeftBrace, i, i + 1));
                i += 1;
            }
            '}' => {
                tokens.push(token(source, TokenKind::RightBrace, i, i + 1));
                i += 1;
            }
            '|' | '>' => {
                tokens.push(token(source, TokenKind::BlockHeader, i, i + 1));
                i += 1;
            }
            '"' | '\'' => {
                let quote = ch as u8;
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    if bytes[i - 1] == quote {
                        break;
                    }
                }
                tokens.push(token(source, TokenKind::String, start, i.min(bytes.len())));
            }
            '@' => {
                let start = i;
                i += 1;
                while i < bytes.len() && is_word(bytes[i] as char) {
                    i += 1;
                }
                tokens.push(token(source, TokenKind::DirectiveName, start, i));
            }
            '!' => {
                let start = i;
                i += 1;
                while i < bytes.len() && is_word(bytes[i] as char) {
                    i += 1;
                }
                tokens.push(token(source, TokenKind::TagName, start, i));
            }
            '$' if text[i..].starts_with("${") => {
                let start = i;
                i += 2;
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                i = (i + 1).min(bytes.len());
                tokens.push(token(source, TokenKind::String, start, i));
            }
            _ if ch.is_ascii_digit() => {
                let start = i;
                i += 1;
                while i < bytes.len() && ((bytes[i] as char).is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                tokens.push(token(source, TokenKind::Number, start, i));
            }
            _ => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i] as char;
                    if c.is_ascii_whitespace() || matches!(c, ':' | '#' | '[' | ']' | '{' | '}') {
                        break;
                    }
                    if text[i..].starts_with("...") || text[i..].starts_with("---") {
                        break;
                    }
                    i += 1;
                }
                let lexeme = &text[start..i];
                let kind = match lexeme {
                    "let" => TokenKind::KeywordLet,
                    "as" => TokenKind::KeywordAs,
                    "in" => TokenKind::KeywordIn,
                    _ if lexeme.starts_with('=') => TokenKind::PlainString,
                    _ => TokenKind::Identifier,
                };
                tokens.push(token(source, kind, start, i));
            }
        }
    }

    tokens.push(Token {
        kind: TokenKind::EndOfFile,
        lexeme: String::new(),
        span: SourceSpan::new(source.file_id, text.len(), text.len()),
    });
    LexResult { tokens }
}

fn token(source: &SourceText, kind: TokenKind, start: usize, end: usize) -> Token {
    Token {
        kind,
        lexeme: source.as_str()[start..end].to_owned(),
        span: SourceSpan::new(source.file_id, start, end),
    }
}

fn is_word(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '?')
}

fn boundary(text: &str, idx: usize) -> bool {
    text.get(idx..=idx)
        .and_then(|s| s.chars().next())
        .is_none_or(char::is_whitespace)
}
