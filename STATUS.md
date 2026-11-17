# Status

## 0.1.0 (compatibility mode)

**Live sudo round trip: passed.** `sudo -A -k /usr/bin/id` was approved from a
phone over the tailnet; the password reached sudo once, never through the
agent, and the command ran as root. Details under "Tried live" below.

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
- **Store requests** (`pitl store --name NAME`): the owner pastes a secret on
  the phone page; pitld writes it to `~/.config/pitl/secrets/NAME` (0600 file,
  0700 directory, atomic write). The client only hears "stored" and the path.
- `contrib/pitl-op` runs the 1Password CLI with a stored service account
  token, read at run time; refuses a token file that isn't 0600/0400.
- **op.env convention** (`docs/OP-ENV.md`): one `~/.config/op-env/<program>.op.env`
  per program with `op://` references only; `contrib/pitl-op-env` adds
  (checking the reference resolves without showing it), lists, checks and runs.
  `pitl-op run` starts the program without the service account token.
- Builds without warnings; `cargo clippy -D warnings` and `cargo fmt` clean.

### Tests

Store feature (on the `pitl-store-secret` branch): 13 more Rust tests and 3
shell tests. Value lands only in the 0600 file (not in the client reply, page
or audit); audit has name, time, device and outcome; link can't be reused;
deny, expiry and disconnect write nothing; unsafe names refused before
anything is shown; replacing a loosened file leaves it 0600; name validation;
atomic writer leaves no temporary files. `contrib/test-pitl-op.sh`: token
reaches op through the environment and is never printed; loose file mode
refused; clear error when nothing is stored.
`contrib/test-pitl-op-env.sh` (7 checks): add stores only the reference with
0700/0600; unresolvable references, values, malformed references, bad names
and duplicates refused; run gives the program the value but not the service
token; an op.env with a value line or loose mode is refused; check prints
names only.

Live on a real 1Password service account: `pitl-op-env add` resolved a test
item by id, `check` passed, and `pitl-op-env run` gave the program the value
while withholding the service token.

37 Rust tests pass in total (`cargo test --workspace`). The original 0.1 set:

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
- Second live `sudo -A -k /usr/bin/id`, answered from a phone over the
  tailnet: approved 37 seconds after the request, delivered once, and sudo ran
  `id` as root (exit 0). Audit: `created`, `authorized` (with the phone's
  Tailscale login and tailnet address, marked unverified), `consumed`.

### Not done yet

- Phone pairing (QR) and passkey approval: 0.2.
- Push notification to the phone: the link is printed to the agent (stderr)
  and listed by `pitl list`.
- Protected mode (isolated executor): 0.3.
- 1Password support.
- A service file to keep pitld running across reboots.

## 2026-11-17: unlocking the screen from the phone (first cut, not tried live)

- `pitl unlock` asks the Omarchy lock screen (`omarchy shell lock phoneUnlock`)
  to take its password from the phone. The lock runs `pitl-askpass` itself
  and submits the answer through its own PAM stack
  (`omarchy-lock-password`, with its faillock count), so the request alone
  unlocks nothing and PITL never decides.
- The lock needs `contrib/omarchy/lock-phone-unlock.patch` (27 lines in
  `shell/plugins/lock/Service.qml`); without it `pitl unlock` says so.
- Not done: tried on a locked screen; a request kind of its own, so the
  phone page says "Unlock the screen" as a verified fact (today it is the
  askpass prompt, marked unverified); a push to the phone.
