# PITL threat model (0.1, compatibility mode)

## What is being protected

1. **The secret** (in 0.1, the owner's sudo password).
2. **The decision**: a privileged action happens only when the owner chose it,
   knowing what it is.

## Parties and trust

| Party | Trusted? | Notes |
|---|---|---|
| Owner and their phone | Yes | The phone browser is where the secret is typed |
| Tailscale (control plane, Serve) | Yes, for reachability | Decides who can reach the page; terminates HTTPS |
| Other devices on the tailnet | Partly | Can reach the page if they have the link |
| pitld | Yes | Runs as the owner's user |
| The agent | **Honest but careless** in 0.1 | Runs as the owner's user |
| Other local users | No | Blocked by file permissions |
| The internet | No | Never reaches pitld |

## The big limitation: same-user hostile agent

In 0.1, pitld, pitl-askpass, sudo's invoking process and the agent all run as
the **same Unix user**. A hostile agent with that user's access can:

- put a fake `sudo` first in `PATH`, show a believable command on the page
  (through process names it controls) and capture the password from askpass;
- replace `~/.local/bin/pitl-askpass` with a program that keeps a copy;
- read or `ptrace` the askpass process, or the pipe into sudo, where the
  kernel allows it;
- connect to the pitld socket itself and pose as askpass;
- shape the process tree pitld inspects, so the page shows misleading details.

**0.1 does not defend against this.** It keeps an honest agent from ever
*holding* the secret (it never enters the agent's context, chat, files or
logs), and it puts a human decision in front of every privileged action. A
malicious agent running as the same user can defeat it.

The fix is **protected mode (0.3)**: an executor running as a different user
(or as a small privileged helper) that receives the approval and runs the
approved command itself. The agent's user then has no process that ever holds
the secret, and the command shown to the owner is the command that runs.

## Threats 0.1 does address

| Threat | Mitigation |
|---|---|
| Secret lands in the agent's chat, files or logs | Secret only flows phone → pitld → askpass stdout → sudo. pitld never logs it; askpass writes it only to stdout |
| Secret lands in PITL's own logs | The audit record type has no field for a secret; tests check the log |
| Secret lingers in memory | Held in zeroizing buffers in pitld and askpass, dropped right after use (see limits below) |
| Someone guesses a link | Request ids are 128 random bits from the kernel |
| A link is reused | Single atomic consume; every later use gets "already finished" |
| A link is answered late | Requests expire (3 min default, 10 min max); an authorized-but-undelivered secret is dropped at expiry |
| Answer arrives after the asker left | Disconnect cancels the request; later answers are refused |
| Daemon restarts mid-request | Requests are in memory only; restart forgets them, askpass fails |
| Agent writes misleading text on the page | Page details come from pitld (kernel peer credentials and `/proc`), not from agent text; sudo's prompt is shown but labelled unverified; all text is HTML-escaped and control characters removed |
| Page reached from the internet | Bound to 127.0.0.1; exposed only through Tailscale Serve (tailnet-only HTTPS) |
| Other local users use the socket | Socket 0600 in a 0700 directory; peer uid checked |
| Page framed, cached or leaked by Referer | CSP, `X-Frame-Options: DENY`, `Cache-Control: no-store`, `Referrer-Policy: no-referrer` |
| Oversized input | 4 KiB form limit, 8 KiB socket line limit |

## Known limits in 0.1

- **No phone authentication beyond the link and the tailnet.** Anyone on the
  tailnet who obtains the link can answer. Passkeys arrive in 0.2.
- **The agent sees the link** (askpass prints it to stderr so the agent can
  pass it on). Holding the link lets it deny, or submit a wrong password; it
  does not reveal the right one.
- **Identity headers.** With Tailscale Serve in front, pitld records the
  `Tailscale-User-Login` header. Because local processes can also reach
  127.0.0.1, pitld marks these as `unverified:` unless started with
  `--behind-serve`. In userspace-networking mode, tailnet peers may also reach
  the local port directly, so `--behind-serve` should only be used when that
  is ruled out.
- **Memory.** Zeroizing covers PITL's own buffers. Copies made by the HTTP
  stack while parsing the request body, by the kernel's socket buffers, or by
  the phone's browser are outside PITL's control.
- **Tailscale Serve terminates TLS** on the same machine and forwards plain
  HTTP to 127.0.0.1. Local processes of the same user could observe that hop
  (same-user limitation again).
