use crate::tokenizer::Token;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Redirect {
    In(String),
    Out(String),
    Append(String),
}

#[derive(Debug, Default)]
pub struct Command {
    pub argv: Vec<String>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Default)]
pub struct Pipeline {
    pub commands: Vec<Command>,
}

/// Turn the tokens of one statement into a pipeline of commands.
pub fn parse(tokens: Vec<Token>) -> Result<Pipeline, String> {
    let mut pipeline = Pipeline::default();
    let mut current = Command::default();
    let mut pending_redirect: Option<Token> = None;

    for tok in tokens {
        match tok {
            Token::Word(w) => {
                match pending_redirect.take() {
                    Some(Token::RedirectOut) => current.redirects.push(Redirect::Out(w)),
                    Some(Token::RedirectAppend) => current.redirects.push(Redirect::Append(w)),
                    Some(Token::RedirectIn) => current.redirects.push(Redirect::In(w)),
                    Some(_) => unreachable!(),
                    None => current.argv.push(w),
                }
            }
            Token::RedirectOut | Token::RedirectAppend | Token::RedirectIn => {
                if pending_redirect.is_some() {
                    return Err("syntax error: missing redirect target".to_string());
                }
                pending_redirect = Some(tok);
            }
            Token::Pipe => {
                if pending_redirect.is_some() {
                    return Err("syntax error: missing redirect target".to_string());
                }
                if current.argv.is_empty() {
                    return Err("syntax error near unexpected token `|'".to_string());
                }
                pipeline.commands.push(std::mem::take(&mut current));
            }
        }
    }
    if pending_redirect.is_some() {
        return Err("syntax error: missing redirect target".to_string());
    }
    if !current.argv.is_empty() || !current.redirects.is_empty() {
        if current.argv.is_empty() {
            return Err("syntax error: missing command".to_string());
        }
        pipeline.commands.push(current);
    }
    Ok(pipeline)
}
