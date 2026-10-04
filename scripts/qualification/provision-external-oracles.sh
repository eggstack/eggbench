#!/usr/bin/env bash
# Provision the pinned external load-generator oracles used by the
# qualification harnesses, and prove which bytes are on PATH.
#
# The harnesses treat an absent oracle as NOT-EXECUTED so a machine without
# the tool still produces evidence for everything else. That is the right
# default, but it quietly means the independent-transport corroboration is
# missing on exactly the machines nobody watches. An oracle is a security
# input, so it is installed from a pinned release asset and its digest is
# verified before it is trusted: a digest mismatch fails the run rather than
# substituting whatever `oha` happened to be on the image.
#
# Usage:
#   provision-external-oracles.sh oha
#   provision-external-oracles.sh oha h2load
#
# Environment:
#   ORACLE_BIN_DIR   install root (default: $PWD/.oracle-bin)
#   ORACLE_CACHE_DIR download cache (default: $ORACLE_BIN_DIR/cache)
#   GITHUB_PATH      when set, the install root is appended so later steps see it
#
# Every provisioned tool prints a `provisioned <tool> <version> sha256:<hex>`
# line. Harnesses copy that identity into their verdict text so the recorded
# evidence names the oracle that produced it rather than just "oha".

set -euo pipefail

# tool | release tag | asset name template | sha256 per linux arch
# Digests are the release-asset digests published by the GitHub release API.
# `<arch>` is substituted for the runner's `uname -m` (x86_64 | aarch64).
OHA_TAG="v1.16.0"
OHA_SHA256_x86_64="620bb9e16fb53eabc9a3fc45f88bdb41fefa3fee5c05e75892011ce320391716"
OHA_SHA256_aarch64=""

die() { echo "provision-external-oracles: $*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required but absent"; }
need curl
need sha256sum
need install

arch="$(uname -m)"
case "$arch" in
  x86_64 | amd64) asset_arch="amd64"; oha_digest="$OHA_SHA256_x86_64" ;;
  aarch64 | arm64) asset_arch="arm64"; oha_digest="$OHA_SHA256_aarch64" ;;
  *) die "no pinned oha digest for architecture $arch" ;;
esac

BIN_DIR="${ORACLE_BIN_DIR:-$PWD/.oracle-bin}"
CACHE_DIR="${ORACLE_CACHE_DIR:-$BIN_DIR/cache}"
mkdir -p "$BIN_DIR" "$CACHE_DIR"

[ "$#" -gt 0 ] || die "name at least one oracle to provision"

# fetch <url> <destination> — download once, reuse the cached copy after.
fetch() {
  local url="$1" destination="$2"
  if [ ! -s "$destination" ]; then
    echo "fetching $url"
    curl --fail --silent --show-error --location --retry 3 --retry-delay 2 \
      --proto '=https' --tlsv1.2 -o "$destination.part" "$url" \
      || die "download failed: $url"
    mv "$destination.part" "$destination"
  fi
}

# install_pinned <tool> <tag> <asset> <expected-sha256>
install_pinned() {
  local tool="$1" tag="$2" asset="$3" expected="$4"
  [ -n "$expected" ] || die "no pinned digest recorded for $tool/$asset"
  local url="https://github.com/hatoo/oha/releases/download/${tag}/${asset}"
  local cached="$CACHE_DIR/${tool}-${tag}-${asset}"
  fetch "$url" "$cached"
  local actual
  actual="$(sha256sum "$cached" | cut -d' ' -f1)"
  if [ "$actual" != "$expected" ]; then
    die "$tool $tag digest mismatch: expected $expected, got $actual"
  fi
  install -m 0755 "$cached" "$BIN_DIR/$tool"
  # Prove the installed bytes run, and compare the version the tool itself
  # claims against the pinned tag rather than trusting the download. Tools
  # print the bare version (`oha 1.16.0`) while tags carry a `v`, so compare
  # the normalized form and require the pinned number to appear as a whole
  # token so `1.1.0` cannot satisfy a `1.16.0` pin.
  local want="${tag#v}" reported
  reported="$("$BIN_DIR/$tool" --version 2>&1 | head -1 || true)"
  case " $reported " in
    *" $want "* | *" $want,"* | *",$want "* | *"/$want"*) ;;
    *) die "$tool reported '$reported' but $tag was pinned" ;;
  esac
  echo "provisioned $tool $tag sha256:$actual"
}

for tool in "$@"; do
  case "$tool" in
    oha) install_pinned oha "$OHA_TAG" "oha-linux-${asset_arch}" "$oha_digest" ;;
    *) die "no pinned provisioning rule for oracle '$tool'" ;;
  esac
done

if [ -n "${GITHUB_PATH:-}" ]; then
  echo "$BIN_DIR" >>"$GITHUB_PATH"
fi
echo "oracle bin dir: $BIN_DIR"
