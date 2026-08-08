#!/bin/bash
set -euo pipefail

VERSION="v6.8.0"
REPO="tang-vu/ContribAI"
INSTALL_DIR="/usr/local/bin"

# Detect OS and arch
OS=$(uname -s | tr "[:upper:]" "[:lower:]")
ARCH=$(uname -m)

case "$OS" in
  linux)
    case "$ARCH" in
      x86_64|amd64)
        BINARY="contribai-$VERSION-linux-x86_64"
        EXPECTED_SHA256="23fad535c931211bab67e3675bd5f67df4603f13f376984e6477a63e3f59ea43"
        ;;
      *) echo "Unsupported Linux architecture: $ARCH"; exit 1 ;;
    esac ;;
  darwin)
    case "$ARCH" in
      arm64|aarch64)
        BINARY="contribai-$VERSION-macos-aarch64"
        EXPECTED_SHA256="bd20e72b945a4f1d59d7d7305b2ce2e8d17f2199d666bd8bfbe649418431e737"
        ;;
      x86_64|amd64)
        BINARY="contribai-$VERSION-macos-x86_64"
        EXPECTED_SHA256="bfd5a92d6492f75b80164402d0acca52af395d61c246b31f4f6905a4ee5e928e"
        ;;
      *) echo "Unsupported macOS architecture: $ARCH"; exit 1 ;;
    esac ;;
  *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

URL="https://github.com/$REPO/releases/download/$VERSION/$BINARY"

echo "Installing ContribAI $VERSION..."
echo "  OS: $OS | Arch: $ARCH"
echo "  Binary: $BINARY"
echo "  Downloading from: $URL"
echo ""

TEMP_FILE=$(mktemp "${TMPDIR:-/tmp}/contribai.XXXXXX")
trap 'rm -f "$TEMP_FILE"' EXIT

curl -fsSL "$URL" -o "$TEMP_FILE"

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA256=$(sha256sum "$TEMP_FILE" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_SHA256=$(shasum -a 256 "$TEMP_FILE" | awk '{print $1}')
else
  echo "Cannot verify download: sha256sum or shasum is required." >&2
  exit 1
fi

if [ "$ACTUAL_SHA256" != "$EXPECTED_SHA256" ]; then
  echo "Checksum verification failed; refusing to install." >&2
  echo "  Expected: $EXPECTED_SHA256" >&2
  echo "  Actual:   $ACTUAL_SHA256" >&2
  exit 1
fi

echo "  SHA256 checksum verified."
chmod +x "$TEMP_FILE"

if [ -w "$INSTALL_DIR" ]; then
  mv "$TEMP_FILE" "$INSTALL_DIR/contribai"
else
  echo "Need sudo to install to $INSTALL_DIR"
  sudo mv "$TEMP_FILE" "$INSTALL_DIR/contribai"
fi

echo ""
echo "ContribAI installed successfully!"
echo "Run: contribai init"
