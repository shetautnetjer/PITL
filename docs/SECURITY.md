# Security

## Read this first

PITL 0.1 runs in **compatibility mode**. It keeps secrets out of an agent's
chat, files and logs, and puts a human decision in front of privileged
commands. It does **not** protect you from a hostile agent running as the same
Unix user: such an agent can wrap `sudo`, swap the askpass program, or read the
pipe to sudo. Details in [THREAT-MODEL.md](THREAT-MODEL.md). Protected mode,
with an isolated executor, is planned for 0.3.

## Stored secrets (`pitl store`) are readable by every agent of that user

`pitl store --name NAME` writes the value you type on your phone to
`~/.config/pitl/secrets/NAME`, mode 0600, in a 0700 directory. That keeps it
away from **other** users, the agent's chat, PITL's logs and the program that
asked. It does **not** keep it away from programs running as **the same
user**: in 0.1, **any agent running as that user can read the file** (and
`pitl-op` places it in the environment of the `op` process it starts).

Store only what you are willing to give every agent on that account. For
example, a 1Password service account token gives each of those agents
everything that service account can reach, such as a whole vault. Scope the
service account to the smallest set of vaults, prefer read-only access, and
revoke the token in 1Password if the machine or an agent is in doubt. Delete
the file to take the access away locally.

Protected mode (0.3) is meant to change this: secrets held by a separate user
or helper and used on the agent's behalf, never readable by the agent's user.

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

In scope: secret leaks into logs, files or agent-visible output (other than the stored-secret file itself, by design); store requests writing outside the secrets directory or with loose permissions; replay or
double use of a request; bypassing expiry or deny; reaching the page from
outside the tailnet in the recommended setup; HTML or header injection on the
page.

Known and out of scope for 0.1: attacks that require running code as the same
Unix user as pitld (see the threat model).
