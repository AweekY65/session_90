# minishell

一个本地迷你 Unix Shell，使用 Rust 编写（仅标准库，无第三方依赖）。
所有命令执行、环境状态与历史均只使用本地进程与本地文件系统，
不连接任何远程主机、容器服务或外部服务。

## 构建与运行

```sh
cargo build
cargo run --bin minishell                 # 交互式 REPL（提示符 minishell>）
cargo run --bin minishell -- -c 'echo hi' # 执行单行命令，进程退出码即命令退出码
cargo run --bin minishell -- script.ms    # 逐行执行脚本文件
```

## 语法范围

- **普通参数**：以空白分隔的单词，原样构成 argv。
- **单引号** `'...'`：完全字面量，不做变量展开与转义。
- **双引号** `"..."`：保留空白，支持 `$VAR` / `${VAR}` / `$?` 展开，
  以及 `\"`、`\\`、`\$` 转义。
- **转义**：引号外 `\c` 表示字面字符 `c`。
- **变量展开**：`$VAR`、`${VAR}`、`$?`（上一条命令退出码）；
  未定义变量展开为空串。展开结果不再做单词拆分（word splitting）。
- **管道**：`cmd1 | cmd2 | cmd3`。
- **重定向**：`> file`（截断写）、`>> file`（追加写）、`< file`（读）。
  仅含重定向的命令（如 `> f`）只创建/截断文件。
- **注释**：单词起始处的 `#` 到行尾为注释。
- **Builtin**：`cd`、`pwd`、`export`、`unset`、`exit`、`echo`、`env`、`history`。

不支持（明确超出范围）：`;`、`&&`、`||`、`&` 后台、通配符 glob、
命令替换 `$(...)`、here-doc、多行续行。

## 进程模型

- Shell 主进程读取一行 → `lexer` 切词（含引号/转义/变量展开）→
  `parser` 生成 pipeline（`Vec<Command>`，每段含 argv 与重定向列表）→
  `executor` 执行。
- **外部命令**通过 `std::process::Command` fork/exec 为子进程，
  argv 原样传递；环境为 Shell 自身维护的 `env` 表
  （`env_clear().envs(...)`，不受父进程环境污染）。
- **Pipeline**：每段一个子进程，前一段 stdout 经管道接后一段 stdin。
  所有子进程先全部 spawn 再统一 `wait()`，每个子进程都被回收，
  不产生 zombie；每个阶段的退出状态都被收集到
  `last_pipeline_statuses`，pipeline 的退出状态取最后一段（与 bash 一致）。
- **Builtin**：单独成行时在 Shell 进程内执行，可修改 Shell 自身状态
  （`cd` 改工作目录、`export`/`unset` 改环境表、`exit` 终止 Shell）；
  出现在 pipeline 中时在克隆的状态上运行（子Shell语义），不影响父 Shell。
- **失败处理**：命令不存在返回 127；重定向文件打开失败返回 1 且
  该命令不会执行，pipeline 其余部分中止；语法错误返回 2；
  被信号杀死的子进程返回 128+signo。

## 文件描述符管理

- 重定向文件在命令执行前打开；打开失败即报错并跳过该命令。
- 相邻进程间通过 `Stdio::piped()` 建立管道，上一段的 `ChildStdout`
  以 `Stdio::from` 移交给下一段作为 stdin，父进程不再持有冗余 fd。
- `std::process::Command` 在子进程中自动关闭无关 fd（CLOEXEC），
  管道写端在所有者处随 `Child`/`ChildStdout` 的 drop 关闭，
  保证下游能正确收到 EOF，大流量数据（如 `seq 1 100000 | wc -l`）不会死锁。
- Builtin 的输入/输出以 `Read`/`Write` 抽象注入，重定向时直接绑定到
  打开的文件，无需在 Shell 进程中 dup 标准 fd。

## 测试

不使用 `cargo test`。集成测试为独立终端脚本，全部通过
`cargo run --bin minishell -- ...` 驱动，逐项输出 PASS/FAIL：

```sh
bash tests/run_tests.sh
```

覆盖：引号与转义、变量展开（`$VAR`/`${VAR}`/`$?`/未定义变量）、
多级 pipeline 与大数据量、三种重定向及打开失败中止、
builtin 状态修改（cd/pwd/export/unset/exit/env）、
失败命令与 pipeline 退出码（127/1/0/语法错误 2）、zombie 进程检查。
脚本退出码为 0 表示全部通过。
