use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(String),
    Pipe,
    RedirectOut,
    RedirectAppend,
    RedirectIn,
}

/// Tokenize a single statement (up to the next unquoted `;` or the end of
/// the input), performing quote removal, backslash escapes and environment
/// variable expansion in a single pass.
///
/// Returns the tokens and the byte offset at which the next statement
/// starts. Tokenizing statement by statement (instead of a whole line at
/// once) ensures `$?` and variables set by earlier commands on the same
/// line expand to their current values.
pub fn tokenize(
    input: &str,
    env: &HashMap<String, String>,
    last_status: i32,
) -> Result<(Vec<Token>, usize), String> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut word_started = false;
    let mut chars = input.char_indices().peekable();

    macro_rules! finish_word {
        () => {
            if word_started {
                tokens.push(Token::Word(std::mem::take(&mut word)));
                word_started = false;
            }
        };
    }

    macro_rules! rest_offset {
        () => {
            chars.peek().map(|(i, _)| *i).unwrap_or(input.len())
        };
    }

    while let Some((_, c)) = chars.next() {
        match c {
            '\'' => {
                word_started = true;
                loop {
                    match chars.next() {
                        Some((_, '\'')) => break,
                        Some((_, ch)) => word.push(ch),
                        None => return Err("unterminated single quote".to_string()),
                    }
                }
            }
            '"' => {
                word_started = true;
                loop {
                    match chars.next() {
                        Some((_, '"')) => break,
                        Some((_, '\\')) => match chars.next() {
                            Some((_, esc @ ('$' | '"' | '\\'))) => word.push(esc),
                            Some((_, '\n')) => {}
                            Some((_, other)) => {
                                word.push('\\');
                                word.push(other);
                            }
                            None => return Err("unterminated double quote".to_string()),
                        },
                        Some((_, '$')) => expand_var(&mut chars, &mut word, env, last_status),
                        Some((_, ch)) => word.push(ch),
                        None => return Err("unterminated double quote".to_string()),
                    }
                }
            }
            '\\' => {
                word_started = true;
                match chars.next() {
                    Some((_, '\n')) => {}
                    Some((_, ch)) => word.push(ch),
                    None => word.push('\\'),
                }
            }
            '$' => {
                word_started = true;
                expand_var(&mut chars, &mut word, env, last_status);
            }
            c if c.is_whitespace() => finish_word!(),
            '|' => {
                finish_word!();
                tokens.push(Token::Pipe);
            }
            ';' => {
                if word_started {
                    tokens.push(Token::Word(std::mem::take(&mut word)));
                }
                return Ok((tokens, rest_offset!()));
            }
            '>' => {
                finish_word!();
                if matches!(chars.peek(), Some((_, '>'))) {
                    chars.next();
                    tokens.push(Token::RedirectAppend);
                } else {
                    tokens.push(Token::RedirectOut);
                }
            }
            '<' => {
                finish_word!();
                tokens.push(Token::RedirectIn);
            }
            '#' if !word_started => {
                // comment: ignore the rest of the line
                return Ok((tokens, input.len()));
            }
            ch => {
                word_started = true;
                word.push(ch);
            }
        }
    }
    if word_started {
        tokens.push(Token::Word(word));
    }
    Ok((tokens, input.len()))
}

fn expand_var(
    chars: &mut std::iter::Peekable<std::str::CharIndices>,
    out: &mut String,
    env: &HashMap<String, String>,
    last_status: i32,
) {
    match chars.peek().copied() {
        Some((_, '?')) => {
            chars.next();
            out.push_str(&last_status.to_string());
        }
        Some((_, '$')) => {
            chars.next();
            out.push_str(&std::process::id().to_string());
        }
        Some((_, '{')) => {
            chars.next();
            let mut name = String::new();
            while let Some(&(_, ch)) = chars.peek() {
                chars.next();
                if ch == '}' {
                    break;
                }
                name.push(ch);
            }
            if let Some(val) = env.get(&name) {
                out.push_str(val);
            }
        }
        Some((_, ch)) if ch.is_ascii_alphabetic() || ch == '_' => {
            let mut name = String::new();
            while let Some(&(_, ch)) = chars.peek() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    name.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }
            if let Some(val) = env.get(&name) {
                out.push_str(val);
            }
        }
        _ => out.push('$'),
    }
}
