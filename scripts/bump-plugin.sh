#!/usr/bin/env bash
# Points the plugin at a published release: sets its version, the version
# the launcher downloads, and the pinned checksums (docs/releasing.md).
#
#   scripts/bump-plugin.sh 1.0.1
#
# The checksums come from the release's SHA256SUMS, so run this after the
# release is published. It then runs the launcher once, which downloads
# this platform's binary and checks it against the new checksums.
set -euo pipefail

version="${1:?usage: scripts/bump-plugin.sh VERSION (such as 1.0.1)}"
version="${version#v}"
repo="erwinvaneijk/whatsapp-mcp"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sums="$root/plugin/scripts/SHA256SUMS"

new="$(mktemp)"
trap 'rm -f "$new"' EXIT
# The public URL, not gh: it only works once the release is published.
curl --fail --silent --show-error --location --output "$new" \
  "https://github.com/$repo/releases/download/v$version/SHA256SUMS" || {
  echo "no SHA256SUMS for v$version: is the release published (not a draft)?" >&2
  exit 1
}

# The same five files as before, each with a sha256.
if ! diff <(awk '{ print $2 }' "$sums" | sort) <(awk '{ print $2 }' "$new" | sort) >/dev/null; then
  echo "v$version's SHA256SUMS lists other files than plugin/scripts/SHA256SUMS:" >&2
  cat "$new" >&2
  exit 1
fi
if awk 'length($1) != 64 || $1 !~ /^[0-9a-f]+$/ { bad = 1 } END { exit !bad }' "$new"; then
  echo "v$version's SHA256SUMS has a line that is not a sha256:" >&2
  cat "$new" >&2
  exit 1
fi

sort -k2 "$new" >"$sums"
sed -i.bak "s/^VERSION=\".*\"$/VERSION=\"$version\"/" "$root/plugin/scripts/launch.sh"
sed -i.bak "s/^  \"version\": \".*\",$/  \"version\": \"$version\",/" "$root/plugin/.claude-plugin/plugin.json"
sed -i.bak "s#releases/tag/v[0-9][0-9.]*)#releases/tag/v$version)#" "$root/plugin/README.md"
rm -f "$root"/plugin/scripts/launch.sh.bak "$root"/plugin/.claude-plugin/plugin.json.bak "$root"/plugin/README.md.bak

data="$(mktemp -d)"
trap 'rm -f "$new"; rm -rf "$data"' EXIT
got="$(CLAUDE_PLUGIN_DATA="$data" bash "$root/plugin/scripts/launch.sh" --version)"
if [ "$got" != "whatsapp-mcp $version" ]; then
  echo "the launcher ran '$got', expected 'whatsapp-mcp $version'" >&2
  exit 1
fi
echo "plugin now at $version; the launcher downloaded and checked $got" >&2
git -C "$root" diff --stat -- plugin
