#!/usr/bin/env bash
# Download prebuilt static CVC5 and print export commands for cvc5-sys.
# Use when local `cargo test --features cvc5-verify` fails building from source.
set -euo pipefail

INSTALL_DIR="${CVC5_INSTALL_DIR:-/tmp/cvc5-install}"

if [ "$(uname)" = "Darwin" ]; then
  ARCH=$(uname -m)
  ZIP_NAME="cvc5-macOS-${ARCH}-static.zip"
  DIR_NAME="cvc5-macOS-${ARCH}-static"
else
  ZIP_NAME="cvc5-Linux-x86_64-static.zip"
  DIR_NAME="cvc5-Linux-x86_64-static"
fi

URL="https://github.com/cvc5/cvc5/releases/latest/download/${ZIP_NAME}"
TMP_ZIP="$(mktemp -t cvc5.XXXXXX.zip)"

cleanup() {
  rm -f "$TMP_ZIP"
}
trap cleanup EXIT

echo "Downloading ${URL} ..."
curl -fsSL "$URL" -o "$TMP_ZIP"
mkdir -p "$INSTALL_DIR"
unzip -o -q "$TMP_ZIP" -d "$INSTALL_DIR"

LIB_DIR="${INSTALL_DIR}/${DIR_NAME}/lib"
INC_DIR="${INSTALL_DIR}/${DIR_NAME}/include"

cat <<EOF
CVC5 prebuilt installed under ${INSTALL_DIR}/${DIR_NAME}

Add to your shell (or paste before cargo test):

  export CVC5_LIB_DIR=${LIB_DIR}
  export CVC5_INCLUDE_DIR=${INC_DIR}

Then verify:

  cargo clippy -p assura-smt --features cvc5-verify -- -D warnings
  cargo test -p assura-smt --features cvc5-verify -- cvc5_
EOF