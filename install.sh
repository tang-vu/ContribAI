#!/bin/bash
set -e

VERSION="v6.5.0"
REPO="tang-vu/ContribAI"
INSTALL_DIR="/usr/local/bin"

# Detect OS and arch
OS=$(uname -s | tr "[:upper:]" "[:lower:]")
ARCH=$(uname -m)

case "$OS" in
  linux)  BINARY="contribai-linux-x86_64" ;;
  darwin)
    case "$ARCH" in
      arm64|aarch64) BINARY="contribai-macos-aarch64" ;;
      *)             BINARY="contribai-macos-x86_64" ;;
    esac ;;
  *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

URL="https://github.com/$REPO/releases/download/$VERSION/$BINARY"

echo "Installing ContribAI $VERSION..."
echo "  OS: $OS | Arch: $ARCH"
echo "  Binary: $BINARY"
echo "  Downloading from: $URL"
echo ""

curl -fsSL "$URL" -o contribai
chmod +x contribai

if [ -w "$INSTALL_DIR" ]; then
  mv contribai "$INSTALL_DIR/contribai"
else
  echo "Need sudo to install to $INSTALL_DIR"
  sudo mv contribai "$INSTALL_DIR/contribai"
fi

echo ""
echo "ContribAI installed successfully!"
echo "Run: contribai init"
