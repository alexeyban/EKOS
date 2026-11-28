#!/usr/bin/env bash
# RFC 0269: the Python package, checked the way a user gets it.
#
# Builds the wheel, installs it in a fresh environment, compiles a two-table workspace with the
# `ekos` binary, then runs ruff, mypy --strict, pytest and the example scripts against the
# *installed* package. Writes
# python/coverage.xml (paths mapped back to python/ekos) for SonarCloud.
#
#   EKOS_BIN       the ekos binary that compiles the fixture (default: ekos/target/release/ekos,
#                  then ekos/target/debug/ekos)
#   UV_PYTHON      the interpreter (default 3.10, the package's floor)
#   MATURIN_ARGS   extra maturin arguments (default: --profile dev)
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export UV_PYTHON="${UV_PYTHON:-3.10}"
maturin_args="${MATURIN_ARGS:---profile dev}"

ekos_bin="${EKOS_BIN:-}"
if [[ -z "$ekos_bin" ]]; then
  for candidate in "$repo/ekos/target/release/ekos" "$repo/ekos/target/debug/ekos"; do
    if [[ -x "$candidate" ]]; then
      ekos_bin="$candidate"
      break
    fi
  done
fi
if [[ -z "$ekos_bin" ]]; then
  echo "no ekos binary: build one (cargo build -p ekos) or set EKOS_BIN" >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "── wheel"
cd "$repo/python"
# shellcheck disable=SC2086  # maturin_args is a list of flags
uvx maturin@1.16.0 build $maturin_args --locked -o "$work/dist"

echo "── fresh environment"
uv venv -q "$work/venv"
py="$work/venv/bin/python"
if [[ ! -x "$py" ]]; then
  py="$work/venv/Scripts/python.exe"
fi
uv pip install -q --python "$py" "$work"/dist/*.whl \
  pytest==8.4.2 coverage==7.11.3 mypy==1.18.2 ruff==0.14.6

echo "── fixture workspace"
fixture="$work/workspace"
mkdir -p "$fixture"
cat > "$fixture/schema.sql" <<'SQL'
CREATE TABLE customers (id INT PRIMARY KEY, name TEXT);
CREATE TABLE orders (
  id INT PRIMARY KEY,
  customer_id INT REFERENCES customers(id),
  status TEXT
);
SQL
(
  cd "$fixture"
  for stage in init build recover resolve compile commit; do
    "$ekos_bin" "$stage" > "$work/$stage.log" 2>&1 || {
      echo "ekos $stage failed:" >&2
      cat "$work/$stage.log" >&2
      exit 1
    }
  done
)

echo "── ruff, mypy"
"$py" -m ruff check .
"$py" -m ruff format --check .
"$py" -m mypy --strict ekos tests examples

echo "── pytest (installed wheel)"
# Run from outside the source tree: with python/ as the working directory `import ekos` would
# find the sources, which have no native module, instead of the installed wheel.
export EKOS_PY_TEST_WORKSPACE="$fixture"
export COVERAGE_FILE="$work/.coverage"
rc="$repo/python/pyproject.toml"
(
  cd "$work"
  "$py" -m coverage run --rcfile="$rc" -p --source=ekos \
    -m pytest -v -c "$rc" --rootdir="$repo/python" "$repo/python/tests"
)
echo "── examples (installed wheel)"
# Under coverage too, so the report covers the scripts people copy from.
example() {
  local script="$1" expect="$2"
  shift 2
  "$py" -m coverage run --rcfile="$rc" -p "$repo/python/examples/$script" "$@" | grep -q -- "$expect"
}
(
  cd "$work"
  example quickstart.py "schema.sql" "$fixture" orders
  example quickstart.py "nothing matches" "$fixture" zzzznothing
  example impact_report.py "via ForeignKey" "$fixture" customers 3
  example impact_report.py "itself depends on" "$fixture" orders
  example schema_overview.py "<- orders" "$fixture"
  example grounded_context.py "^\[1\]" "$fixture" "which tables reference customers?"
)

cd "$repo"
"$py" -m coverage combine --rcfile="$rc" -q
"$py" -m coverage xml --rcfile="$rc" -q -o python/coverage.xml
"$py" -m coverage report --rcfile="$rc"
