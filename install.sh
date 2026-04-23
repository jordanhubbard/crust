#!/usr/bin/env sh
# Install crust — no Rust or Cargo required.
# Downloads a pre-built binary from GitHub releases.

set -e

REPO="jordanhubbard/crust"
BIN="crust"
RELEASES="https://github.com/${REPO}/releases/latest/download"

# ── detect OS ──────────────────────────────────────────────────────────────
OS=$(uname -s)
ARCH=$(uname -m)

case "${OS}" in
  Darwin)
    case "${ARCH}" in
      arm64)  TARGET="aarch64-apple-darwin"   ;;
      x86_64) TARGET="x86_64-apple-darwin"    ;;
      *)      echo "Unsupported Mac architecture: ${ARCH}"; exit 1 ;;
    esac
    ;;
  Linux)
    case "${ARCH}" in
      x86_64)          TARGET="x86_64-unknown-linux-musl"   ;;
      aarch64 | arm64) TARGET="aarch64-unknown-linux-musl"  ;;
      *)               echo "Unsupported Linux architecture: ${ARCH}"; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: ${OS}"
    echo "For Windows, download the .exe from https://github.com/${REPO}/releases"
    exit 1
    ;;
esac

ASSET="${BIN}-${TARGET}"
URL="${RELEASES}/${ASSET}"

# ── choose install dir ──────────────────────────────────────────────────────
if [ -w "/usr/local/bin" ]; then
  INSTALL_DIR="/usr/local/bin"
elif [ -d "${HOME}/.local/bin" ]; then
  INSTALL_DIR="${HOME}/.local/bin"
else
  INSTALL_DIR="${HOME}/.local/bin"
  mkdir -p "${INSTALL_DIR}"
fi

DEST="${INSTALL_DIR}/${BIN}"

# ── download ────────────────────────────────────────────────────────────────
echo "Detected: ${OS} / ${ARCH}"
echo "Downloading crust (${TARGET})..."

if command -v curl >/dev/null 2>&1; then
  curl -fSL --progress-bar "${URL}" -o "${DEST}"
elif command -v wget >/dev/null 2>&1; then
  wget -q --show-progress "${URL}" -O "${DEST}"
else
  echo "Error: neither curl nor wget found. Please install one and try again."
  exit 1
fi

chmod +x "${DEST}"

# ── verify ──────────────────────────────────────────────────────────────────
echo "Installed to ${DEST}"
"${DEST}" --version

# ── PATH hint ──────────────────────────────────────────────────────────────
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo ""
    echo "NOTE: ${INSTALL_DIR} is not in your PATH."
    echo "Add this to your shell profile:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac
