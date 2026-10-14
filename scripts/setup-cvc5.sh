#!/usr/bin/env bash
# Download the solver 1.4.0 static prebuilt and print export commands.
# CI uses this. Leaving CVC5_LIB_DIR unset builds tag cvc5-1.4.0 from source.
#
#   bash scripts/setup-cvc5.sh
#   bash scripts/setup-cvc5.sh --github-env   # KEY=VALUE lines for $GITHUB_ENV
set -euo pipefail

GITHUB_ENV_MODE=0
if [ "${1:-}" = "--github-env" ]; then
  GITHUB_ENV_MODE=1
elif [ -n "${1:-}" ]; then
  echo "usage: scripts/setup-cvc5.sh [--github-env]" >&2
  exit 2
fi

INSTALL_DIR="${CVC5_INSTALL_DIR:-/tmp/cvc5-install}"

if [ "$(uname)" = "Darwin" ]; then
  ARCH=$(uname -m)
  ZIP_NAME="cvc5-macOS-${ARCH}-static.zip"
  DIR_NAME="cvc5-macOS-${ARCH}-static"
else
  ZIP_NAME="cvc5-Linux-x86_64-static.zip"
  DIR_NAME="cvc5-Linux-x86_64-static"
fi

# Prebuilt for the cvc5 0.6.1 crate (cvc5-sys 0.6.1 expects solver 1.4.0).
# 1.4.0 applies -Wno-error=deprecated-literal-operator for Apple Clang.
# Do not point these bindings at a 1.3.x library.
# Without CVC5_LIB_DIR, cvc5-sys clones tag cvc5-1.4.0 and builds it.
# Do not use /releases/latest.
# Override with CVC5_RELEASE_TAG if you need a different prebuilt.
CVC5_RELEASE_TAG="${CVC5_RELEASE_TAG:-cvc5-1.4.0}"
URL="https://github.com/cvc5/cvc5/releases/download/${CVC5_RELEASE_TAG}/${ZIP_NAME}"
TMP_ZIP="$(mktemp -t cvc5.XXXXXX.zip)"

cleanup() {
  rm -f "$TMP_ZIP"
}
trap cleanup EXIT

echo "Downloading ${URL} ..." >&2
curl -fsSL "$URL" -o "$TMP_ZIP"
mkdir -p "$INSTALL_DIR"
unzip -o -q "$TMP_ZIP" -d "$INSTALL_DIR"

LIB_DIR="${INSTALL_DIR}/${DIR_NAME}/lib"
INC_DIR="${INSTALL_DIR}/${DIR_NAME}/include"
if [ ! -d "$LIB_DIR" ] || [ ! -d "$INC_DIR" ]; then
  echo "cvc5 prebuilt missing lib or include under ${INSTALL_DIR}/${DIR_NAME}" >&2
  exit 1
fi

if [ "$GITHUB_ENV_MODE" -eq 1 ]; then
  printf 'CVC5_LIB_DIR=%s\n' "$LIB_DIR"
  printf 'CVC5_INCLUDE_DIR=%s\n' "$INC_DIR"
  exit 0
fi

cat <<EOF
CVC5 prebuilt installed under ${INSTALL_DIR}/${DIR_NAME}

Add to your shell (or paste before cargo test):

  export CVC5_LIB_DIR=${LIB_DIR}
  export CVC5_INCLUDE_DIR=${INC_DIR}

Then verify:

  cargo clippy -p assura-smt --features cvc5-verify -- -D warnings
  cargo test -p assura-smt --features cvc5-verify -- cvc5_
EOF