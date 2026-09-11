#!/usr/bin/env bash
# Assura verify composite helper. Inputs come from the environment so
# GitHub Actions never splices caller-controlled values into script text.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: verify.sh --step run|install
  run      Verify files matching FILES (default **/*.assura)
  install  Install assura according to VERSION (default latest)
EOF
}

STEP=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --step)
      STEP="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "FAIL: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$STEP" ]]; then
  usage >&2
  exit 2
fi

github_output() {
  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    printf '%s\n' "$1" >>"$GITHUB_OUTPUT"
  fi
}

step_install() {
  echo "PLAN: install assura for VERSION=${VERSION:-latest}"
  local want="${VERSION:-latest}"
  local have=""
  if command -v assura >/dev/null 2>&1; then
    have="$(assura --version 2>/dev/null || true)"
    echo "DO: found on PATH: ${have:-unknown}"
  fi

  local need_install=1
  if [[ -n "$have" && "$want" == "latest" ]]; then
    need_install=0
    echo "OK: reusing existing assura (version=latest)"
  elif [[ -n "$have" && "$have" == *"$want"* ]]; then
    need_install=0
    echo "OK: on-PATH assura matches $want"
  fi

  if [[ "$need_install" -eq 1 ]]; then
    echo "DO: cargo install assura version=$want"
    if [[ "$want" == "latest" ]]; then
      cargo install --locked assura
    else
      cargo install --locked assura --version "$want"
    fi
  fi
  assura --version
  echo "DONE: assura install step"
}

step_run() {
  echo "PLAN: verify Assura contracts"
  local files_glob="${FILES:-**/*.assura}"
  local layer="${LAYER:-1}"
  local json_output="${JSON_OUTPUT:-false}"
  local fail_on_warning="${FAIL_ON_WARNING:-false}"
  local allow_empty="${ALLOW_EMPTY:-false}"

  shopt -s globstar nullglob

  local -a matches=()
  # Deliberate glob expansion: FILES is a glob pattern, not a single path.
  # shellcheck disable=SC2086
  for file in $files_glob; do
    if [[ -f "$file" ]]; then
      matches+=("$file")
    fi
  done

  local total="${#matches[@]}"
  local verified=0
  local errors=0
  local results="["
  local first=1

  echo "DO: matched $total file(s) for '$files_glob'"

  if [[ "$total" -eq 0 ]]; then
    if [[ "$allow_empty" == "true" ]]; then
      echo "OK: no files matched; allow-empty=true"
    else
      echo "FAIL: No .assura files matched '$files_glob'"
      echo "::error::No .assura files matched '${files_glob}'"
      github_output "total-contracts=0"
      github_output "verified=0"
      github_output "errors=1"
      {
        echo "result<<__ASSURA_EOF__"
        echo "[]"
        echo "__ASSURA_EOF__"
      } >>"${GITHUB_OUTPUT:-/dev/null}"
      exit 1
    fi
  fi

  local json_flag=()
  if [[ "$json_output" == "true" ]]; then
    json_flag=(--json)
  fi

  local file out rc is_warning
  for file in "${matches[@]}"; do
    echo "DO: assura check --layer $layer $file"
    rc=0
    out="$(assura check --layer "$layer" "${json_flag[@]}" "$file" 2>&1)" || rc=$?
    printf '%s\n' "$out"
    is_warning=0
    if printf '%s\n' "$out" | grep -Eq '"severity"[[:space:]]*:[[:space:]]*"warning"|Warning:'; then
      is_warning=1
    fi
    if [[ "$rc" -eq 0 && "$fail_on_warning" == "true" && "$is_warning" -eq 1 ]]; then
      echo "FAIL: warning treated as error (fail-on-warning=true) for $file"
      rc=1
    fi
    if [[ "$rc" -eq 0 ]]; then
      verified=$((verified + 1))
      echo "OK: $file"
    else
      errors=$((errors + 1))
      echo "::error file=${file}::Verification failed"
      echo "FAIL: $file"
    fi
    if [[ "$first" -eq 1 ]]; then
      first=0
    else
      results+=","
    fi
    results+="$(printf '{"file":%s,"ok":%s}' \
      "$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$file")" \
      "$([[ "$rc" -eq 0 ]] && echo true || echo false)")"
  done
  results+="]"

  github_output "total-contracts=$total"
  github_output "verified=$verified"
  github_output "errors=$errors"
  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    {
      echo "result<<__ASSURA_EOF__"
      echo "$results"
      echo "__ASSURA_EOF__"
    } >>"$GITHUB_OUTPUT"
  fi

  if [[ "$errors" -gt 0 ]]; then
    echo "FAIL: $errors contract(s) failed verification"
    echo "::error::$errors contract(s) failed verification"
    echo "DONE: total=$total verified=$verified errors=$errors"
    exit 1
  fi
  echo "DONE: All $verified contract(s) verified successfully"
}

case "$STEP" in
  run) step_run ;;
  install) step_install ;;
  *)
    echo "FAIL: unknown --step $STEP" >&2
    usage >&2
    exit 2
    ;;
esac
