#!/usr/bin/env bash
# Run the action against a local revision fixture. All publication commands are stubbed.
set -euo pipefail
script=$(cd "$(dirname "$0")/.." && pwd)/step-diff.sh
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
export REAL_GIT
REAL_GIT=$(command -v git)
mkdir -p "$fixture/bin" "$fixture/repo" "$fixture/release/specify-core-test-x86_64-unknown-linux-gnu"
cat > "$fixture/bin/git" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  worktree) mkdir -p "${@: -1}" ;;
  ls-remote) exit 2 ;;
  checkout|rm|clean|add) ;;
  -c)
    if [[ $2 == user.name=* ]]; then exit 1; fi
    exec "$REAL_GIT" "$@"
    ;;
  push|fetch|rebase) echo 'Unexpected publication command' >&2; exit 99 ;;
  *) exec "$REAL_GIT" "$@" ;;
esac
STUB
cat > "$fixture/bin/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [ "$1 $2" = 'release download' ]; then
  cp "$TEST_RELEASE"/*tar.gz* "$RUNNER_TEMP/"
elif [ "$1" = api ]; then
  for arg in "$@"; do
    case "$arg" in body=@*) cp "${arg#body=@}" "$CAPTURED_COMMENT" ;; esac
  done
else
  echo 'Unexpected gh call' >&2
  exit 99
fi
STUB
cat > "$fixture/bin/uname" <<'STUB'
#!/usr/bin/env bash
case "$1" in -s) echo Linux ;; -m) echo x86_64 ;; *) exit 99 ;; esac
STUB
# macOS has shasum; the action's Linux runners have sha256sum.
if ! command -v sha256sum > /dev/null; then
  cat > "$fixture/bin/sha256sum" <<'STUB'
#!/usr/bin/env bash
exec shasum -a 256 "$@"
STUB
fi
cat > "$fixture/release/specify-core-test-x86_64-unknown-linux-gnu/specify-core" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
while [ "$1" != -o ]; do shift; done
out=$2
printf '### plate.step: Specify\n\n✅ All 2 requirements still apply to the changed part.\n' > "$out/report.md"
printf '%s\n' "$TEST_REPORT" > "$out/report.json"
printf '%s\n' "$TEST_CARRIED" > "$out/answers.json"
STUB
chmod +x "$fixture/bin/"* "$fixture/release/"*/specify-core
export PATH="$fixture/bin:$PATH" TEST_RELEASE="$fixture/release"
(cd "$TEST_RELEASE" && tar -czf specify-core-test-x86_64-unknown-linux-gnu.tar.gz specify-core-test-x86_64-unknown-linux-gnu && sha256sum specify-core-test-x86_64-unknown-linux-gnu.tar.gz > specify-core-test-x86_64-unknown-linux-gnu.tar.gz.sha256)
cd "$fixture/repo"
"$REAL_GIT" init -q -b main
"$REAL_GIT" config user.name 'Fixture'
"$REAL_GIT" config user.email 'fixture@example.invalid'
printf 'old STEP fixture\n' > plate.step
printf '{"hole.function:holes:9":"general"}\n' > plate.answers.json
"$REAL_GIT" add .
"$REAL_GIT" commit -qm base
printf 'new STEP fixture\n' > plate.step
"$REAL_GIT" add .
"$REAL_GIT" commit -qm revision
export PR=1 HEAD_REPO=fixture/repo REPO=fixture/repo VERSION=test ACTION_REF=main
export PR_HEAD TEST_CARRIED TEST_REPORT
PR_HEAD=$("$REAL_GIT" rev-parse HEAD)
TEST_CARRIED='{"hole.function:holes:8":"general","part.material":"6061-T6"}'
TEST_REPORT='{"verdict":"ok"}'
run_case() {
  export RUNNER_TEMP="$fixture/$1" PRIVATE=$2 CAPTURED_COMMENT="$fixture/$1.md"
  printf '%s\n' "$1" >> plate.step
  "$REAL_GIT" add -A
  "$REAL_GIT" commit -q --allow-empty -m "$1"
  PR_HEAD=$("$REAL_GIT" rev-parse HEAD)
  mkdir -p "$RUNNER_TEMP"
  bash "$script" > "$fixture/$1.log" 2>&1 || { cat "$fixture/$1.log"; return 1; }
  test -f "$CAPTURED_COMMENT"
  ! grep -Eq '```json|<details>|hole.function|6061-T6' "$CAPTURED_COMMENT"
  grep -q 'All 2 requirements still apply' "$CAPTURED_COMMENT"
  test -f "$RUNNER_TEMP/step-diff/0/answers.json"
}
run_case stale false
grep -Fq '`plate.answers.json` is stale: its requirements differ from this comparison. [Review in 3D](https://draftwright.io/diff?' "$CAPTURED_COMMENT"
# Equal JSON with different order/whitespace is current.
printf '{ "part.material": "6061-T6", "hole.function:holes:8": "general" }\n' > plate.answers.json
run_case current false
! grep -q 'is stale' "$CAPTURED_COMMENT"
grep -q 'Turn the part round in 3D' "$CAPTURED_COMMENT"
rm plate.answers.json
run_case absent false
! grep -q 'is stale' "$CAPTURED_COMMENT"
printf '{}\n' > plate.answers.json
run_case private true
grep -Fq '`plate.answers.json` is stale' "$CAPTURED_COMMENT"
! grep -q 'draftwright.io' "$CAPTURED_COMMENT"
TEST_REPORT='{"parts":[]}'
TEST_CARRIED='{"plate":{"part.material":"6061-T6","_specify_history":{"number":7}}}'
run_case assembly false
grep -Fq '`plate.answers.json` is stale' "$CAPTURED_COMMENT"
printf '{"plate":{"_specify_history":{"number":7},"part.material":"6061-T6"}}\n' > plate.answers.json
run_case assembly_current false
! grep -q 'is stale' "$CAPTURED_COMMENT"
grep -q 'Turn the assembly round in 3D' "$CAPTURED_COMMENT"
echo 'step-diff comment: 6 cases passed'
