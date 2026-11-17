# `specify-core open`: the local app contract

`specify-core open FILE` serves the Specify app, the engine and one STEP file from this
computer and opens the browser at the app. This page is the contract between the command
(src/open.rs) and the app (built by specify-ui) that it serves.

```text
specify-core open FILE [--read-only | --in-place] [--port N] [--no-browser]
                       [--app DIR] [--wasm DIR]
```

`FILE` is a `.step` or `.stp` file, or either gzipped. `--read-only` and `--in-place` cannot be
combined.

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
| `GET <token>/config` | the app's settings, below |
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
  `--in-place`), or `null` when read-only. It changes after each save.
- `specifyCore`: this command's version.

### `save`

`POST <token>/save` with the STEP file as the body:

- `Content-Type: application/octet-stream` (415 otherwise) and `Content-Length` (411 otherwise);
- `Origin: http://127.0.0.1:<port>` exactly (403 otherwise, and when `Origin` is absent);
- at most min(4 × the input's STEP text, 512 MB) (413, before the body is read; a gzipped input
  is measured unzipped);
- a STEP Part 21 file: `ISO-10303-21;` after an optional UTF-8 BOM and whitespace (400
  otherwise).

The file is written to a temporary file next to the input and renamed into place:

- by default under the first free name of `<stem>.specified.step`, `<stem>.specified-2.step`,
  `<stem>.specified-3.step`, … (`<stem>` is the name without `.gz` and `.step`/`.stp`). An
  existing file is never overwritten, even by a concurrent save;
- with `--in-place` over the input, after the original is copied to the first free name of
  `<file>.bak`, `<file>.bak-2`, …. A gzipped input is written gzipped.

`200 {"path": "<absolute path written>", "bytes": <body length>}`. Read-only:
`409 {"error": "…"}` and nothing written. Every refusal of a save has a JSON `{"error"}` body.
The command prints `Saved <path> (<bytes> bytes).` for each save.

### `closed`

`POST <token>/closed`, with the same `Origin` rule as `save` (any body, ignored; 204). Send it
with `navigator.sendBeacon` on `pagehide`. The command stops 3 s later unless another request
arrives first (a reload), printing `Closed.`. Without `closed` it stops after 30 minutes with no
request.

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
    style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self' blob:;
    worker-src 'self' blob:; object-src 'none'; base-uri 'none'; frame-ancestors 'none';
    form-action 'none'
  ```

  so the app must load nothing from elsewhere (no CDN fonts or scripts) and use no inline
  scripts.
- Content types: `.html` text/html, `.js`/`.mjs` text/javascript, `.wasm` application/wasm,
  `.css` text/css, `.json`/`.map` application/json, `.svg` image/svg+xml, `.png` image/png,
  `.woff2` font/woff2, `.txt` text/plain; anything else application/octet-stream.
- At most 16 connections are answered at once; more get 503 with `Retry-After: 1`.

## Where the app and the engine come from

The app is not bundled with specify-core yet. Until it is:

- `--app DIR` (or `SPECIFY_APP_DIR`): the built app, `index.html` at its root. Without one,
  `open` refuses (exit 2) and says how to pass it.
- `--wasm DIR` (or `SPECIFY_WASM_DIR`): a wasm-pack `pkg/` directory served at `wasm/`.
  Without one, `wasm/` is served from the app directory, so an app that ships the engine in its
  own `wasm/` needs no `--wasm`.

A later release will download the app and the engine instead; the addresses above stay.
