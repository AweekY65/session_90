# minishell

一个本地迷你 Unix Shell，纯 Rust 标准库实现（无第三方依赖）。所有命令执行、
环境状态与测试均只使用本地进程与本地文件系统，不连接任何远程主机、
容器服务或外部服务。

## 构建与运行

```sh
cargo build
cargo run --bin minishell                 # 交互模式（TTY 下显示 minishell$ 提示符）
cargo run --bin minishell -- -c 'echo hi' # 执行命令字符串后退出
cargo run --bin minishell -- script.ms    # 执行脚本文件
printf 'echo hi\n' | cargo run --bin minishell   # 从标准输入读取
```

`-c` 与脚本文件内容按行执行，每行可包含多条以 `;` 分隔的命令；
进程退出码为最后一条命令的退出码。

## 语法范围

- **词法（tokenizer）**：普通参数；单引号（完全字面）；双引号（内部仍展开
  变量，`\` 仅转义 `$`、`"`、`\`、换行）；反斜杠转义；`#` 行注释。
- **变量展开**：`$VAR`、`${VAR}`、`$?`（上一条命令退出码）、`$$`（Shell PID）。
  未定义变量展开为空串。变量在单引号内不展开。同一行中按 `;` 逐条
  tokenize，因此 `export A=1; echo $A` 与 `false; echo $?` 能取到新值。
- **运算符**：`|`（管道）、`>`（覆盖重定向）、`>>`（追加重定向）、
  `<`（输入重定向）、`;`（命令分隔）。
- **不支持**（有意简化）：`&&`/`||`、命令替换 `$( )`、通配符 glob、
  后台任务 `&`、作业控制、here-doc、tilde 展开、变量展开后的单词拆分。

## 进程模型

- 每条外部命令通过 `std::process::Command`（fork+exec）运行，argv 原样
  传递，环境变量取自 Shell 自身维护的环境表（`env_clear` + `envs`，因此
  `export PATH=...` 会影响后续命令查找）。
- pipeline 中所有进程并发启动，前一进程的 stdout 接到后一进程的 stdin。
- 每个被 spawn 的子进程都会在 pipeline 结束前被 `wait()` 回收，不会留下
  zombie process。
- 退出码：pipeline 的退出码为最后一个命令的退出码；命令不存在为 127，
  无法执行为 126，被信号杀死为 128+signo，语法/词法错误为 2。
  中间命令失败不会中断其余进程的启动与回收。
- 子进程继承 Shell 的 stderr 与（未被重定向时的）stdin/stdout；Rust 运行时在
  exec 前会把子进程的 SIGPIPE 恢复为默认行为，`yes | head` 这类管道可正常终止。

## 文件描述符管理

- 相邻命令之间用 `std::io::pipe()` 创建管道；每个命令启动前，属于它的管道
  读/写端立即从 Shell 的表中取出：要么移动进子进程，要么当场 drop。
- 重定向文件在启动命令之前打开；打开失败时打印错误、跳过该命令的执行
  （状态置 1），其管道端点随之 drop，下游进程读到 EOF。
- `Command` 在 spawn 后立即析构，Shell 进程在等待期间不持有子进程管道的
  冗余副本，下游进程因此能正确观察到 EOF，不会死锁。
- builtin 在 pipeline 中直接把输出写入管道写端（流式，不经过内存缓冲）。

## 内建命令（修改 Shell 自身状态）

- `cd [dir]`：切换工作目录（无参数时用 `$HOME`），同步更新 `PWD`/`OLDPWD`。
- `pwd`：打印当前工作目录。
- `export [NAME[=VALUE] ...]`：设置/标记环境变量；无参数时列出全部变量。
- `unset NAME ...`：删除环境变量。
- `echo [-n] ...`：回显参数（便于脱离外部 echo 测试）。
- `exit [n]`：以指定状态码（缺省为上一条命令的状态码）退出 Shell。

单独出现时 builtin 在当前 Shell 进程内执行；出现在 pipeline 中时按
subshell 语义在环境副本上执行，状态修改（如 `cd`、`export`）不会保留，
`exit` 也不会终止整个 Shell。

## 测试

不使用 `cargo test`。测试脚本为 `tests/run_tests.sh`，通过
`cargo run --bin minishell -- ...` 驱动被测 Shell，使用系统基础命令
（`echo`、`printf`、`cat`、`tr`、`grep`、`wc`、`printenv`、`ps`、`awk`）
验证 quoting、变量展开、pipeline、重定向、builtin、失败命令的退出码以及
zombie 回收，逐项输出 `PASS`/`FAIL` 并给出汇总：

```sh
bash tests/run_tests.sh
```

全部通过时脚本退出码为 0，否则为 1。
