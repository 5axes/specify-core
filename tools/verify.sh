#!/usr/bin/env bash
# One command that runs what CI runs on a pull request, on this machine, against this checkout's own
# build. It ends with one line: `VERIFY: PASS` or `VERIFY: FAIL (<steps>)` (working-rules.md, rule 1).
#
#   tools/verify.sh             run every step
#   tools/verify.sh --dry-run   list the steps, run nothing
#
# It mirrors, step for step, the pull-request jobs of:
#   ci.yml       test (the Linux "Full suite" form), wasm, ui, app
#   python.yml   wheel (this machine's platform), test (pytest against that wheel)
#   crates.yml   package
#   bundle.yml   the bundle/ layout the wheel and the crate are built from
# It runs every job whatever the PR changes, as main does (CI skips some on a PR by path). Not run:
# the other platforms' builds (cli.yml, python.yml's matrix: CI covers them), dco.yml (a sign-off
# check on commits), and steps that only run on main or a release.
#
# When a workflow gains or loses a pull-request step, this script changes in the same PR.
#
# Needs: the Rust toolchain (rust-toolchain.toml) with wasm32-unknown-unknown, wasm-pack 0.15.0,
# node 22, python3, uv, jq. The quiddity corpus: SPECIFY_CORPUS (default ../quiddity/tests/corpus).

set -uo pipefail
cd "$(dirname "$0")/.."
root=$PWD

dry=0
case "${1:-}" in
  --dry-run) dry=1 ;;
  "") ;;
  *) echo "usage: tools/verify.sh [--dry-run]" >&2; exit 2 ;;
esac

corpus=${SPECIFY_CORPUS:-$root/../quiddity/tests/corpus}
export SPECIFY_CORPUS=$corpus QUIDDITY_CORPUS=$corpus QUIDDITY_CORPUS_REQUIRED=1
export CARGO_TERM_COLOR=never
if [ "$dry" = 1 ]; then work=${TMPDIR:-/tmp}/specify-verify.DRYRUN; else work=$(mktemp -d "${TMPDIR:-/tmp}/specify-verify.XXXXXX"); fi
logs=$work/logs
[ "$dry" = 1 ] || mkdir -p "$logs"
failed=()

# step NAME COMMAND...: run COMMAND (in a subshell from the repository root), log it, record the result.
step() {
  local name=$1; shift
  if [ "$dry" = 1 ]; then
    printf '  %-34s %s\n' "$name" "$*"
    return 0
  fi
  local log="$logs/$(echo "$name" | tr ' /' '__').log"
  printf '%-36s ' "$name"
  (cd "$root" && eval "$@") >"$log" 2>&1
  local code=$?
  printf 'exit %d\n' "$code"
  if [ "$code" != 0 ]; then
    failed+=("$name")
    tail -n 15 "$log" | sed 's/^/    | /'
  fi
}

[ "$dry" = 1 ] && echo "tools/verify.sh: the steps (nothing is run)" || {
  echo "tools/verify.sh at $(git rev-parse --short HEAD)$(git diff --quiet HEAD -- . ':!bundle' || echo ' (with uncommitted changes)')"
  echo "logs: $logs"
}

# --- ci.yml: test -------------------------------------------------------------------------------
step "ci test: step-diff comment"     "bash step-diff/tests/comment.sh"
step "ci test: fmt"                   "cargo fmt --all --check"
step "ci test: clippy"                "cargo clippy --workspace --all-targets --release -- -D warnings"
step "ci test: build tests"           "cargo test --workspace --release --locked --no-run"
step "ci test: full suite"            "cargo test --workspace --release --locked --no-fail-fast"

# --- ci.yml: wasm --------------------------------------------------------------------------------
base=$(git merge-base HEAD origin/main 2>/dev/null || git hash-object -t tree -w --stdin </dev/null)
step "ci wasm: tools unittests"       "python3 -m unittest discover -s tools -p 'test_*.py'"
step "ci wasm: wasm-pack"             "wasm-pack build crates/specify-core-wasm --target web --profile wasm-release --locked"
step "ci wasm: API snapshot"          "python3 tools/check_wasm_api.py --base-ref $base"
step "ci wasm: size"                  "python3 tools/wasm_size.py"
step "ci wasm: npm package packs"     "rm -rf '$work/npm-pkg' && cp -R crates/specify-core-wasm/pkg '$work/npm-pkg' && cp NOTICE '$work/npm-pkg/NOTICE' && python3 tools/npm_package.py '$work/npm-pkg' --check-pack"
step "ci wasm: parity reference"      "cargo build --locked --release -p specify-core-wasm --example parity_reference"
for script in $(sed -n 's/^ *run: node \(scripts\/wasm-[a-z-]*\.mjs\)$/\1/p' .github/workflows/ci.yml); do
  step "ci wasm: $(basename "$script" .mjs)" "node $script"
done

# --- ci.yml: ui (in ui/) -----------------------------------------------------------------------
step "ci ui: npm ci"                  "cd ui && npm ci"
step "ci ui: fetch:wasm"              "cd ui && npm run fetch:wasm"
step "ci ui: typecheck"               "cd ui && npm run typecheck"
step "ci ui: build"                   "cd ui && npm run build"
step "ci ui: build:app"               "cd ui && npm run build:app"
step "ci ui: npm test"                "cd ui && npm test"
step "ci ui: playwright chromium"     "cd ui && npx playwright install chromium"
step "ci ui: test:browser"            "cd ui && npm run test:browser"
step "ci ui: test:package"            "cd ui && npm run test:package"
step "ci ui: npm pack --dry-run"      "cd ui && npm pack --dry-run"

# --- ci.yml: app (the command built with this checkout's app and engine) ------------------------
cli=$root/target/release/specify-core
step "ci app: build specify-core"     "SPECIFY_REQUIRE_BUNDLE=1 cargo build --release --locked --bin specify-core"
step "ci app: open smoke"             "gunzip -c tests/fixtures/thread-depth-alpha7.step.gz > '$work/part.step' && python3 tools/open_smoke.py '$cli' '$work/part.step'"
step "ci app: app-browser, app-agent" "cd ui && SPECIFY_CORE='$cli' SPECIFY_APP_BUILT_IN=1 node test/app-browser.mjs && SPECIFY_CORE='$cli' SPECIFY_APP_BUILT_IN=1 node test/app-agent.mjs"

# --- bundle.yml: bundle/, which python.yml and crates.yml build from ------------------------------
step "bundle: lay out bundle/"        "rm -rf bundle && mkdir -p bundle/wasm && cp -R ui/dist-app bundle/app && cp crates/specify-core-wasm/pkg/specify_core_wasm.js crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm crates/specify-core-wasm/pkg/package.json bundle/wasm/"

# --- python.yml: wheel and test (this machine's platform) ----------------------------------------
venv=$work/venv
step "python: wheel"                  "SPECIFY_REQUIRE_BUNDLE=1 uvx maturin build --release --locked -m crates/specify-core-py/Cargo.toml -o '$work/dist'"
step "python: venv, wheel, pytest"    "uv venv -q '$venv' && uv pip install -q --python '$venv' pytest jsonschema && uv pip install -q --python '$venv' --no-index --no-cache --find-links '$work/dist' specify-core"
step "python: wheel carries licence"  "'$venv/bin/python' - '$work/dist' <<'EOF'
import glob, sys, zipfile
(wheel,) = glob.glob(sys.argv[1] + '/*.whl')
names = zipfile.ZipFile(wheel).namelist()
for name in ('LICENSE', 'NOTICE'):
    (found,) = [n for n in names if n.endswith('.dist-info/licenses/' + name)]
    assert len(zipfile.ZipFile(wheel).read(found)) > 100, found
EOF"
step "python: wheel's open smoke"     "'$venv/bin/python' tools/open_smoke.py '$venv/bin/specify-core' '$work/part.step'"
step "python: pytest"                 "'$venv/bin/python' -m pytest tests/python -q"

# --- crates.yml: package --------------------------------------------------------------------------
step "crates: package"                "bash tools/package_crate.sh"
step "crates: publish --dry-run"      "cargo publish --dry-run --locked --allow-dirty -p specify-core"

if [ "$dry" = 1 ]; then
  exit 0
fi
if [ ${#failed[@]} = 0 ]; then
  echo "VERIFY: PASS"
else
  ( IFS=,; echo "VERIFY: FAIL (${failed[*]})" )
  exit 1
fi
