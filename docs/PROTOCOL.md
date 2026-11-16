# PITL protocol (0.1)

## Pieces

```text
 agent ──runs──► sudo -A -k <cmd>
                   │ runs SUDO_ASKPASS with the prompt as argv[1]
                   ▼
               pitl-askpass ──unix socket (0600)──► pitld ◄──HTTPS (Tailscale Serve)── phone
                   │ stdout (pipe)                     │
                   ▼                                   └─ audit.jsonl (metadata only)
                  sudo
```

## Request states

```text
Created ──► Pending ──► Authorized ──► Consumed
               │            │
               ├──► Denied  ├──► Expired   (secret dropped)
               ├──► Expired └──► Canceled  (secret dropped)
               └──► Canceled
                                 Consumed ──► Failed (delivery to askpass failed)
```

- **Created → Pending** happens before the request is visible to anyone.
- **Pending → Authorized** when the owner submits a password (once).
- **Authorized → Consumed** is the single atomic consume: the secret is moved
  out of the store and handed to the connection that created the request.
  Every other caller gets "already finished".
- **Denied, Expired, Canceled, Failed** are final and carry no secret.
- All state is in memory. Restarting pitld clears every request.

## Unix socket

Path: `$PITL_SOCKET`, else `$XDG_RUNTIME_DIR/pitl/pitld.sock`, else
`/run/user/<uid>/pitl/pitld.sock`. Directory 0700, socket 0600. pitld also
checks the peer uid from `SO_PEERCRED`.

Messages are one JSON object per line, at most 8 KiB.

### askpass

```text
→ {"op":"ask","prompt":"[sudo] password for alice: "}
← {"status":"pending","id":"<32 hex>","url":"https://host.tailnet.ts.net/r/<id>","expires_unix":1790000000}
   ... waits ...
← {"status":"secret"}
← <secret bytes>\n                       (raw line, not JSON; then the connection closes)
   or
← {"status":"denied"} | {"status":"expired"} | {"status":"canceled"} | {"status":"failed"}
```

askpass sends nothing after its question. If pitld reads anything more, or the
connection closes, the request is canceled.

On `secret`, askpass writes the secret plus a newline to stdout and exits 0.
On anything else it writes nothing to stdout and exits 1, which sudo treats as
no password.

### CLI

```text
→ {"op":"list"}
← {"status":"list","pending":[{"id":...,"state":"pending","info":{...},"expires_unix":...,"seconds_left":...,"url":...}]}
```

The list never includes secrets.

## What pitld shows the owner

Gathered by pitld, not supplied by the asker:

| Field | Source |
|---|---|
| Machine | `/proc/sys/kernel/hostname` |
| User, uid | peer credentials of the socket, `/etc/passwd` |
| Command | `/proc/<parent of askpass>/cmdline` (normally the `sudo` command line) |
| Started by | `/proc/<parent of that>/cmdline` |
| Expires | pitld's own clock |
| Prompt text | sudo's argv[1] to askpass, labelled unverified |

Command lines are truncated to 512 characters, and control characters are
replaced, so they can't fake text on the page.

## HTTP (phone page)

pitld listens on `127.0.0.1:8787` by default; Tailscale Serve publishes it on
the tailnet over HTTPS.

| Method and path | Does |
|---|---|
| `GET /r/{id}` | Request page with details, a password box, Approve and Deny. 404 if unknown, 410 if finished |
| `POST /r/{id}/approve` | Form field `password`. 200 sent, 400 empty or contains a line break, 409 not pending, 413 over 4 KiB |
| `POST /r/{id}/deny` | 200 denied, 409 not pending |
| `GET /static/pitl.css`, `/static/pitl.js` | Styles, countdown |

`{id}` must be exactly 32 lowercase hex characters.

Every response carries `Cache-Control: no-store`, `Referrer-Policy:
no-referrer`, `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY` and a
strict `Content-Security-Policy` (no inline script, forms post only to self).

## Audit log

`$XDG_STATE_HOME/pitl/audit.jsonl`, else `~/.local/state/pitl/audit.jsonl`,
mode 0600. One JSON object per event:

```json
{"time":"2026-01-01T12:00:00Z","event":"authorized","id":"<32 hex>","machine":"host","user":"alice","command":"sudo -A -k /usr/bin/id","requester":"bash","requester_pid":1234,"answered_by":"unverified:alice@example.com","device":"unverified:100.64.0.2"}
```

Events: `created`, `authorized`, `consumed`, `denied`, `expired`, `canceled`,
`failed`. The record type has no field for a secret.
