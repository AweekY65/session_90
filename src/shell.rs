//! Shell state, builtin commands and the pipeline executor.

use crate::lexer;
use crate::parser::{self, Command, Pipeline, RedirectKind};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::process::{Child, ChildStdout, Command as ProcCommand, Stdio};

pub struct Shell {
    pub env: HashMap<String, String>,
    pub last_status: i32,
    /// Exit statuses of every stage of the most recent pipeline.
    pub last_pipeline_statuses: Vec<i32>,
    pub history: Vec<String>,
    pub running: bool,
    pub exit_code: i32,
}

/// Input handed from one pipeline stage to the next.
enum PrevInput {
    None,
    Pipe(ChildStdout),
    Buffer(Vec<u8>),
}

const BUILTINS: &[&str] = &["cd", "pwd", "export", "unset", "exit", "echo", "env", "history"];

fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

impl Shell {
    pub fn new() -> Self {
        Shell {
            env: std::env::vars().collect(),
            last_status: 0,
            last_pipeline_statuses: Vec::new(),
            history: Vec::new(),
            running: true,
            exit_code: 0,
        }
    }

    /// Execute one input line. Returns the line's exit status.
    pub fn run_line(&mut self, line: &str) -> i32 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return self.last_status;
        }
        self.history.push(trimmed.to_string());

        let tokens = match lexer::tokenize(trimmed, &self.env, self.last_status) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("minishell: {e}");
                self.last_status = 2;
                return 2;
            }
        };
        let pipeline = match parser::parse(tokens) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("minishell: {e}");
                self.last_status = 2;
                return 2;
            }
        };
        if pipeline.is_empty() {
            return self.last_status;
        }
        let status = self.execute(&pipeline);
        self.last_status = status;
        status
    }

    /// Execute a multi-line script; stops early on `exit`.
    pub fn run_script(&mut self, content: &str) {
        for line in content.lines() {
            if !self.running {
                break;
            }
            self.run_line(line);
        }
    }

    fn execute(&mut self, pipeline: &Pipeline) -> i32 {
        // Fast path: a lone builtin runs in the shell process so it can
        // mutate shell state (cd / export / unset / exit).
        if pipeline.len() == 1 && !pipeline[0].argv.is_empty() && is_builtin(&pipeline[0].argv[0])
        {
            return self.run_builtin_with_redirects(&pipeline[0]);
        }

        let n = pipeline.len();
        let mut statuses = vec![0i32; n];
        let mut children: Vec<(usize, String, Child)> = Vec::new();
        let mut prev = PrevInput::None;
        let mut aborted = false;

        for (idx, cmd) in pipeline.iter().enumerate() {
            let last = idx == n - 1;
            if aborted {
                statuses[idx] = 1;
                prev = PrevInput::None;
                continue;
            }

            // Open redirection files first; on failure the command must not run.
            let redirects = match open_redirects(cmd) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("minishell: {e}");
                    statuses[idx] = 1;
                    aborted = true; // abort the rest of the pipeline
                    drain_prev(prev);
                    prev = PrevInput::None;
                    continue;
                }
            };
            let (in_file, out_file) = redirects;

            if cmd.argv.is_empty() {
                // Redirection-only command, e.g. `> file`: just create/truncate.
                statuses[idx] = 0;
                drain_prev(prev);
                prev = PrevInput::None;
                continue;
            }

            if is_builtin(&cmd.argv[0]) {
                // Builtins inside a pipeline run against a cloned state
                // (subshell semantics): they cannot mutate the parent shell.
                let mut subshell = Shell {
                    env: self.env.clone(),
                    last_status: self.last_status,
                    last_pipeline_statuses: Vec::new(),
                    history: self.history.clone(),
                    running: true,
                    exit_code: 0,
                };
                let input: Box<dyn Read> = if let Some(f) = in_file {
                    Box::new(f)
                } else {
                    match prev {
                        PrevInput::Pipe(mut p) => {
                            let mut buf = Vec::new();
                            let _ = p.read_to_end(&mut buf);
                            Box::new(std::io::Cursor::new(buf))
                        }
                        PrevInput::Buffer(b) => Box::new(std::io::Cursor::new(b)),
                        PrevInput::None => Box::new(std::io::empty()),
                    }
                };
                let mut out_buf: Vec<u8> = Vec::new();
                let status = subshell.run_builtin(
                    &cmd.argv[0],
                    &cmd.argv[1..],
                    Box::new(input),
                    &mut out_buf,
                );
                statuses[idx] = status;
                if last {
                    match out_file {
                        Some(mut f) => {
                            let _ = f.write_all(&out_buf);
                        }
                        None => {
                            let _ = std::io::stdout().write_all(&out_buf);
                            let _ = std::io::stdout().flush();
                        }
                    }
                    prev = PrevInput::None;
                } else {
                    prev = PrevInput::Buffer(out_buf);
                }
                continue;
            }

            // External command.
            let mut proc = ProcCommand::new(&cmd.argv[0]);
            proc.args(&cmd.argv[1..]).env_clear().envs(&self.env);

            let mut write_after_spawn: Option<Vec<u8>> = None;
            if let Some(f) = in_file {
                proc.stdin(Stdio::from(f));
            } else {
                match prev {
                    PrevInput::Pipe(p) => {
                        proc.stdin(Stdio::from(p));
                    }
                    PrevInput::Buffer(b) => {
                        proc.stdin(Stdio::piped());
                        write_after_spawn = Some(b);
                    }
                    PrevInput::None => {
                        proc.stdin(Stdio::inherit());
                    }
                }
            }

            let has_out_file = out_file.is_some();
            if let Some(f) = out_file {
                proc.stdout(Stdio::from(f));
            } else if !last {
                proc.stdout(Stdio::piped());
            } else {
                proc.stdout(Stdio::inherit());
            }

            match proc.spawn() {
                Ok(mut child) => {
                    if let Some(buf) = write_after_spawn {
                        if let Some(mut stdin) = child.stdin.take() {
                            let _ = stdin.write_all(&buf); // ignore broken pipe
                        }
                    }
                    if !last && !has_out_file {
                        match child.stdout.take() {
                            Some(out) => prev = PrevInput::Pipe(out),
                            None => prev = PrevInput::Buffer(Vec::new()),
                        }
                    } else {
                        prev = PrevInput::None;
                    }
                    children.push((idx, cmd.argv[0].clone(), child));
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        eprintln!("minishell: {}: command not found", cmd.argv[0]);
                        statuses[idx] = 127;
                    } else {
                        eprintln!("minishell: {}: {e}", cmd.argv[0]);
                        statuses[idx] = 126;
                    }
                    prev = PrevInput::Buffer(Vec::new());
                }
            }
        }

        // Reap every child: no zombies, and collect all exit statuses.
        for (idx, name, mut child) in children {
            match child.wait() {
                Ok(s) => {
                    statuses[idx] = status_code(s);
                }
                Err(e) => {
                    eprintln!("minishell: {name}: wait failed: {e}");
                    statuses[idx] = 1;
                }
            }
        }

        self.last_pipeline_statuses = statuses.clone();
        *statuses.last().unwrap_or(&0)
    }

    /// Run a builtin as the only command of its line, applying redirections
    /// in the shell process itself.
    fn run_builtin_with_redirects(&mut self, cmd: &Command) -> i32 {
        let (in_file, out_file) = match open_redirects(cmd) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("minishell: {e}");
                return 1;
            }
        };
        let input: Box<dyn Read> = match in_file {
            Some(f) => Box::new(f),
            None => Box::new(std::io::empty()),
        };
        let mut output: Box<dyn Write> = match out_file {
            Some(f) => Box::new(f),
            None => Box::new(std::io::stdout()),
        };
        let status = self.run_builtin(&cmd.argv[0], &cmd.argv[1..], input, &mut output);
        let _ = output.flush();
        status
    }

    fn run_builtin(
        &mut self,
        name: &str,
        args: &[String],
        _stdin: Box<dyn Read>,
        stdout: &mut dyn Write,
    ) -> i32 {
        match name {
            "cd" => {
                let target = args
                    .first()
                    .cloned()
                    .or_else(|| self.env.get("HOME").cloned())
                    .unwrap_or_else(|| "/".to_string());
                match std::env::set_current_dir(&target) {
                    Ok(()) => {
                        if let Ok(cwd) = std::env::current_dir() {
                            self.env
                                .insert("PWD".to_string(), cwd.display().to_string());
                        }
                        0
                    }
                    Err(e) => {
                        eprintln!("minishell: cd: {target}: {e}");
                        1
                    }
                }
            }
            "pwd" => match std::env::current_dir() {
                Ok(p) => {
                    let _ = writeln!(stdout, "{}", p.display());
                    0
                }
                Err(e) => {
                    eprintln!("minishell: pwd: {e}");
                    1
                }
            },
            "export" => {
                if args.is_empty() {
                    let mut keys: Vec<&String> = self.env.keys().collect();
                    keys.sort();
                    for k in keys {
                        let _ = writeln!(stdout, "declare -x {k}=\"{}\"", self.env[k]);
                    }
                    return 0;
                }
                let mut status = 0;
                for arg in args {
                    match arg.split_once('=') {
                        Some((k, v)) if !k.is_empty() => {
                            self.env.insert(k.to_string(), v.to_string());
                        }
                        Some((_, _)) => {
                            eprintln!("minishell: export: `{arg}': not a valid identifier");
                            status = 1;
                        }
                        None => {
                            if self.env.contains_key(arg) {
                                // already exported; nothing to do
                            } else {
                                self.env.insert(arg.clone(), String::new());
                            }
                        }
                    }
                }
                status
            }
            "unset" => {
                for arg in args {
                    self.env.remove(arg);
                }
                0
            }
            "exit" => {
                let code = match args.first() {
                    Some(s) => match s.parse::<i32>() {
                        Ok(n) => n,
                        Err(_) => {
                            eprintln!("minishell: exit: {s}: numeric argument required");
                            2
                        }
                    },
                    None => self.last_status,
                };
                self.running = false;
                self.exit_code = code;
                code
            }
            "echo" => {
                let (newline, rest) = match args.first() {
                    Some(a) if a == "-n" => (false, &args[1..]),
                    _ => (true, args),
                };
                let _ = write!(stdout, "{}", rest.join(" "));
                if newline {
                    let _ = writeln!(stdout);
                }
                0
            }
            "env" => {
                let mut keys: Vec<&String> = self.env.keys().collect();
                keys.sort();
                for k in keys {
                    let _ = writeln!(stdout, "{k}={}", self.env[k]);
                }
                0
            }
            "history" => {
                for (i, h) in self.history.iter().enumerate() {
                    let _ = writeln!(stdout, "{:5}  {h}", i + 1);
                }
                0
            }
            _ => {
                eprintln!("minishell: unknown builtin: {name}");
                1
            }
        }
    }
}

/// Open all redirection files of a command. Returns (stdin_file, stdout_file).
fn open_redirects(cmd: &Command) -> Result<(Option<File>, Option<File>), String> {
    let mut in_file = None;
    let mut out_file = None;
    for r in &cmd.redirects {
        match r.kind {
            RedirectKind::Input => {
                let f = File::open(&r.target)
                    .map_err(|e| format!("{}: {e}", r.target))?;
                in_file = Some(f);
            }
            RedirectKind::Output => {
                let f = File::create(&r.target)
                    .map_err(|e| format!("{}: {e}", r.target))?;
                out_file = Some(f);
            }
            RedirectKind::Append => {
                let f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&r.target)
                    .map_err(|e| format!("{}: {e}", r.target))?;
                out_file = Some(f);
            }
        }
    }
    Ok((in_file, out_file))
}

fn drain_prev(prev: PrevInput) {
    if let PrevInput::Pipe(mut p) = prev {
        let mut sink = Vec::new();
        let _ = p.read_to_end(&mut sink);
    }
}

fn status_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        code
    } else if let Some(sig) = status.signal() {
        128 + sig
    } else {
        1
    }
}
