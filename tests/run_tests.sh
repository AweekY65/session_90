#!/usr/bin/env bash
# minishell test suite.
#
# Uses only local processes and the local filesystem: the shell under test
# is started via `cargo run --bin minishell -- ...` and exercised with
# standard system utilities (echo, printf, cat, tr, grep, wc, printenv, ps).
#
# Usage: tests/run_tests.sh
# Exit status: 0 if all tests pass, 1 otherwise.

set -u
cd "$(dirname "$0")/.."

PASS=0
FAIL=0

TMPDIR_TEST="$(mktemp -d /tmp/minishell_test.XXXXXX)"
trap 'rm -rf "$TMPDIR_TEST"' EXIT

# Run a script through minishell, return its stdout.
shell() {
  cargo run --quiet --bin minishell -- -c "$1"
}

check() {
  local name="$1" expected="$2" actual="$3"
  if [ "$actual" = "$expected" ]; then
    echo "PASS: $name"
    PASS=$((PASS + 1))
  else
    echo "FAIL: $name"
    echo "  expected: $(printf '%s' "$expected" | sed -n l)"
    echo "  actual:   $(printf '%s' "$actual" | sed -n l)"
    FAIL=$((FAIL + 1))
  fi
}

# --- quoting -------------------------------------------------------------
check "quote-single"        'hello   world'  "$(shell "echo 'hello   world'")"
check "quote-double"        'hello   world'  "$(shell 'echo "hello   world"')"
check "quote-escape-space"  'hello world'    "$(shell 'echo hello\ world')"
check "quote-mixed"         'abc'            "$(shell "echo \"a\"b'c'")"
check "quote-escape-in-dq"  'say "hi"'       "$(shell 'echo "say \"hi\""')"
check "quote-empty-args"    'a||b'           "$(shell "printf '%s|%s|%s\n' a '' b" | cat)"

# --- variable expansion ---------------------------------------------------
check "expand-in-dq"        'val=expanded'   "$(shell 'export MS_T=expanded; echo "val=$MS_T"')"
check "no-expand-in-sq"     '$MS_T'          "$(shell 'export MS_T=x; echo '"'"'$MS_T'"'"'')"
check "expand-braces"       'abcd'           "$(shell 'export MS_A=ab; echo ${MS_A}cd')"
check "expand-undefined"    '[]'             "$(shell 'echo "[${MS_UNDEFINED_XYZ}]"')"
check "expand-escape-dollar" 'price: $5'     "$(shell 'echo "price: \$5"')"
check "expand-status-ok"    '0'              "$(shell 'true; echo $?')"
check "expand-status-fail"  '1'              "$(shell 'false; echo $?')"
check "expand-env-in-child" 'hello_child'    "$(shell 'export MS_E=hello_child; printenv MS_E')"

# --- argv passing ----------------------------------------------------------
check "argv-spaces"         'one|two words|three' \
  "$(shell "printf '%s|%s|%s\n' one 'two words' three")"

# --- pipelines -------------------------------------------------------------
check "pipe-basic"          'HELLO'          "$(shell "echo hello | tr 'a-z' 'A-Z'")"
check "pipe-three-stage"    '1'              "$(shell "printf 'a\nb\nc\n' | grep b | wc -l | tr -d ' '")"
check "pipe-builtin-stage"  'HELLO'          "$(shell "echo hello | tr 'a-z' 'A-Z' | cat")"
check "pipe-exit-last-ok"   '0'              "$(shell 'false | true; echo $?')"
check "pipe-exit-last-fail" '1'              "$(shell 'true | false; echo $?')"
check "pipe-missing-cmd-mid" 'rc=0'          "$(shell 'echo x | no_such_cmd_xyz_123 | cat > /dev/null; echo rc=$?')"
check "pipe-missing-cmd-last" '127'          "$(shell 'echo x | no_such_cmd_xyz_123 > /dev/null; echo $?')"
check "pipe-stdin-eof"      'done'           "$(shell 'echo one | cat | cat > /dev/null; echo done')"

# --- redirections ----------------------------------------------------------
check "redir-out"           'hi'             "$(shell "echo hi > '$TMPDIR_TEST/out'; cat '$TMPDIR_TEST/out'")"
check "redir-append"        'a
b'                                            "$(shell "echo a > '$TMPDIR_TEST/app'; echo b >> '$TMPDIR_TEST/app'; cat '$TMPDIR_TEST/app'")"
printf 'from-file\n' > "$TMPDIR_TEST/in"
check "redir-in"            'from-file'      "$(shell "cat < '$TMPDIR_TEST/in'")"
check "redir-out-fail"      'status=1'       "$(shell "echo x > /no_such_dir_xyz/f; echo status=\$?")"
check "redir-in-fail"       'status=1'       "$(shell "cat < /no_such_file_xyz_123; echo status=\$?")"
check "redir-fail-skips-cmd" '127'           "$(shell "no_such_cmd_xyz_123 > '$TMPDIR_TEST/created'; echo \$?")"
check "redir-pipe-combined" 'piped'          "$(shell "echo piped | cat > '$TMPDIR_TEST/rp'; cat < '$TMPDIR_TEST/rp'")"

# --- builtins --------------------------------------------------------------
check "builtin-pwd"         '/'              "$(shell 'cd /; pwd')"
check "builtin-cd-relative" '/'              "$(shell 'cd /tmp; cd ..; pwd')"
check "builtin-export-unset" '[]'            "$(shell 'export MS_U=1; unset MS_U; echo "[${MS_U}]"')"
check "builtin-unset-child" 'status=1'       "$(shell 'export MS_V=1; unset MS_V; printenv MS_V >/dev/null 2>&1; echo status=$?')"
check "builtin-export-list" 'export MS_L=1'  "$(shell 'export MS_L=1; export | grep ^export.MS_L=')"
check "builtin-cd-fail"     '1'              "$(shell 'cd /no_such_dir_xyz; echo $?')"

# --- failing commands ------------------------------------------------------
check "cmd-not-found"       '127'            "$(shell 'no_such_cmd_xyz_123; echo $?')"
check "cmd-exit-code-propagates" '3'         "$(shell 'exit 3' >/dev/null; echo $?)"
check "shell-exit-status"   '1'              "$(shell 'false' >/dev/null; echo $?)"
check "shell-continues-after-error" 'ok'     "$(shell 'no_such_cmd_xyz_123; echo ok')"

# --- zombie reaping ---------------------------------------------------------
if command -v ps >/dev/null 2>&1 && command -v awk >/dev/null 2>&1; then
  check "no-zombies" '0' \
    "$(shell 'true | true | true; ps -eo stat=,ppid= | awk -v p=$$ '"'"'$2==p && $1 ~ /^Z/ {c++} END {print c+0}'"'"'')"
else
  echo "SKIP: no-zombies (ps or awk not available)"
fi

# --- script file mode -------------------------------------------------------
cat > "$TMPDIR_TEST/script.ms" <<'SCRIPT'
# comment line
export MS_S=file_mode
echo "mode=$MS_S"
SCRIPT
check "script-file" 'mode=file_mode' "$(cargo run --quiet --bin minishell -- "$TMPDIR_TEST/script.ms")"

# --- summary -----------------------------------------------------------------
echo
echo "passed: $PASS, failed: $FAIL"
[ "$FAIL" -eq 0 ]
