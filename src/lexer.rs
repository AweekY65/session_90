//! Tokenizer: splits an input line into tokens, handling quoting,
//! escaping and environment variable expansion.
//!
//! Rules:
//! - Whitespace separates words.
//! - '...'  : literal, no expansion, no escapes.
//! - "..."  : allows $VAR / ${VAR} / $? expansion and \\ \" \$ escapes.
//! - \c     : literal c (outside quotes).
//! - $VAR, ${VAR}, $? are expanded outside single quotes.
//! - | > >> < are operators when unquoted.
//! - # at the start of a word begins a comment to end of line.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(String),
    Pipe,
    RedirectOut,
    RedirectAppend,
    RedirectIn,
}

pub fn tokenize(
    input: &str,
    env: &HashMap<String, String>,
    last_status: i32,
) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut has_current = false; // true once a word started (even if empty, e.g. '')
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' | '\n' => {
                if has_current {
                    tokens.push(Token::Word(std::mem::take(&mut current)));
                    has_current = false;
                }
                i += 1;
            }
            '#' if !has_current => break, // comment until end of line
            '|' | '>' | '<' => {
                if has_current {
                    tokens.push(Token::Word(std::mem::take(&mut current)));
                    has_current = false;
                }
                match c {
                    '|' => tokens.push(Token::Pipe),
                    '<' => tokens.push(Token::RedirectIn),
                    '>' => {
                        if i + 1 < chars.len() && chars[i + 1] == '>' {
                            tokens.push(Token::RedirectAppend);
                            i += 1;
                        } else {
                            tokens.push(Token::RedirectOut);
                        }
                    }
                    _ => unreachable!(),
                }
                i += 1;
            }
            '\'' => {
                has_current = true;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    current.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err("unterminated single quote".to_string());
                }
                i += 1;
            }
            '"' => {
                has_current = true;
                i += 1;
                loop {
                    if i >= chars.len() {
                        return Err("unterminated double quote".to_string());
                    }
                    match chars[i] {
                        '"' => {
                            i += 1;
                            break;
                        }
                        '\\' if i + 1 < chars.len()
                            && matches!(chars[i + 1], '"' | '\\' | '$') =>
                        {
                            current.push(chars[i + 1]);
                            i += 2;
                        }
                        '$' => {
                            i = expand_var(&chars, i, &mut current, env, last_status)?;
                        }
                        ch => {
                            current.push(ch);
                            i += 1;
                        }
                    }
                }
            }
            '\\' => {
                has_current = true;
                if i + 1 < chars.len() {
                    current.push(chars[i + 1]);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '$' => {
                has_current = true;
                i = expand_var(&chars, i, &mut current, env, last_status)?;
            }
            _ => {
                has_current = true;
                current.push(c);
                i += 1;
            }
        }
    }
    if has_current {
        tokens.push(Token::Word(current));
    }
    Ok(tokens)
}

/// Expand a variable starting at chars[i] == '$'. Returns the next index.
fn expand_var(
    chars: &[char],
    i: usize,
    out: &mut String,
    env: &HashMap<String, String>,
    last_status: i32,
) -> Result<usize, String> {
    let mut j = i + 1;
    if j < chars.len() && chars[j] == '?' {
        out.push_str(&last_status.to_string());
        return Ok(j + 1);
    }
    if j < chars.len() && chars[j] == '{' {
        j += 1;
        let start = j;
        while j < chars.len() && chars[j] != '}' {
            j += 1;
        }
        if j >= chars.len() {
            return Err("unterminated ${...}".to_string());
        }
        let name: String = chars[start..j].iter().collect();
        if name.is_empty() {
            return Err("empty variable name in ${}".to_string());
        }
        out.push_str(env.get(&name).map(String::as_str).unwrap_or(""));
        return Ok(j + 1);
    }
    let start = j;
    while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
        j += 1;
    }
    if j == start {
        out.push('$'); // lone '$' stays literal
        return Ok(j);
    }
    let name: String = chars[start..j].iter().collect();
    out.push_str(env.get(&name).map(String::as_str).unwrap_or(""));
    Ok(j)
}
