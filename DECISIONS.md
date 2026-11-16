# Decisions

Newest last. Each entry: what was decided, and why.

## D1. Human authorization first, password relay second

PITL's job is to put a human decision in front of a privileged action. For
0.1 the only way to make that decision count with existing programs is to
carry the password, so it does. Later versions should prefer approvals that
need no secret at all (protected-mode executor, policy approvals).

## D2. Ship 0.1 as "compatibility mode" and say so plainly

An askpass helper running as the agent's own user cannot stop a hostile agent
of that user. Rather than hold the release for an isolated executor, 0.1 ships
with that limit written into the README, SECURITY.md and the threat model.
Protected mode is 0.3.

## D3. Askpass over a 0600 unix socket

`SUDO_ASKPASS` is supported by sudo without configuration changes or root.
The socket sits in the user's runtime directory (0700), is 0600, and pitld
checks the peer uid. The peer pid from `SO_PEERCRED` lets pitld find the real
command instead of trusting text.

## D4. Raw line for the secret, JSON for everything else

The secret is sent as a raw line after a `{"status":"secret"}` message, so it
never sits inside a JSON string or parser buffer, and askpass can read it
byte by byte into a zeroizing buffer.

## D5. In-memory state only

Pending requests are never written to disk. A restart forgets them and every
waiting askpass fails. That is the safe direction.

## D6. 128-bit random ids from `/dev/urandom`

The id in the link is the capability. 128 bits from the kernel, no extra
dependency. Ids are parsed strictly (32 lowercase hex characters).

## D7. Tailscale Serve, bind to 127.0.0.1

The page must never be on the public internet. Tailscale Serve gives
tailnet-only HTTPS with a real certificate and works with userspace
networking, so no root is needed. Plain tailnet-IP HTTP is the fallback when
Serve is not available.

## D8. Identity headers are "unverified" by default

`Tailscale-User-Login` is only trustworthy when nothing but Serve can reach
pitld. Local processes can reach 127.0.0.1, so headers are recorded with an
`unverified:` prefix unless `--behind-serve` is given.

## D9. Minimal dependencies

Axum and Tokio for the daemon, serde for messages, zeroize for secrets.
Time formatting, hex and `/proc` parsing are a few lines each and written in
place. askpass is synchronous and has no async runtime.

## D10. Secrets with line breaks are refused

sudo reads one line from askpass. A password containing a newline, carriage
return or NUL would be cut or corrupted, so the page rejects it rather than
sending something different from what the owner typed.

## D11. Default expiry 3 minutes, maximum 10

Long enough to pick up a phone, short enough that a forgotten link is dead.
Live testing showed sudo does **not** retry when askpass returns nothing: one
expiry ends that sudo run.
