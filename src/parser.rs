//! Parser: turns a token stream into a pipeline of commands.

use crate::lexer::Token;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectKind {
    Input,  // <
    Output, // >
    Append, // >>
}

#[derive(Debug, Clone)]
pub struct Redirect {
    pub kind: RedirectKind,
    pub target: String,
}

#[derive(Debug, Clone, Default)]
pub struct Command {
    pub argv: Vec<String>,
    pub redirects: Vec<Redirect>,
}

pub type Pipeline = Vec<Command>;

/// Parse one input line (one pipeline). Empty token list yields an empty pipeline.
pub fn parse(tokens: Vec<Token>) -> Result<Pipeline, String> {
    let mut pipeline: Pipeline = Vec::new();
    let mut current = Command::default();
    let mut iter = tokens.into_iter().peekable();
    let mut last_was_pipe = false;

    while let Some(tok) = iter.next() {
        match tok {
            Token::Word(w) => {
                current.argv.push(w);
                last_was_pipe = false;
            }
            Token::Pipe => {
                if current.argv.is_empty() && current.redirects.is_empty() {
                    return Err("syntax error near unexpected token `|'".to_string());
                }
                pipeline.push(std::mem::take(&mut current));
                last_was_pipe = true;
            }
            Token::RedirectOut | Token::RedirectAppend | Token::RedirectIn => {
                let kind = match tok {
                    Token::RedirectOut => RedirectKind::Output,
                    Token::RedirectAppend => RedirectKind::Append,
                    _ => RedirectKind::Input,
                };
                match iter.next() {
                    Some(Token::Word(target)) => {
                        current.redirects.push(Redirect { kind, target });
                        last_was_pipe = false;
                    }
                    _ => return Err("syntax error: redirection missing target".to_string()),
                }
            }
        }
    }
    if last_was_pipe {
        return Err("syntax error: unexpected end of input after `|'".to_string());
    }
    if !current.argv.is_empty() || !current.redirects.is_empty() {
        pipeline.push(current);
    }
    if pipeline.iter().any(|c| c.argv.is_empty() && c.redirects.is_empty()) {
        return Err("syntax error near unexpected token `|'".to_string());
    }
    Ok(pipeline)
}
