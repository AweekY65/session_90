use crate::builtins;
use crate::parser::{Pipeline, Redirect};
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, ExitStatus, Stdio};

pub struct ShellState {
    pub env: HashMap<String, String>,
    pub last_status: i32,
}

impl ShellState {
    pub fn new() -> Self {
        ShellState {
            env: std::env::vars().collect(),
            last_status: 0,
        }
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        code
    } else if let Some(sig) = status.signal() {
        128 + sig
    } else {
        1
    }
}

/// Execute a whole pipeline. Returns the exit status of its last command.
///
/// Every spawned child is waited on before this function returns, so no
/// zombie processes are left behind. Pipe ends owned by the shell are
/// dropped as soon as they are handed to a child (or discarded), so
/// downstream processes observe EOF correctly.
pub fn execute_pipeline(state: &mut ShellState, pipeline: &Pipeline) -> i32 {
    let n = pipeline.commands.len();
    if n == 0 {
        return 0;
    }
    let in_pipeline = n > 1;

    // One pipe between each pair of adjacent commands.
    let mut readers: Vec<Option<std::io::PipeReader>> = Vec::new();
    let mut writers: Vec<Option<std::io::PipeWriter>> = Vec::new();
    for _ in 0..n - 1 {
        match std::io::pipe() {
            Ok((r, w)) => {
                readers.push(Some(r));
                writers.push(Some(w));
            }
            Err(e) => {
                eprintln!("minishell: pipe: {}", e);
                return 1;
            }
        }
    }

    let mut children: Vec<Option<Child>> = Vec::with_capacity(n);
    let mut statuses: Vec<i32> = vec![0; n];

    for (i, cmd) in pipeline.commands.iter().enumerate() {
        // Take this command's pipe ends out of the vectors right away: they
        // are either moved into the child or dropped here, so the shell
        // never keeps a stale copy open while waiting.
        let pipe_in = if i > 0 { readers[i - 1].take() } else { None };
        let pipe_out = if i + 1 < n { writers[i].take() } else { None };

        // Open redirection files before launching anything. On failure the
        // command is not executed at all.
        let mut stdin_file = None;
        let mut stdout_file = None;
        let mut redir_err: Option<(String, std::io::Error)> = None;
        for redir in &cmd.redirects {
            match redir {
                Redirect::In(path) => match std::fs::File::open(path) {
                    Ok(f) => stdin_file = Some(f),
                    Err(e) => {
                        redir_err = Some((path.clone(), e));
                        break;
                    }
                },
                Redirect::Out(path) => match std::fs::File::create(path) {
                    Ok(f) => stdout_file = Some(f),
                    Err(e) => {
                        redir_err = Some((path.clone(), e));
                        break;
                    }
                },
                Redirect::Append(path) => {
                    match std::fs::OpenOptions::new().create(true).append(true).open(path)
                    {
                        Ok(f) => stdout_file = Some(f),
                        Err(e) => {
                            redir_err = Some((path.clone(), e));
                            break;
                        }
                    }
                }
            }
        }
        if let Some((path, e)) = redir_err {
            eprintln!("minishell: {}: {}", path, e);
            statuses[i] = 1;
            children.push(None);
            continue; // pipe_in/pipe_out dropped: downstream sees EOF
        }

        if builtins::is_builtin(&cmd.argv[0]) {
            let mut out: Box<dyn Write> = if let Some(f) = stdout_file {
                Box::new(f)
            } else if let Some(p) = pipe_out {
                Box::new(p)
            } else {
                Box::new(std::io::stdout())
            };
            let status = if in_pipeline {
                // Subshell semantics: state changes are discarded.
                let mut sub = ShellState {
                    env: state.env.clone(),
                    last_status: state.last_status,
                };
                builtins::run_builtin(&mut sub, &cmd.argv, &mut out, true)
            } else {
                builtins::run_builtin(state, &cmd.argv, &mut out, false)
            };
            statuses[i] = status;
            children.push(None);
            continue;
        }

        let stdin_stdio: Stdio = match stdin_file {
            Some(f) => Stdio::from(f),
            None => match pipe_in {
                Some(p) => Stdio::from(p),
                None => Stdio::inherit(),
            },
        };
        let stdout_stdio: Stdio = match stdout_file {
            Some(f) => Stdio::from(f),
            None => match pipe_out {
                Some(p) => Stdio::from(p),
                None => Stdio::inherit(),
            },
        };

        let mut command = Command::new(&cmd.argv[0]);
        command
            .args(&cmd.argv[1..])
            .env_clear()
            .envs(&state.env)
            .stdin(stdin_stdio)
            .stdout(stdout_stdio)
            .stderr(Stdio::inherit());
        match command.spawn() {
            Ok(child) => children.push(Some(child)),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    eprintln!("minishell: {}: command not found", cmd.argv[0]);
                    statuses[i] = 127;
                } else {
                    eprintln!("minishell: {}: {}", cmd.argv[0], e);
                    statuses[i] = 126;
                }
                children.push(None);
            }
        }
        // `command` (and with it the shell-side copies of the child's stdio
        // fds) is dropped at the end of this iteration.
    }

    // Reap every child; none is left as a zombie.
    for (i, child) in children.into_iter().enumerate() {
        if let Some(mut c) = child {
            match c.wait() {
                Ok(status) => statuses[i] = exit_code(status),
                Err(e) => {
                    eprintln!("minishell: wait: {}", e);
                    statuses[i] = 1;
                }
            }
        }
    }
    statuses[n - 1]
}
