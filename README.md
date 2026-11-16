# PITL: Password In The Loop

Agents ask a human for secrets from a phone, without the secret passing through the agent.

> **Status: early design.** This repository holds the design and the first
> building block (a no-sudo Tailscale setup). The phone flow described below is
> not built yet.

## The problem

AI agents now do real work on real machines: install packages, fix services,
sign in to tools. Sooner or later they hit a prompt that only a human should
answer:

- `sudo` asks for a password.
- A password manager such as 1Password is locked and wants its master password.
- A tool wants an API key or a one-time code.

On a machine nobody is sitting at, such as an agent PC, a home server or a box
in the cloud, the work just stops. The usual workarounds are bad:

- Paste the password into the agent's chat. Now it sits in the chat history,
  the model provider's logs and maybe the agent's memory.
- Put it in a file or an environment variable. Now any process, and the agent
  itself, can read it.
- Turn off the password entirely (passwordless sudo for everything). Now there
  is no human check at all.

PITL is a fourth way: the agent asks, a human answers from their phone, and the
secret goes straight to the waiting prompt. The agent sees only "approved" or
"declined".

## How it is meant to work

1. **The agent hits a prompt.** A command needs `sudo`, or 1Password is
   locked.
2. **The agent asks PITL.** PITL opens a request saying what is being asked
   for, by which command, on which machine.
3. **A one-time link goes to your phone.** It travels over your own private
   network ([Tailscale](https://tailscale.com)), never the public internet.
   The link works **once** and **expires within minutes**.
4. **You approve on your phone.** You confirm with your phone's passkey (Face
   ID, fingerprint or PIN), then type or paste the secret into a **blind entry
   box**.
5. **The secret goes straight to the waiting prompt.** PITL hands it to the
   prompt the same way `sudo`'s askpass helper does: from PITL's own process
   into the process that asked. It is **never** written into the agent's chat,
   its files or its logs, and the agent never receives it.
6. **Everything is recorded, except the secret.** Each request is logged with
   the time, the device that answered, and what was asked for and why. The
   secret itself is never logged.

### Pairing

Your phone is paired with a machine once, by scanning a **QR code** shown on
that machine. After that, only paired phones can answer, and every answer also
needs the phone's passkey.

### Design rules

- The secret never passes through the agent, in either direction.
- Links are single-use and short-lived.
- Only paired devices can answer, and each answer needs a passkey.
- No passwordless sudo, no secrets stored on disk by PITL.
- Every request is logged; secrets never are.
- A human can always say no, and "no" is the default when nobody answers.

## What is here today

### `bootstrap/tailscale-nosudo.sh`

Puts a machine on your Tailscale network **without sudo**, so your phone can
reach it. It:

1. downloads Tailscale's official static build from `pkgs.tailscale.com`,
2. checks it against the SHA-256 checksum Tailscale publishes next to it, and
   stops if they differ,
3. installs it into your home folder (`~/.local`),
4. starts `tailscaled` as your user in userspace networking mode, and
5. runs `tailscale up`, which prints a login link you can approve from your
   phone.

```bash
bash bootstrap/tailscale-nosudo.sh
```

It contains no keys or secrets. In userspace mode, other devices on your
tailnet can reach this machine; programs on this machine do not route their
own traffic through Tailscale. The daemon is started by hand, so it does not
come back after a reboot until you run the script again.

## License

MIT. See [LICENSE](LICENSE).
