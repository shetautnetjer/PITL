#!/bin/sh
# Tests for pitl-op using a stand-in for op. Never uses a real token.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fake="$work/op"
cat > "$fake" <<'FAKE'
#!/bin/sh
# Report only whether the token arrived and its length, never its value.
if [ -n "${OP_SERVICE_ACCOUNT_TOKEN:-}" ]; then
  echo "token-present len=${#OP_SERVICE_ACCOUNT_TOKEN} args=$*"
else
  echo "token-missing args=$*"
fi
FAKE
chmod 0755 "$fake"

secrets="$work/secrets"
mkdir -m 0700 "$secrets"
value="ops_FAKE-test-token-123"
printf '%s\n' "$value" > "$secrets/op-service-account"
chmod 0600 "$secrets/op-service-account"

run() { PITL_REAL_OP="$fake" PITL_SECRETS_DIR="$secrets" "$here/pitl-op" "$@"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

out="$(run vault list)"
[ "$out" = "token-present len=${#value} args=vault list" ] || fail "token not passed: $out"
case "$out" in *"$value"*) fail "value printed" ;; esac
echo "ok: token passed through environment, not printed"

chmod 0644 "$secrets/op-service-account"
if run vault list >/dev/null 2>"$work/err"; then fail "accepted a 0644 token file"; fi
grep -q "expected 600" "$work/err" || fail "wrong error for loose mode"
grep -q "$value" "$work/err" && fail "value in error output"
echo "ok: refuses a token file others can read"

rm "$secrets/op-service-account"
if run vault list >/dev/null 2>"$work/err"; then fail "ran without a token"; fi
grep -q "pitl store --name op-service-account" "$work/err" || fail "missing hint"
echo "ok: clear error when no token is stored"

echo "all pitl-op tests passed"
