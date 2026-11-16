# PITL product requirements

## One line

PITL is **human authorization** for agents: when an agent's command needs a
secret only a human should give, the human approves from their phone and the
secret goes to the waiting program, not through the agent.

PITL is not a password relay and not a password manager. The point is the
human decision. Carrying the secret is how 0.1 makes that decision effective
for programs that only understand passwords (sudo today).

## Problem

Agents run real work on machines nobody is sitting at. They stop at:

- `sudo` asking for a password,
- a locked password manager (for example 1Password),
- a tool asking for a key or a one-time code.

Today's workarounds put the secret in the agent's chat or files, or remove the
human check entirely (passwordless sudo for everything).

## Users

- **The owner**: the human who owns the machine and the secrets. Usually away
  from the machine, with a phone.
- **The agent**: an AI coding or operations agent running as the owner's user
  account on that machine.

## Goals for 0.1 ("compatibility mode")

1. An agent runs `SUDO_ASKPASS=~/.local/bin/pitl-askpass sudo -A -k <cmd>`.
2. The owner gets a one-time link, reachable only over their tailnet.
3. The page shows what is being asked, gathered by pitld itself (machine,
   user, the exact command, who started it, when it expires), not text the
   agent wrote.
4. The owner can **Deny**, or enter the password.
5. The password goes once to the askpass process that asked, which prints it
   only to its stdout (a pipe to sudo).
6. The password never appears in the agent's chat, files, environment or
   logs, and never in PITL's logs.
7. Links are single-use and expire in 3 minutes by default.
8. Every request is logged (time, request, who answered), never the secret.
9. Anything unexpected fails closed: no answer, expiry, a dropped connection,
   a daemon restart or a reused link all mean "no".

### Added in 0.1: store a secret

`pitl store --name NAME` asks the owner, through the same kind of link, for a
long-lived secret (for example a 1Password service account token). pitld
writes it to a 0600 file; the asking program learns only "stored" and the
path. `contrib/pitl-op` runs the 1Password CLI with that token.

## Non-goals for 0.1

- Protection against a **hostile agent** running as the same user. That
  agent can wrap `sudo`, replace the askpass program, or read the pipe.
  0.1 keeps honest agents from ever *holding* the secret; it cannot stop a
  malicious one. See [THREAT-MODEL.md](THREAT-MODEL.md).
- Phone pairing and passkeys (planned for 0.2).
- 1Password and other secret sources (planned).
- Persistence: pending requests live in memory only.

## Roadmap

| Version | What |
|---|---|
| 0.1 | Compatibility mode: askpass for sudo, phone page over Tailscale Serve, single-use expiring links, audit log |
| 0.2 | QR pairing of a phone; WebAuthn passkey required for every answer; device identity in the audit log; notification to the phone |
| 0.3 | **Protected mode**: an isolated executor running as a separate user (or privileged helper) that runs the approved command itself, so the agent's user never handles the secret or the privileged process at all |
| later | 1Password unlock and item requests; approval without a password (policy-based) where the target supports it |

## Success for 0.1

- An agent on a headless machine gets `sudo` approved from a phone, with the
  owner typing the password only on the phone.
- The test suite proves: deny, expiry, replay/double consume, disconnect fails
  closed, audit never contains the secret.
