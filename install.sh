#!/bin/sh
# Installs codemycel from the latest GitHub release into ~/.local/bin, then runs
# `codemycel setup` with its answers read from the terminal.
#   curl -fsSL https://raw.githubusercontent.com/msaeedsakib/codemycel/main/install.sh | sh
# CODEMYCEL_INSTALL_BASE_URL replaces the release download URL.
set -eu

base="${CODEMYCEL_INSTALL_BASE_URL:-https://github.com/msaeedsakib/codemycel/releases/latest/download}"
dir="$HOME/.local/bin"

case "$(uname -s)" in
    Linux) os=unknown-linux-musl ;;
    Darwin) os=apple-darwin ;;
    *)
        echo "codemycel: unsupported system $(uname -s); on Windows use install.ps1" >&2
        exit 1
        ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *)
        echo "codemycel: unsupported architecture $(uname -m)" >&2
        exit 1
        ;;
esac
if [ "$os" = apple-darwin ] && [ "$arch" = x86_64 ] &&
    [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
    arch=aarch64
fi
asset="codemycel-$arch-$os.tar.gz"

download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$2" "$1"
    else
        echo "codemycel: curl or wget is needed" >&2
        exit 1
    fi
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "downloading $asset"
download "$base/$asset" "$tmp/$asset"
download "$base/$asset.sha256" "$tmp/$asset.sha256"
expected=$(cut -d ' ' -f 1 <"$tmp/$asset.sha256")
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp/$asset" | cut -d ' ' -f 1)
else
    actual=$(shasum -a 256 "$tmp/$asset" | cut -d ' ' -f 1)
fi
if [ "$expected" != "$actual" ]; then
    echo "codemycel: checksum mismatch for $asset" >&2
    exit 1
fi

mkdir "$tmp/unpacked"
tar -xzf "$tmp/$asset" -C "$tmp/unpacked"
binary=$(find "$tmp/unpacked" -type f -name codemycel | head -n 1)
if [ -z "$binary" ]; then
    echo "codemycel: $asset holds no codemycel binary" >&2
    exit 1
fi
mkdir -p "$dir"
cp "$binary" "$dir/codemycel.tmp"
chmod 755 "$dir/codemycel.tmp"
mv "$dir/codemycel.tmp" "$dir/codemycel"
echo "installed $dir/codemycel"

case ":$PATH:" in
    *":$dir:"*) ;;
    *)
        echo "warning: $dir is not on your PATH. Add this line to your shell profile:"
        echo "  export PATH=\"$dir:\$PATH\""
        ;;
esac

if (exec </dev/tty) 2>/dev/null; then
    "$dir/codemycel" setup </dev/tty || echo "run codemycel setup to configure your agents"
else
    echo "run codemycel setup to configure your agents"
fi
