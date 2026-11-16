# Notes for agents working on this repository

## What this is

PITL ("Password In The Loop") lets an agent ask a human, on their phone, for
a secret, without the secret passing through the agent. Read
[docs/PRD.md](docs/PRD.md) and [docs/THREAT-MODEL.md](docs/THREAT-MODEL.md)
before changing behaviour.

## Layout

| Path | What |
|---|---|
| `crates/pitl-core` | State machine, request ids, socket protocol, audit record type, `/proc` helpers. No networking |
| `crates/pitl-daemon` | `pitld`: unix socket server, Axum phone page, audit log. Integration tests in `tests/flow.rs` |
| `crates/pitl-askpass` | `SUDO_ASKPASS` helper. Synchronous, no async runtime |
| `crates/pitl-cli` | `pitl store`, `pitl list`, `pitl audit`, `pitl paths`. Never handles secret values |
| `contrib/pitl-op` | Runs the 1Password CLI with a token stored by `pitl store`; `contrib/test-pitl-op.sh` tests it |
| `bootstrap/` | No-sudo Tailscale setup |
| `docs/` | PRD, threat model, protocol, security policy |

## Rules that must not break

1. **A secret goes to exactly one place**: the waiting askpass's stdout
   (sudo requests) or its 0600 file (store requests). Not to stderr, logs,
   the audit file, the CLI, the page, error messages, panics or debug output.
   `AuditRecord` has no field for it; keep it that way.
2. **Secrets live in `Zeroizing` buffers** and are dropped as soon as they are
   delivered.
3. **One atomic consume.** Only `Store::consume` moves a secret out, under the
   store lock, and only once.
4. **Fail closed.** Unknown, expired, canceled, failed, or a dropped
   connection all mean no secret.
5. **What the human sees comes from pitld**, not from text the requester
   supplied. Anything requester-supplied is escaped and labelled unverified.
6. **No sudo** in build, install or tests.
7. Don't put machine names, tailnet names, IPs, usernames or home paths in the
   repository.

## Commands

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
sh contrib/test-pitl-op.sh
```

Install for local use (no sudo):

```bash
cargo build --release --workspace
install -m 0755 target/release/{pitld,pitl,pitl-askpass} ~/.local/bin/
```

## When you change behaviour

Update `docs/PROTOCOL.md` if messages, states or HTTP routes change, record
the reason in `DECISIONS.md`, and update `STATUS.md`.
