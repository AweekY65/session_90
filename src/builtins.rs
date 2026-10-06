use crate::executor::ShellState;
use std::io::Write;

pub fn is_builtin(name: &str) -> bool {
    matches!(name, "cd" | "pwd" | "export" | "unset" | "exit" | "echo")
}

/// Run a builtin. `in_pipeline` is true when the builtin runs as one stage of
/// a pipeline; in that case it operates on a cloned environment (subshell
/// semantics) and `exit` does not terminate the shell.
pub fn run_builtin(
    state: &mut ShellState,
    argv: &[String],
    out: &mut dyn Write,
    in_pipeline: bool,
) -> i32 {
    match argv[0].as_str() {
        "cd" => builtin_cd(state, argv),
        "pwd" => builtin_pwd(out),
        "export" => builtin_export(state, argv, out),
        "unset" => builtin_unset(state, argv),
        "echo" => builtin_echo(argv, out),
        "exit" => {
            let code = match argv.get(1) {
                Some(s) => match s.parse::<i32>() {
                    Ok(n) => n,
                    Err(_) => {
                        eprintln!("minishell: exit: {}: numeric argument required", s);
                        2
                    }
                },
                None => state.last_status,
            };
            if in_pipeline {
                code
            } else {
                let _ = std::io::stdout().flush();
                std::process::exit(code);
            }
        }
        _ => unreachable!(),
    }
}

fn builtin_cd(state: &mut ShellState, argv: &[String]) -> i32 {
    let target = match argv.get(1) {
        Some(p) => p.clone(),
        None => match state.env.get("HOME") {
            Some(h) => h.clone(),
            None => {
                eprintln!("minishell: cd: HOME not set");
                return 1;
            }
        },
    };
    match std::env::set_current_dir(&target) {
        Ok(()) => {
            if let Ok(cwd) = std::env::current_dir() {
                if let Some(old) = state.env.get("PWD").cloned() {
                    state.env.insert("OLDPWD".to_string(), old);
                }
                state
                    .env
                    .insert("PWD".to_string(), cwd.display().to_string());
            }
            0
        }
        Err(e) => {
            eprintln!("minishell: cd: {}: {}", target, e);
            1
        }
    }
}

fn builtin_pwd(out: &mut dyn Write) -> i32 {
    match std::env::current_dir() {
        Ok(dir) => {
            let _ = writeln!(out, "{}", dir.display());
            0
        }
        Err(e) => {
            eprintln!("minishell: pwd: {}", e);
            1
        }
    }
}

fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn builtin_export(state: &mut ShellState, argv: &[String], out: &mut dyn Write) -> i32 {
    if argv.len() == 1 {
        let mut keys: Vec<&String> = state.env.keys().collect();
        keys.sort();
        for key in keys {
            let _ = writeln!(out, "export {}={}", key, state.env[key]);
        }
        return 0;
    }
    let mut status = 0;
    for arg in &argv[1..] {
        match arg.find('=') {
            Some(eq) => {
                let (name, value) = arg.split_at(eq);
                if is_valid_name(name) {
                    state.env.insert(name.to_string(), value[1..].to_string());
                } else {
                    eprintln!("minishell: export: `{}': not a valid identifier", arg);
                    status = 1;
                }
            }
            None => {
                if is_valid_name(arg) {
                    state.env.entry(arg.clone()).or_default();
                } else {
                    eprintln!("minishell: export: `{}': not a valid identifier", arg);
                    status = 1;
                }
            }
        }
    }
    status
}

fn builtin_unset(state: &mut ShellState, argv: &[String]) -> i32 {
    for name in &argv[1..] {
        state.env.remove(name);
    }
    0
}

fn builtin_echo(argv: &[String], out: &mut dyn Write) -> i32 {
    let mut args = &argv[1..];
    let mut newline = true;
    if let Some(first) = args.first() {
        if first == "-n" {
            newline = false;
            args = &args[1..];
        }
    }
    let mut text = args.join(" ");
    if newline {
        text.push('\n');
    }
    // Ignore write errors (e.g. EPIPE when the downstream of a pipeline
    // closed its read end early).
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
    0
}
