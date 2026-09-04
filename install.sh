#!/bin/sh
# local-tools installer — fetches prebuilt CLIs from GitHub release assets.
#
#   curl -fsSL https://raw.githubusercontent.com/alanrsoares/local-tools/main/install.sh | sh
#   wget -qO-  https://raw.githubusercontent.com/alanrsoares/local-tools/main/install.sh | sh
#
# Install a subset by passing tool names (needs `| sh -s --` when piped):
#   curl -fsSL .../install.sh | sh -s -- webdriver jwt
#
# Options (env vars):
#   GITHUB_TOKEN / GH_TOKEN   auth to lift anonymous GitHub API rate limits
#   LOCAL_TOOLS_VERSION       install a specific tag (e.g. v0.1.0); default: latest
#   LOCAL_TOOLS_BIN_DIR       install destination (default ~/.local/bin)
#   LOCAL_TOOLS_API_URL       override the release endpoint (testing/mirrors)
set -eu

REPO="alanrsoares/local-tools"
ALL_TOOLS="scaffold portkill jwt devclean fanout webdriver"
BIN_DIR="${LOCAL_TOOLS_BIN_DIR:-$HOME/.local/bin}"
TOKEN="${GITHUB_TOKEN:-${GH_TOKEN:-}}"

say() { printf '%s\n' "$*"; }
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

# --- tool selection ----------------------------------------------------------
if [ "$#" -gt 0 ]; then
  TOOLS="$*"
  for tool in $TOOLS; do
    case " $ALL_TOOLS " in
      *" $tool "*) ;;
      *) fail "unknown tool: $tool (available: $ALL_TOOLS)" ;;
    esac
  done
else
  TOOLS="$ALL_TOOLS"
fi

# --- downloader (curl or wget) -----------------------------------------------
if command -v curl >/dev/null 2>&1; then
  HTTP=curl
elif command -v wget >/dev/null 2>&1; then
  HTTP=wget
else
  fail "need curl or wget"
fi

# fetch <url>: print body. fetch_to <ref> <out>: download an asset — with a
# token the ref is an API asset URL and needs the octet-stream Accept.
fetch() {
  if [ "$HTTP" = curl ]; then
    if [ -n "$TOKEN" ]; then
      curl -fsSL -H "Authorization: Bearer $TOKEN" "$1"
    else
      curl -fsSL "$1"
    fi
  else
    if [ -n "$TOKEN" ]; then
      wget -qO- --header="Authorization: Bearer $TOKEN" "$1"
    else
      wget -qO- "$1"
    fi
  fi
}

fetch_to() {
  if [ "$HTTP" = curl ]; then
    if [ -n "$TOKEN" ]; then
      curl -fsSL -H "Authorization: Bearer $TOKEN" -H "Accept: application/octet-stream" -o "$2" "$1"
    else
      curl -fsSL -o "$2" "$1"
    fi
  else
    if [ -n "$TOKEN" ]; then
      wget -qO "$2" --header="Authorization: Bearer $TOKEN" --header="Accept: application/octet-stream" "$1"
    else
      wget -qO "$2" "$1"
    fi
  fi
}

# --- platform detection ------------------------------------------------------
case "$(uname -s)" in
  Darwin) OS="macos" ;;
  Linux) OS="linux" ;;
  *) fail "unsupported OS: $(uname -s) (local-tools ships macOS and Linux builds)" ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) ARCH="x64" ;;
  arm64 | aarch64) ARCH="arm64" ;;
  *) fail "unsupported architecture: $(uname -m)" ;;
esac

ASSET="local-tools-${OS}-${ARCH}.tar.gz"

# --- release lookup ----------------------------------------------------------
# LOCAL_TOOLS_API_URL overrides the release endpoint (testing/mirrors).
if [ -n "${LOCAL_TOOLS_API_URL:-}" ]; then
  API_URL="$LOCAL_TOOLS_API_URL"
elif [ -n "${LOCAL_TOOLS_VERSION:-}" ]; then
  API_URL="https://api.github.com/repos/${REPO}/releases/tags/${LOCAL_TOOLS_VERSION}"
else
  API_URL="https://api.github.com/repos/${REPO}/releases/latest"
fi

say "» platform: ${OS}-${ARCH}"
say "» looking up release (${LOCAL_TOOLS_VERSION:-latest})…"
RELEASE_JSON=$(fetch "$API_URL") ||
  fail "no release found. If you hit the anonymous GitHub API rate limit, pass a token:
       GITHUB_TOKEN=\$(gh auth token) sh install.sh
       Also note: draft releases are invisible — publish one first."

# One "<name>\t<download-ref>" line per asset. With a token, pair each asset's
# API url with its name (browser URLs 404 on private repos); without, derive
# the name from the public browser URL.
list_assets() {
  if [ -n "$TOKEN" ]; then
    printf '%s\n' "$RELEASE_JSON" |
      tr ',' '\n' |
      grep -oE '"(url|name)" *: *"[^"]*"' |
      awk -F'"' '
        $2 == "url" && $4 ~ /\/releases\/assets\// { u = $4; next }
        $2 == "name" && u != "" { print $4 "\t" u; u = "" }
      '
  else
    printf '%s\n' "$RELEASE_JSON" |
      grep -oE '"browser_download_url" *: *"http[^"]*"' |
      sed 's/.*"\(http[^"]*\)"/\1/' |
      awk -F/ '{ print $NF "\t" $0 }'
  fi
}

ASSETS=$(list_assets)

# pick_asset <exact-name>: print the download ref, empty when absent.
pick_asset() {
  printf '%s\n' "$ASSETS" | awk -F'\t' -v name="$1" '$1 == name { print $2; exit }'
}

TARBALL_REF=$(pick_asset "$ASSET")
[ -n "$TARBALL_REF" ] || fail "this release has no ${OS}-${ARCH} build ($ASSET).
       CI builds macos-arm64, macos-x64, linux-x64 and linux-arm64."

TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/local-tools-install.XXXXXX")
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

say "» downloading ${ASSET}…"
fetch_to "$TARBALL_REF" "$TMP_DIR/$ASSET"

# --- checksum (best effort: skipped when the sidecar or a hasher is absent) ---
SUM_REF=$(pick_asset "${ASSET}.sha256")
if [ -n "$SUM_REF" ]; then
  if command -v shasum >/dev/null 2>&1; then
    HASHER="shasum -a 256"
  elif command -v sha256sum >/dev/null 2>&1; then
    HASHER="sha256sum"
  else
    HASHER=""
  fi
  if [ -n "$HASHER" ]; then
    fetch_to "$SUM_REF" "$TMP_DIR/$ASSET.sha256"
    expected=$(awk '{ print $1; exit }' "$TMP_DIR/$ASSET.sha256")
    actual=$($HASHER "$TMP_DIR/$ASSET" | awk '{ print $1; exit }')
    [ "$expected" = "$actual" ] ||
      fail "checksum mismatch for $ASSET
       expected $expected
       actual   $actual"
    say "» checksum ok"
  fi
fi

# --- install -----------------------------------------------------------------
tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"
mkdir -p "$BIN_DIR"

for tool in $TOOLS; do
  [ -f "$TMP_DIR/$tool" ] || fail "$tool is missing from $ASSET"
  install -m 755 "$TMP_DIR/$tool" "$BIN_DIR/$tool" 2>/dev/null || {
    cp "$TMP_DIR/$tool" "$BIN_DIR/$tool"
    chmod 755 "$BIN_DIR/$tool"
  }
  # Release binaries are ad-hoc signed by the linker but unnotarized; clear
  # quarantine so Gatekeeper doesn't block them on macOS.
  if [ "$OS" = macos ] && command -v xattr >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$BIN_DIR/$tool" 2>/dev/null || true
  fi
  say "✓ $BIN_DIR/$tool"
done

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say "note: $BIN_DIR is not on your PATH — add:
       export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac
