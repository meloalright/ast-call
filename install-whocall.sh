#!/bin/sh
set -e

REPO="meloalright/who-ast"
BIN="whocall"
INSTALL_DIR="/usr/local/bin"

get_target() {
  OS=$(uname -s)
  ARCH=$(uname -m)

  case "${OS}-${ARCH}" in
    Darwin-arm64)  echo "aarch64-apple-darwin" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin" ;;
    Linux-x86_64)  echo "x86_64-unknown-linux-gnu" ;;
    Linux-aarch64) echo "aarch64-unknown-linux-gnu" ;;
    *) echo "Unsupported platform: ${OS}-${ARCH}" >&2; exit 1 ;;
  esac
}

TARGET=$(get_target)
URL="https://github.com/${REPO}/releases/latest/download/who-${TARGET}.tar.gz"

echo "Installing ${BIN} (${TARGET})..."

TMP=$(mktemp -d)
curl -fsSL "${URL}" -o "${TMP}/who.tar.gz"
tar xzf "${TMP}/who.tar.gz" -C "${TMP}" "${BIN}"

if [ -w "${INSTALL_DIR}" ]; then
  mv "${TMP}/${BIN}" "${INSTALL_DIR}/${BIN}"
else
  sudo mv "${TMP}/${BIN}" "${INSTALL_DIR}/${BIN}"
fi

chmod +x "${INSTALL_DIR}/${BIN}"
rm -rf "${TMP}"

echo "${BIN} installed to ${INSTALL_DIR}/${BIN}"
