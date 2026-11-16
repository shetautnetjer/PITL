#!/usr/bin/env bash
# Put this machine on your Tailscale network without sudo.
#
# Downloads Tailscale's official static build, verifies its published SHA-256,
# installs it under ~/.local, starts tailscaled as the current user in
# userspace-networking mode, and runs `tailscale up`, which prints a login link
# to approve from your phone.
#
# Linux only. Contains no keys or secrets.
#
# Environment overrides:
#   TS_VERSION   version to install (default: latest stable)
#   TS_ARCH      amd64 (default), arm64, 386 or arm

set -euo pipefail

BASE_URL="https://pkgs.tailscale.com/stable"
ARCH="${TS_ARCH:-amd64}"
OPT_DIR="$HOME/.local/opt/tailscale"
BIN_DIR="$HOME/.local/bin"
STATE_DIR="$HOME/.local/share/tailscale"
SOCKET="$STATE_DIR/tailscaled.sock"
STATE="$STATE_DIR/tailscaled.state"
LOG="$STATE_DIR/tailscaled.log"

die() { echo "error: $*" >&2; exit 1; }

[ "$(uname -s)" = "Linux" ] || die "this script supports Linux only"
for cmd in curl tar sha256sum; do
  command -v "$cmd" >/dev/null 2>&1 || die "missing required command: $cmd"
done

# 1. Find the version.
if [ -n "${TS_VERSION:-}" ]; then
  VERSION="$TS_VERSION"
else
  VERSION="$(curl -fsSL "$BASE_URL/" \
    | grep -oE "tailscale_[0-9]+\.[0-9]+\.[0-9]+_${ARCH}\.tgz" \
    | sed -E "s/tailscale_(.*)_${ARCH}\.tgz/\1/" \
    | sort -uV | tail -n 1)"
  [ -n "$VERSION" ] || die "could not find the latest version on $BASE_URL"
fi
FILE="tailscale_${VERSION}_${ARCH}.tgz"
echo "Installing Tailscale $VERSION ($ARCH)"

# 2. Download and verify into a fresh temporary folder.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
curl -fsSL "$BASE_URL/$FILE" -o "$WORK/$FILE"
curl -fsSL "$BASE_URL/$FILE.sha256" -o "$WORK/$FILE.sha256"

EXPECTED="$(tr -d '[:space:]' < "$WORK/$FILE.sha256" | cut -c1-64)"
ACTUAL="$(sha256sum "$WORK/$FILE" | cut -c1-64)"
[ "${#EXPECTED}" -eq 64 ] || die "published checksum looks malformed"
if [ "$EXPECTED" != "$ACTUAL" ]; then
  die "checksum mismatch (expected $EXPECTED, got $ACTUAL); not installing"
fi
echo "Checksum OK: $ACTUAL"

# 3. Install under ~/.local.
mkdir -p "$OPT_DIR" "$BIN_DIR" "$STATE_DIR"
chmod 700 "$STATE_DIR"
tar -xzf "$WORK/$FILE" -C "$OPT_DIR" --strip-components=1
ln -sfn "$OPT_DIR/tailscale"  "$BIN_DIR/tailscale"
ln -sfn "$OPT_DIR/tailscaled" "$BIN_DIR/tailscaled"
"$BIN_DIR/tailscale" version | head -n 1

# 4. Start tailscaled as this user, in userspace-networking mode.
if pgrep -u "$(id -u)" -f "tailscaled.*--socket=$SOCKET" >/dev/null 2>&1; then
  echo "tailscaled is already running for this user"
else
  nohup "$BIN_DIR/tailscaled" \
    --tun=userspace-networking \
    --state="$STATE" \
    --socket="$SOCKET" \
    >"$LOG" 2>&1 &
  for _ in $(seq 1 20); do
    [ -S "$SOCKET" ] && break
    sleep 0.5
  done
  [ -S "$SOCKET" ] || die "tailscaled did not start; see $LOG"
  echo "tailscaled started (log: $LOG)"
fi

# 5. Log in. This prints a link; open it on your phone and approve.
echo
echo "Open the login link below on your phone to add this machine to your tailnet."
"$BIN_DIR/tailscale" --socket="$SOCKET" up --timeout=10m || true
echo
"$BIN_DIR/tailscale" --socket="$SOCKET" status || true
echo
echo "Use: tailscale --socket=$SOCKET status"
