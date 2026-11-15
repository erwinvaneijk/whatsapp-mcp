#!/usr/bin/env bash
# Starts whatsapp-mcp for the plugin: downloads the release binary this
# plugin version pins, checks it against SHA256SUMS, and runs it with the
# arguments given (such as `serve`).
#
# The binary is kept in ${CLAUDE_PLUGIN_DATA}/bin, which survives plugin
# updates; the bridge service can run the same file. stdout belongs to the
# MCP protocol, so every message here goes to stderr.
#
# macOS and Linux, and Windows through the Git Bash that Claude Code uses.
set -euo pipefail

VERSION="1.0.0"
REPO="erwinvaneijk/whatsapp-mcp"
# For tests: where the release files are, instead of the GitHub release.
BASE_URL="${WHATSAPP_MCP_DOWNLOAD_URL:-https://github.com/$REPO/releases/download/v$VERSION}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

fail() {
  echo "whatsapp-mcp plugin: $*" >&2
  exit 1
}

case "$(uname -s)" in
  Darwin) os="apple-darwin"; exe="" ;;
  Linux) os="unknown-linux-gnu"; exe="" ;;
  MINGW* | MSYS* | CYGWIN*) os="pc-windows-msvc"; exe=".exe" ;;
  *) fail "no release binary for $(uname -s)" ;;
esac
case "$(uname -m)" in
  arm64 | aarch64) arch="aarch64" ;;
  x86_64 | amd64) arch="x86_64" ;;
  *) fail "no release binary for $(uname -m)" ;;
esac
asset="whatsapp-mcp-$arch-$os$exe"

want="$(awk -v a="$asset" '$2 == a { print $1 }' "$here/SHA256SUMS")"
[ -n "$want" ] || fail "no release binary for $arch-$os (not in SHA256SUMS)"

data="${CLAUDE_PLUGIN_DATA:?CLAUDE_PLUGIN_DATA is not set; run this through Claude Code}"
bin="$data/bin/whatsapp-mcp$exe"

# From stdin: with a file name, Git Bash's sha256sum escapes a path with
# backslashes and puts a \ before the hash.
sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum <"$1" | awk '{ print $1 }'
  else
    shasum -a 256 <"$1" | awk '{ print $1 }'
  fi
}

# The pinned version, checked by its hash: a binary that only claims the
# right --version is not enough.
if [ ! -f "$bin" ] || [ "$(sha256 "$bin")" != "$want" ]; then
  mkdir -p "$data/bin"
  tmp="$(mktemp "$data/bin/.download.XXXXXX")"
  trap 'rm -f "$tmp"' EXIT
  echo "whatsapp-mcp plugin: downloading $asset $VERSION" >&2
  curl --fail --silent --show-error --location --output "$tmp" "$BASE_URL/$asset" ||
    fail "download of $BASE_URL/$asset failed"
  got="$(sha256 "$tmp")"
  [ "$got" = "$want" ] || fail "checksum mismatch for $asset: expected $want, got $got; not running it"
  chmod 755 "$tmp"
  # A new file in place of the old one: a bridge running the old binary keeps
  # running it until it restarts.
  mv -f "$tmp" "$bin"
  trap - EXIT
fi

# A configuration file given as ~/... in the plugin's settings.
config="${WHATSAPP_MCP_CONFIG:-}"
if [ "$config" != "${config#\~/}" ]; then
  export WHATSAPP_MCP_CONFIG="$HOME/${config#\~/}"
fi

exec "$bin" "$@"
