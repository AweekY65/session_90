mod builtins;
mod executor;
mod parser;
mod tokenizer;

use executor::ShellState;
use std::io::{BufRead, IsTerminal, Write};

fn run_line(state: &mut ShellState, line: &str) -> i32 {
    let mut rest = line;
    let mut status: i32;
    loop {
        let (tokens, consumed) = match tokenizer::tokenize(rest, &state.env, state.last_status) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("minishell: {}", e);
                state.last_status = 2;
                return 2;
            }
        };
        rest = &rest[consumed..];
        let pipeline = match parser::parse(tokens) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("minishell: {}", e);
                state.last_status = 2;
                return 2;
            }
        };
        status = executor::execute_pipeline(state, &pipeline);
        state.last_status = status;
        if rest.trim().is_empty() {
            break;
        }
    }
    status
}

fn run_script(state: &mut ShellState, script: &str) -> i32 {
    let mut status = 0;
    for line in script.lines() {
        status = run_line(state, line);
    }
    status
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut state = ShellState::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-c" => {
                if i + 1 >= args.len() {
                    eprintln!("minishell: -c: option requires an argument");
                    std::process::exit(2);
                }
                let status = run_script(&mut state, &args[i + 1]);
                std::process::exit(status);
            }
            "--" => {
                i += 1;
                break;
            }
            _ => break,
        }
    }

    if i < args.len() {
        match std::fs::read_to_string(&args[i]) {
            Ok(content) => {
                let status = run_script(&mut state, &content);
                std::process::exit(status);
            }
            Err(e) => {
                eprintln!("minishell: {}: {}", args[i], e);
                std::process::exit(1);
            }
        }
    }

    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal();
    loop {
        if interactive {
            print!("minishell$ ");
            let _ = std::io::stdout().flush();
        }
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF (Ctrl-D)
            Ok(_) => {
                run_line(&mut state, &line);
            }
            Err(e) => {
                eprintln!("minishell: read: {}", e);
                break;
            }
        }
    }
}
