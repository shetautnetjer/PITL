# PITL: Password In The Loop

Agents ask a human for secrets from a phone, without the secret passing through the agent.

> **Status: 0.1, compatibility mode.** Works for `sudo`. Keeps the password out
> of the agent's chat, files and logs, and puts your decision in front of every
> privileged command. **It does not protect you from a hostile agent running
> as the same user**: such an agent can put a fake `sudo` in front of the real
> one, swap the askpass program, or read the pipe into sudo. Protected mode,
> with an isolated executor, is planned for 0.3. See
> [the threat model](docs/THREAT-MODEL.md).

## The problem

AI agents now do real work on real machines: install packages, fix services,
sign in to tools. Sooner or later they hit a prompt only a human should
answer:

- `sudo` asks for a password,
- a password manager such as 1Password is locked,
- a tool wants an API key or a one-time code.

On a machine nobody is sitting at, the work stops. The usual workarounds are
bad: paste the password into the agent's chat (now it is in the chat history
and the provider's logs), put it in a file or environment variable (now any
process can read it), or turn the password off entirely (now there is no human
check).

## What PITL does

PITL is **human authorization**, not a password relay. The agent asks; you
decide on your phone; if you approve, the password goes straight to the
program that is waiting for it. The agent only learns "approved" or "denied".

```text
agent runs:  SUDO_ASKPASS=~/.local/bin/pitl-askpass sudo -A -k <command>
                │
                ▼
pitl-askpass ──(private unix socket)──► pitld ──(one-time link, tailnet only)──► your phone
     ▲                                    │                                         │
     └──── password, once, to sudo ◄──────┴──────────── you approve or deny ◄───────┘
```

1. sudo needs a password and runs `pitl-askpass` instead of prompting.
2. pitl-askpass asks **pitld**, a small daemon running as you.
3. pitld creates a request with a random 128-bit id that expires in 3 minutes,
   and prints a one-time link.
4. You open the link on your phone. It is served through **Tailscale Serve**,
   so only devices on your tailnet can reach it, over HTTPS.
5. The page shows what pitld itself found out: the machine, the user, the
   exact command, what started it, and when the request expires. It does not
   show text the agent wrote (sudo's prompt is shown, marked unverified).
6. You press **Deny**, or type the password and press **Approve**.
7. pitld hands the password **once** to the askpass that asked. askpass prints
   it only to its stdout, which is a pipe into sudo. It is never written to
   the agent's chat, files or logs, nor to PITL's logs.
8. Each step is recorded in an audit log (time, request, command, who
   answered). The password never is.

Anything unexpected means **no**: no answer, expiry, the askpass going away, a
daemon restart, or a link used a second time.

## Try it (Linux, no root needed)

You need Rust (`rustup` installs into your home folder) and Tailscale. If
Tailscale isn't installed and you can't use sudo,
[`bootstrap/tailscale-nosudo.sh`](bootstrap/tailscale-nosudo.sh) installs it
into `~/.local`.

```bash
cargo build --release --workspace
install -m 0755 target/release/{pitld,pitl,pitl-askpass} ~/.local/bin/

# Publish the page on your tailnet only (HTTPS, tailnet-only):
tailscale serve --bg http://127.0.0.1:8787

# Start the daemon; use the address `tailscale serve` printed:
pitld --public-url https://<machine>.<tailnet>.ts.net &

# Ask for sudo through PITL:
SUDO_ASKPASS=~/.local/bin/pitl-askpass sudo -A -k /usr/bin/id
```

askpass prints the link on stderr. `pitl list` shows pending requests and
their links; `pitl audit` shows the audit log.

If you run Tailscale in userspace mode with its own socket, add
`--socket=<path>` to the `tailscale` commands.

## Store a secret from your phone (`pitl store`)

Some tools need a long-lived token, such as a 1Password service account
token. `pitl store` lets you hand one to the machine without pasting it into
an agent's chat:

```bash
pitl store --name op-service-account
```

You get the same kind of one-time, tailnet-only link. The page shows the
secret's name and the file it will go to. You paste the value and approve;
**pitld** writes it to `~/.config/pitl/secrets/op-service-account` (file
0600, folder 0700). The command that asked only learns "stored" and the path;
the value is never echoed or logged.

For 1Password, [`contrib/pitl-op`](contrib/pitl-op) runs the `op` CLI with
that token at run time:

```bash
install -m 0755 contrib/pitl-op ~/.local/bin/
pitl-op vault list
```

To give programs 1Password secrets without writing values anywhere, use the
**op.env convention**: each program has `~/.config/op-env/<program>.op.env`
holding only `op://` references, added once with `pitl-op-env add`, and is
always started with `pitl-op-env run <program> -- <command>`. See
[docs/OP-ENV.md](docs/OP-ENV.md).

> **Every program running as your user can read a stored secret**, including
> every agent. Store only what you are willing to give all of them. See
> [the security notes](docs/SECURITY.md#stored-secrets-pitl-store-are-readable-by-every-agent-of-that-user).

## Planned

- **0.2**: pair your phone by scanning a QR code; every answer needs your
  phone's passkey (Face ID, fingerprint or PIN); device identity in the audit
  log; a notification instead of copying the link.
- **0.3**: protected mode. A separate executor runs the approved command, so
  the agent's user never handles the secret or the privileged process.
- Later: 1Password unlock and item requests.

## Documents

- [Product requirements](docs/PRD.md)
- [Threat model](docs/THREAT-MODEL.md)
- [Protocol](docs/PROTOCOL.md)
- [Security policy](docs/SECURITY.md)
- [Using 1Password secrets in programs (op.env)](docs/OP-ENV.md)
- [Status](STATUS.md) and [decisions](DECISIONS.md)
- [Notes for agents working on this repo](AGENTS.md)

## License

MIT. See [LICENSE](LICENSE).
