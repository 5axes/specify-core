# `specify-core open`: the local app contract

`specify-core open FILE` serves the Specify app, the engine and one STEP file from this
computer and opens the browser at the app. This page is the contract between the command
(src/open.rs) and the app (built by specify-ui) that it serves.

```text
specify-core open FILE [--read-only | --in-place] [--port N] [--no-browser]
                       [--app DIR] [--wasm DIR]
```

`FILE` is a `.step` or `.stp` file, or either gzipped. `--read-only` and `--in-place` cannot be
combined. Every path the command shows or returns (`saveTarget`, a save's `path`, its notices)
is `FILE` as given, made absolute, never resolved: a link stays the link, and nothing is shown
in a canonical form such as Windows' `\\?\C:\…`.

## Addresses

Everything is under `http://127.0.0.1:<port>/<token>/`, where `<token>` is 32 lowercase hex
characters drawn from the operating system's random source for each run. The command prints

```text
Open part.step: http://127.0.0.1:<port>/<token>/
```

and opens the browser there (not with `--no-browser`). `<token>` without its trailing slash is
redirected (308) to it. The app must use relative URLs only: it is served under the token, never
at `/`.

| Request | Answer |
| --- | --- |
| `GET <token>/` | the app's `index.html` |
| `GET <token>/<path>` | the static app: the file at `<path>` in the app directory |
| `GET <token>/wasm/<path>` | the engine: the file at `<path>` in the wasm-pack `pkg/` directory |
| `GET <token>/file` | the STEP file's bytes, as on disk (gzipped if the file is), `application/octet-stream` |
| `GET <token>/config` | the app's settings, below; also the page's heartbeat |
| `POST <token>/save` | the specified STEP written to disk, below |
| `POST <token>/closed` | the page went away, below |

A query string is ignored. `file` serves the input as it was opened, or as the last `--in-place`
save left it.

### `config`

```json
{
  "name": "part.step",
  "readOnly": false,
  "wasmUrl": "wasm/specify_core_wasm.js",
  "saveTarget": "/home/me/parts/part.specified.step",
  "specifyCore": "0.1.17"
}
```

- `name`: the file's name.
- `readOnly`: saves are refused (`--read-only`); hide or disable Save.
- `wasmUrl`: the engine's entry module, relative to the app.
- `saveTarget`: the absolute path the next save would write (the input itself with
  `--in-place`), or `null` when read-only. It changes after each save. A save may still take
  another name if a file appears there first.
- `specifyCore`: this command's version.

### `save`

`POST <token>/save` with the STEP file as the body:

- `Content-Type: application/octet-stream` (415 otherwise);
- `Content-Length`, and no `Transfer-Encoding`: a chunked body is refused (411). `fetch` with
  an `ArrayBuffer` or `Blob` body sends exactly this; a `ReadableStream` body does not;
- `Origin: http://127.0.0.1:<port>` exactly (403 otherwise, and when `Origin` is absent);
- at most min(4 × the input's STEP text, 512 MB) (413, before the body is read; a gzipped input
  is measured unzipped);
- a STEP Part 21 file: `ISO-10303-21;` after an optional UTF-8 BOM and whitespace (400
  otherwise);
- the whole body within 30 s plus 1 s per MiB of `Content-Length` (408 otherwise; 400 if the
  connection ends first).

The file is written to a temporary file next to the input and renamed into place:

- by default in the directory of `FILE` as given (beside a link, not in its target's
  directory) under the first free name of `<stem>.specified.step`, `<stem>.specified-2.step`,
  `<stem>.specified-3.step`, … (`<stem>` is the name without `.gz` and `.step`/`.stp`:
  `part.stp` and `part.step.gz` both save as `part.specified.step`, uncompressed). An existing
  file is never overwritten, even by a concurrent save;
- with `--in-place` over the input. The first save of the run first copies the original to
  the first free name of `<FILE>.bak`, `<FILE>.bak-2`, … (an existing backup is never
  overwritten); later saves in the same run replace the file and make no further backup, so
  the `.bak` holds the file as it was before the run. The replacement keeps the original's
  permissions. If `FILE` is a link, the link is followed: its target's contents are replaced
  (written beside the target and renamed onto it), the link is kept, and the `.bak` is beside
  the link, named after it. A gzipped input is written gzipped.

`200 {"path": "<absolute path written>", "bytes": <body length>}`. Read-only:
`409 {"error": "…"}` and nothing written. The command prints `Saved <path> (<bytes> bytes).`
for each save.

### Heartbeat and `closed`

The engine runs in the page, so editing makes no requests. While the page is open, visible or
not, the app sends `GET <token>/config` every 30 s (the heartbeat; the answer may be used or
ignored). Any request resets the idle time and cancels a pending close.

`POST <token>/closed`, with the same `Origin` rule as `save` (any body, ignored; 204). Send it
with `navigator.sendBeacon` on `pagehide` only when `!event.persisted`: a page going into the
back/forward cache is not closed, and sends nothing. After `closed` the command stops once no
request has arrived for 90 s, printing `Closed.`; a reload, or another open tab's heartbeat,
keeps it running (90 s, because browsers run a long-hidden tab's timers about once a
minute). Without `closed` (a forgotten server, no page sending heartbeats) it stops
after 30 minutes with no request, printing `Stopped: no request for 30 minutes.`.

(Contract addition, after the first version: the heartbeat, the `!event.persisted` rule and
the 90 s delay, which was 3 s.)

## Which answers have a body

- A JSON `{"error": "…"}` body (`application/json`): every refusal of `POST <token>/save`
  (403, 409, 411, 413, 415, 400, 408, 500) and a `POST <token>/closed` from another origin
  (403).
- JSON: `config` and a successful `save` (200).
- No body (`Content-Length: 0`): `closed` (204), a wrong `Host` (403), a `GET` from another
  origin (403), any other token or path (404), a 404 under the token, a wrong method (405,
  with `Allow`), a request that cannot be read (400), the 308 redirect and 503.

## Rules for every request

- `Host` must be exactly `127.0.0.1:<port>` (403 otherwise; `localhost` is refused), so a page
  that reaches the port through DNS rebinding gets nothing.
- Any other token or path outside it: 404 with no body.
- A `GET` carrying an `Origin` other than the app's own: 403.
- Static files are only files inside the app or engine directory: `..`, `.`, empty segments,
  encoded slashes, backslashes and colons are refused, and a link leading outside is not
  followed (404).
- Every answer to the app's requests (app and engine files, `file`, `config`, `save`,
  `closed`, and a 404 under the token) carries `Cache-Control: no-store`,
  `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer` and

  ```text
  Content-Security-Policy: default-src 'self'; script-src 'self' 'wasm-unsafe-eval';
    style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:;
    connect-src 'self' blob:;
    worker-src 'self' blob:; object-src 'none'; base-uri 'none'; frame-ancestors 'none';
    form-action 'none'
  ```

  so the app must load nothing from elsewhere (no CDN fonts or scripts) and use no inline
  scripts.
- Content types: `.html` text/html, `.js`/`.mjs` text/javascript, `.wasm` application/wasm,
  `.css` text/css, `.json`/`.map` application/json, `.svg` image/svg+xml, `.png` image/png,
  `.jpg`/`.jpeg` image/jpeg, `.webp` image/webp, `.ico` image/x-icon, `.woff2` font/woff2,
  `.woff` font/woff, `.ttf` font/ttf, `.txt` text/plain; anything else
  application/octet-stream.
- A directory, with or without a trailing slash, is not served (404): there is no index but
  the app's own `index.html` at `<token>/`.
- The token is compared in constant time.
- At most 16 connections are answered at once; more get 503 with `Retry-After: 1`.

## Where the app and the engine come from

The app is not bundled with specify-core yet. Until it is:

- `--app DIR` (or `SPECIFY_APP_DIR`): the built app, `index.html` at its root. Without one,
  `open` refuses (exit 2) and says how to pass it.
- `--wasm DIR` (or `SPECIFY_WASM_DIR`): a wasm-pack `pkg/` directory served at `wasm/`.
  Without one, `wasm/` is served from the app directory, so an app that ships the engine in its
  own `wasm/` needs no `--wasm`.

A later release will download the app and the engine instead; the addresses above stay.
