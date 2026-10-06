//! minishell: a small local Unix shell.
//!
//! Usage:
//!   minishell                interactive REPL
//!   minishell -c 'cmd ...'   execute one command line, exit with its status
//!   minishell script.ms      execute a script file line by line

mod lexer;
mod parser;
mod shell;

use shell::Shell;
use std::io::{IsTerminal, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut sh = Shell::new();

    if args.len() >= 2 && args[1] == "-c" {
        if args.len() < 3 {
            eprintln!("minishell: -c requires an argument");
            std::process::exit(2);
        }
        let status = sh.run_line(&args[2]);
        std::process::exit(status);
    }

    if args.len() >= 2 {
        match std::fs::read_to_string(&args[1]) {
            Ok(content) => {
                sh.run_script(&content);
                std::process::exit(if sh.running { sh.last_status } else { sh.exit_code });
            }
            Err(e) => {
                eprintln!("minishell: {}: {e}", args[1]);
                std::process::exit(1);
            }
        }
    }

    // Interactive REPL (also works with piped stdin).
    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal();
    loop {
        if interactive {
            print!("minishell> ");
            let _ = std::io::stdout().flush();
        }
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                sh.run_line(&line);
                if !sh.running {
                    std::process::exit(sh.exit_code);
                }
            }
            Err(e) => {
                eprintln!("minishell: read error: {e}");
                break;
            }
        }
    }
    std::process::exit(if sh.running { sh.last_status } else { sh.exit_code });
}
