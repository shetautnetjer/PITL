# Status

## 0.1.0 (compatibility mode)

### Works

- Rust workspace: `pitl-core`, `pitl-daemon` (`pitld`), `pitl-cli` (`pitl`),
  `pitl-askpass`.
- `SUDO_ASKPASS=~/.local/bin/pitl-askpass sudo -A -k <cmd>` asks pitld over a
  0600 unix socket and waits.
- pitld creates a request (128-bit random id, 3-minute expiry), gathers the
  details from the kernel and `/proc`, and serves a phone page.
- Phone page: details, password box, Approve, Deny, countdown. Strict security
  headers, no inline script, all text escaped.
- Single atomic consume; deny, expiry, cancel on disconnect and failed delivery
  all fail closed.
- Audit log (JSONL, 0600) with metadata only.
- Builds without warnings; `cargo clippy -D warnings` and `cargo fmt` clean.

### Tests

24 passing (`cargo test --workspace`):

- core: state machine (approve/consume once, deny final, expiry blocks approval
  and drops an undelivered secret, cancel, cancel after authorize, unknown id,
  16 threads racing to consume get exactly one secret), id parsing, `/proc`
  parsing, time formatting, wire format.
- daemon end-to-end (real socket client plus the real HTTP router): secret
  delivered once and the connection closed; replay and double consume
  refused; deny sends nothing; expiry fails closed; disconnect cancels and
  blocks later approval; audit has metadata but never the secret, and is 0600;
  socket is 0600; bad ids, empty or multi-line passwords and oversized bodies
  rejected; page escapes HTML and sets security headers.

### Tried live

- pitld running behind Tailscale Serve (tailnet-only HTTPS) in userspace
  networking mode, without root.
- First live `sudo -A -k /usr/bin/id`: nobody answered within 3 minutes; the
  request expired, askpass printed nothing to stdout and sudo refused. The
  audit log shows `created` then `expired`. Fail-closed path confirmed.
- Learned: sudo does not call askpass again after an empty answer.

### Not done yet

- A live run where the password is approved from the phone end to end.
- Phone pairing (QR) and passkey approval: 0.2.
- Push notification to the phone: the link is printed to the agent (stderr)
  and listed by `pitl list`.
- Protected mode (isolated executor): 0.3.
- 1Password support.
- A service file to keep pitld running across reboots.
