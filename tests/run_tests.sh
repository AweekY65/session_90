#!/usr/bin/env bash
# minishell integration tests.
# Every case runs the shell via `cargo run --bin minishell -- ...` and
# compares stdout / exit codes. Prints PASS/FAIL per case and a summary.
# Only local processes and the local filesystem are used.

set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PASS=0
FAIL=0

# Run one line through the shell; stdout captured, exit code in $RC.
run_c() {
    OUT="$(cargo run --quiet --bin minishell -- -c "$1" 2>/dev/null)"
    RC=$?
}

# Run a multi-line script through the shell.
run_script() {
    OUT="$(cargo run --quiet --bin minishell -- "$1" 2>/dev/null)"
    RC=$?
}

check() { # name expected actual
    if [ "$2" = "$3" ]; then
        echo "PASS: $1"
        PASS=$((PASS + 1))
    else
        echo "FAIL: $1 (expected [$2], got [$3])"
        FAIL=$((FAIL + 1))
    fi
}

check_rc() { # name expected-rc actual-rc
    if [ "$2" = "$3" ]; then
        echo "PASS: $1"
        PASS=$((PASS + 1))
    else
        echo "FAIL: $1 (expected rc=$2, got rc=$3)"
        FAIL=$((FAIL + 1))
    fi
}

TMP="$ROOT/tests/tmp"
mkdir -p "$TMP"

echo "== building =="
cargo build --quiet || { echo "BUILD FAILED"; exit 1; }

echo "== quoting =="
run_c "echo 'a  b'"
check "single quotes preserve spaces" "a  b" "$OUT"
run_c 'echo "x  y"'
check "double quotes preserve spaces" "x  y" "$OUT"
run_c 'echo a\ b'
check "backslash escape" "a b" "$OUT"
run_c 'echo "she said \"hi\""'
check "escaped quote in double quotes" 'she said "hi"' "$OUT"
run_c "echo ''"
check "empty single-quoted arg" "" "$OUT"
run_c "echo '\$HOME'"
check "no expansion in single quotes" '$HOME' "$OUT"

echo "== variable expansion =="
cat > "$TMP/vars.ms" <<'EOS'
export MS_FOO=bar
echo $MS_FOO
echo ${MS_FOO}
echo "x${MS_FOO}y"
echo "[$MS_NO_SUCH_VAR]"
echo $?
false
echo $?
EOS
run_script "$TMP/vars.ms"
check "expansion in one shell process" "bar
bar
xbary
[]
0
1" "$OUT"

echo "== pipeline =="
run_c 'echo hello | tr a-z A-Z'
check "two-stage pipeline" "HELLO" "$OUT"
run_c 'printf "b\na\nc\n" | sort | head -1'
check "three-stage pipeline" "a" "$OUT"
run_c 'seq 1 100000 | wc -l'
check "large data through pipe" "100000" "$OUT"
run_c 'echo hi | cat | cat | wc -c'
check "multi-stage byte count" "3" "$OUT"

echo "== redirection =="
run_c "echo line1 > $TMP/out.txt"
check_rc "write redirect rc" 0 "$RC"
check "write redirect content" "line1" "$(cat "$TMP/out.txt")"
run_c "echo line2 >> $TMP/out.txt"
check "append redirect" "line1
line2" "$(cat "$TMP/out.txt")"
run_c "wc -l < $TMP/out.txt"
check "input redirect" "2" "$(echo "$OUT" | tr -d ' ')"
run_c 'cat < /nonexistent_file_xyz_123'
check_rc "missing input file fails" 1 "$RC"
run_c 'echo hi > /nonexistent_dir_xyz_123/f'
check_rc "unwritable output fails, command not run" 1 "$RC"
run_c "cat < /nonexistent_file_xyz_123 | wc -l"
check_rc "redirect failure aborts pipeline" 1 "$RC"
run_c "echo abc | cat > $TMP/p.txt"
check "pipeline into file" "abc" "$(cat "$TMP/p.txt")"

echo "== builtins =="
run_c 'pwd'
check "pwd" "$ROOT" "$OUT"
cat > "$TMP/cd.ms" <<'EOS'
cd /
pwd
cd /nonexistent_dir_xyz_123
echo cd_failed=$?
EOS
run_script "$TMP/cd.ms"
check "cd changes shell state" "/
cd_failed=1" "$OUT"
cat > "$TMP/env.ms" <<'EOS'
export MS_A=1
echo $MS_A
unset MS_A
echo "[$MS_A]"
export MS_B=seen
env
EOS
run_script "$TMP/env.ms"
check "export/unset in one shell process" "1
[]
$(printf '%s\n' "$OUT" | sed -n '3,$p')" "$OUT"
case "$OUT" in
    *MS_B=seen*) check "env lists exported var" "yes" "yes" ;;
    *)           check "env lists exported var" "yes" "no"  ;;
esac
run_c 'echo builtin-echo | cat'
check "builtin in pipeline" "builtin-echo" "$OUT"

echo "== exit codes / failures =="
run_c 'true'
check_rc "true exits 0" 0 "$RC"
run_c 'false'
check_rc "false exits 1" 1 "$RC"
run_c 'nonexistent_cmd_xyz_123'
check_rc "command not found is 127" 127 "$RC"
run_c 'false | true'
check_rc "pipeline status is last command (0)" 0 "$RC"
run_c 'true | false'
check_rc "pipeline status is last command (1)" 1 "$RC"
run_c 'exit 42'
check_rc "exit builtin sets status" 42 "$RC"
run_c 'echo |'
check_rc "trailing pipe is syntax error" 2 "$RC"
run_c 'echo >'
check_rc "missing redirect target is syntax error" 2 "$RC"

echo "== zombie check =="
cat > "$TMP/zombie.ms" <<'EOS'
seq 1 100 | wc -l
echo a | cat | cat | wc -c
sleep 2
EOS
cargo run --quiet --bin minishell -- "$TMP/zombie.ms" >/dev/null 2>&1 &
MS_PID=$!
sleep 1
if command -v ps >/dev/null 2>&1; then
    ZOMBIES="$(ps -o stat= --ppid "$MS_PID" 2>/dev/null | grep -c Z)"
    check "no zombie children while shell alive" "0" "$ZOMBIES"
else
    echo "SKIP: ps not available for zombie check"
fi
wait "$MS_PID"

echo
echo "== summary: $PASS passed, $FAIL failed =="
[ "$FAIL" -eq 0 ]
