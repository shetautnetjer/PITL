# Using 1Password secrets in programs: the op.env convention

This is how agents and programs on a PITL machine use secrets from 1Password
without a password each time, and without the secret ever sitting in a file,
a log or a chat.

## The idea in one paragraph

Each program has a small **op.env** file that lists the secrets it needs as
**1Password references** (`op://vault/item/field`), never the secrets
themselves. When you start the program through **`pitl-op run`**, 1Password
fills in the real values for that one program, in its memory only. The
1Password service account token that makes this work was stored once with
`pitl store` and is never handed to the program.

## Where things live

| What | Where |
|---|---|
| Service account token (stored once from your phone) | `~/.config/pitl/secrets/op-service-account` (file 0600) |
| One op.env per program | `~/.config/op-env/<program>.op.env` (folder 0700, files 0600) |
| Tools | `pitl-op` (runs `op` with the token), `pitl-op-env` (manages op.env files) |

An op.env file looks like this, and nothing else:

```text
# op.env for myagent: references only, never values.
OPENAI_API_KEY=op://<vault-id>/<item-id>/credential
GITHUB_TOKEN=op://<vault-id>/<item-id>/credential
```

## The three steps

### 1. Find the item once

Look up the item's id, not its value. Ask for JSON and read only the ids and
field names:

```bash
pitl-op item list --vault <vault> --format json      # titles and ids
pitl-op item get "<title>" --vault <vault> --format json \
  | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["vault"]["id"], d["id"], [f["id"] for f in d["fields"]])'
```

Use ids in references (`op://<vault-id>/<item-id>/<field>`). They survive
renames and never contain spaces, which env files can't handle.

### 2. Add its reference

```bash
pitl-op-env add <program> <ENV_NAME> op://<vault-id>/<item-id>/<field>
```

This checks that the reference resolves (the value is thrown away, never
shown) and appends one line to `<program>.op.env`. It refuses anything that
isn't a reference, names that aren't `LIKE_THIS`, and names already in the
file. Do this once per secret; later runs need no lookup.

### 3. Always run through pitl-op

```bash
pitl-op-env run <program> -- <command> [args...]
# which is the same as:
pitl-op run --env-file ~/.config/op-env/<program>.op.env -- <command> [args...]
```

The command starts with each `ENV_NAME` set to the real value. Before
running, `pitl-op-env run` refuses an op.env that contains anything other
than references, or that others can read. `pitl-op run` starts the command
**without** the service account token, so the program gets only the secrets
its op.env lists.

Other commands: `pitl-op-env list <program>` shows names and references,
`pitl-op-env check <program>` confirms each reference still resolves (names
only), `pitl-op-env path <program>` prints the file's location.

## Rules (for agents especially)

- **Never write a secret value** to a file, a log, a commit, an issue, a
  chat, or a command line. Only `op://` references go in op.env files.
- **Never print a value to check it.** `pitl-op-env add` and `check` already
  confirm a reference works without showing it.
- **Never run `pitl-op read`** (or `op read`) where its output can reach a
  chat, terminal log or file. If you must test, send its output to
  `/dev/null`.
- **Always start programs through `pitl-op run`** (or `pitl-op-env run`), so
  values exist only inside the running program.
- If a needed item doesn't exist, **ask the human**. Don't create items or
  paste values on anyone's behalf.

## What this protects, and what it doesn't

It keeps values out of files, logs, chats and other programs' environments,
and gives each program only what its op.env lists.

It does **not** isolate agents running as the same user from each other: any
of them can read the service account token file, run `pitl-op` themselves,
or read a running program's environment through `/proc`. Everything the
service account can reach is reachable by every agent of that user. Scope the
service account narrowly. See [SECURITY.md](SECURITY.md).
