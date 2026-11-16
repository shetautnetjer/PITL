#!/bin/sh
# Tests for pitl-op-env and `pitl-op run`, using a stand-in for op.
# No real 1Password account or values are used.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

VALUE="fake-secret-value-9d2c"

# Stand-in op: `read` resolves refs containing "good"; `run` exports each
# NAME=op://... from --env-file with a fake value, then runs the command.
cat > "$work/op" <<FAKE
#!/bin/sh
[ -n "\${OP_SERVICE_ACCOUNT_TOKEN:-}" ] || { echo "no token" >&2; exit 9; }
case "\$1" in
  read)
    ref="\$3"
    case "\$ref" in *good*) printf '%s' "$VALUE"; exit 0 ;; esac
    echo "[ERROR] could not find item" >&2; exit 1 ;;
  run)
    shift
    [ "\$1" = "--env-file" ] || exit 3
    file="\$2"; shift 2
    [ "\$1" = "--" ] || exit 4
    shift
    while IFS= read -r line; do
      case "\$line" in [A-Z_]*=op://*) export "\${line%%=*}=$VALUE" ;; esac
    done < "\$file"
    exec "\$@" ;;
esac
exit 5
FAKE
chmod 0755 "$work/op"

mkdir -m 0700 "$work/secrets"
printf 'ops_FAKE-token\n' > "$work/secrets/op-service-account"
chmod 0600 "$work/secrets/op-service-account"

export PITL_REAL_OP="$work/op" PITL_SECRETS_DIR="$work/secrets" PITL_OP="$here/pitl-op"
export PITL_OP_ENV_DIR="$work/op-env"
ope() { "$here/pitl-op-env" "$@"; }

# add: checks the reference, writes only the reference, 0700/0600.
out="$(ope add demo DEMO_KEY op://vault/good-item/credential)"
case "$out" in *"$VALUE"*) fail "add printed the value" ;; esac
f="$PITL_OP_ENV_DIR/demo.op.env"
grep -q '^DEMO_KEY=op://vault/good-item/credential$' "$f" || fail "reference not written"
grep -q "$VALUE" "$f" && fail "value written to env file"
[ "$(stat -c %a "$PITL_OP_ENV_DIR")" = 700 ] || fail "folder not 0700"
[ "$(stat -c %a "$f")" = 600 ] || fail "file not 0600"
echo "ok: add checks the reference and stores only the reference (0700/0600)"

# add: unresolvable reference is refused and not written.
if ope add demo OTHER op://vault/missing/credential 2>"$work/err"; then fail "accepted a bad reference"; fi
grep -q OTHER "$f" && fail "bad reference written"
grep -q "could not resolve" "$work/err" || fail "unclear error"
echo "ok: a reference that doesn't resolve is refused"

# add: things that aren't references are refused.
for bad in "plain-value" "op://only-two/parts" "op://v/item with space/f"; do
  if ope add demo X_KEY "$bad" 2>/dev/null; then fail "accepted '$bad'"; fi
done
if ope add demo lower op://v/good/f 2>/dev/null; then fail "accepted lowercase name"; fi
if ope add ../evil KEY op://v/good/f 2>/dev/null; then fail "accepted bad program name"; fi
if ope add demo DEMO_KEY op://v/good/other 2>/dev/null; then fail "accepted duplicate name"; fi
echo "ok: values, malformed references, bad names and duplicates are refused"

# run: the program sees the value; the token does not reach it; output has no value.
out="$(ope run demo -- sh -c '
  [ "$DEMO_KEY" = "'"$VALUE"'" ] && echo value-present || echo value-wrong
  [ -z "${OP_SERVICE_ACCOUNT_TOKEN:-}" ] && echo token-absent || echo token-leaked')"
[ "$out" = "value-present
token-absent" ] || fail "run: $out"
echo "ok: run gives the program the value and withholds the service token"

# run: a file with a value in it is refused before anything runs.
printf 'LEAKED=%s\n' "$VALUE" >> "$f"
if ope run demo -- true 2>"$work/err"; then fail "ran with a value in the env file"; fi
grep -q "not NAME=op://" "$work/err" || fail "unclear error for value line"
grep -q "$VALUE" "$work/err" && fail "error printed the value"
echo "ok: run refuses an env file that contains a value"

# run: loose permissions are refused.
sed -i '/^LEAKED=/d' "$f"; chmod 0644 "$f"
if ope run demo -- true 2>/dev/null; then fail "ran with a 0644 env file"; fi
chmod 0600 "$f"
echo "ok: run refuses an env file others can read"

# check: reports names only.
out="$(ope check demo)"
[ "$out" = "ok      DEMO_KEY" ] || fail "check: $out"
echo "ok: check reports names, never values"

echo "all pitl-op-env tests passed"
