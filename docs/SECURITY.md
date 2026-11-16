# Security

## Read this first

PITL 0.1 runs in **compatibility mode**. It keeps secrets out of an agent's
chat, files and logs, and puts a human decision in front of privileged
commands. It does **not** protect you from a hostile agent running as the same
Unix user: such an agent can wrap `sudo`, swap the askpass program, or read the
pipe to sudo. Details in [THREAT-MODEL.md](THREAT-MODEL.md). Protected mode,
with an isolated executor, is planned for 0.3.

## Recommended setup

- Expose pitld only through **Tailscale Serve** (tailnet-only HTTPS). Don't
  use Tailscale Funnel or any public tunnel.
- Keep pitld on `127.0.0.1` (the default).
- Use a short expiry (the default 3 minutes is fine).
- Read the command on the phone page before answering. If you didn't expect
  the request, press **Deny**.
- Restrict your tailnet so only your own devices can reach the machine.

## Reporting a vulnerability

Please report security problems privately via GitHub's "Report a
vulnerability" (Security tab of this repository), not in a public issue.
Include what you found, how to reproduce it, and the version or commit.

## Scope

In scope: secret leaks into logs, files or agent-visible output; replay or
double use of a request; bypassing expiry or deny; reaching the page from
outside the tailnet in the recommended setup; HTML or header injection on the
page.

Known and out of scope for 0.1: attacks that require running code as the same
Unix user as pitld (see the threat model).
