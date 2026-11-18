# `specify-core open`: the local app contract

The Specify page serves the Specify app, the engine and one STEP file (two for a diff) from
this computer on 127.0.0.1 and opens the browser at the app. This page is the contract between
that page server (src/open.rs) and the app (built by specify-ui) that it serves.

The page server is started in one of two ways, with the same code and the same contract:

- **By an AI agent** (the usual way): Specify is set up once as the agent's stdio MCP server,
  `specify-core mcp`; the agent opens a part and calls the `show` tool, which starts the page
  server for that session and returns its address (docs/session.md).
- **By hand**, with no agent: `specify-core open FILE` or `specify-core open --diff OLD NEW`,
  below.

```text
specify-core open FILE [--read-only | --in-place] [--port N] [--no-browser]
                       [--app DIR] [--wasm DIR]
specify-core open --diff OLD NEW [--answers FILE] [--in-place] [--port N] [--no-browser]
                       [--app DIR] [--wasm DIR]
```

`FILE`, `OLD` and `NEW` are `.step` or `.stp` files, or either gzipped. `--read-only` and
`--in-place` cannot be combined; `--read-only` cannot be combined with `--diff`. Every path
the command shows or returns (`saveTarget`, a save's `path`, its notices) is the file as given,
made absolute, never resolved: a link stays the link, and nothing is shown in a canonical form
such as Windows' `\\?\C:\…`.

The run has one of three modes, given to the app as `config.mode`:

| Mode | Command | The page |
| --- | --- | --- |
| `specify` | `open FILE` | answers the part's questions and saves the specified STEP |
| `read-only` | `open FILE --read-only` | shows the part and its PMI; no answers change, no Save |
| `diff` | `open --diff OLD NEW` | shows what changed and what was carried; answers what needs a decision; Save writes `NEW` specified |

In `diff` mode everything below that speaks of "the file" or `FILE` means `NEW`; `OLD` is
only read.

The answers being edited live in the process that serves the page (the *session*, below, and
docs/session.md), not only in the page: under an agent, the MCP server process, so the agent
sees the page's answers and the page sees the agent's as they are made. In what follows, "the
command" is that process: `specify-core open`, or `specify-core mcp` after `show`.

## Addresses

Everything is under `http://127.0.0.1:<port>/<token>/`, where `<token>` is 32 lowercase hex
characters drawn from the operating system's random source for each run. The command prints

```text
Open part.step: http://127.0.0.1:<port>/<token>/
```

and opens the browser there (not with `--no-browser`). (Under an agent nothing is printed: the
`show` tool returns the address, and opens the browser unless asked not to.) `<token>`
without its trailing slash is redirected (308) to it. The app must use relative URLs only: it is served under the token, never
at `/`.

| Request | Answer |
| --- | --- |
| `GET <token>/` | the app's `index.html` |
| `GET <token>/<path>` | the static app: the file at `<path>` in the app directory |
| `GET <token>/wasm/<path>` | the engine: the file at `<path>` in the wasm-pack `pkg/` directory |
| `GET <token>/file` | the STEP file's bytes, as on disk (gzipped if the file is), `application/octet-stream` |
| `GET <token>/file?rev=old` | `diff` mode: `OLD`'s bytes; `?rev=new` (or no query) is `NEW`; any other `rev` is 404 |
| `GET <token>/config` | the app's settings, below; also the page's heartbeat |
| `GET <token>/answers` | the session's answers, below |
| `PUT <token>/answers` | replace one part's answers in the session, below |
| `GET <token>/events` | the session's changes as server-sent events, below |
| `GET <token>/diff` | `diff` mode: the comparison, as WASM `diff()` gives it, below |
| `POST <token>/save` | the specified STEP written to disk from the session's answers, below |
| `POST <token>/closed` | the page went away, below |

A query string is ignored, except `rev` on `file`. `file` serves the input as it was opened, or
as the last `--in-place` save left it. Outside `diff` mode `?rev=old` is 404, and `diff` is 404.

### `config`

```json
{
  "mode": "specify",
  "name": "part.step",
  "path": "/home/me/parts/part.step",
  "part": null,
  "oldName": null,
  "oldPath": null,
  "readOnly": false,
  "wasmUrl": "wasm/specify_core_wasm.js",
  "saveTarget": "/home/me/parts/part.specified.step",
  "specifyCore": "0.1.19"
}
```

- `mode`: `"specify"`, `"read-only"` or `"diff"` (above).
- `name`: the file's name (`NEW`'s in `diff` mode).
- `path`: the opened file's absolute path (`NEW`'s in `diff` mode), as given, not resolved
  (above).
- `part`: the part index the page is shown for: under an agent, the part (`doc`) `show` was
  called for, in a file of several parts; otherwise `null` (`specify-core open`, or a file of
  one part).
- `oldName`, `oldPath`: `OLD`'s name and absolute path in `diff` mode, otherwise `null`.
- `readOnly`: saves and answer changes are refused (`--read-only`); hide or disable Save.
  `true` exactly when `mode` is `"read-only"`.
- `wasmUrl`: the engine's entry module, relative to the app.
- `saveTarget`: the absolute path the next save would write (the input itself with
  `--in-place`), or `null` when read-only. It changes after each save. A save may still take
  another name if a file appears there first.
- `specifyCore`: this command's version.

### `save`

The command writes the STEP itself, from the session's answers, with the same writer as
`specify-core write` (the answers are stored in the file, so opening it again resumes them).
The page sends no STEP bytes.

`POST <token>/save` with a JSON body:

```json
{"rev": 7, "acceptDefaults": false}
```

- `rev`: the session revision the page last saw (from `answers` or `events`). If the session
  has moved on since, the save is refused (409, with the current `rev`) so the page never
  saves answers it has not shown; the page refreshes and the person saves again.
- `acceptDefaults` (required, `true` or `false`; 400 when absent): `true` fills each open
  question that has a default with it, as `write --accept-defaults` does; `false` writes only
  what is answered. The page sends `true` only when the person chose it (e.g. "Save with
  defaults"), never by default; the same holds for the agent's `save`.

**An incomplete save is written** (Paul, 2026-11-18: "The user might want to save something
with incomplete PMI. What's wrong with that? Lots of step files with ap242 don't have a complete
answer to everything."). Open questions never refuse a save: the file gets the PMI that is
answered, its stored answers are the session's, and reopening it resumes with the same
questions open. The answer says what is still open (`complete`, `missing`).
- `Content-Type: application/json` (415 otherwise), `Content-Length` and no
  `Transfer-Encoding` (411), at most 64 KB (413), the whole body within 30 s (408).
- `Origin: http://127.0.0.1:<port>` exactly (403 otherwise, and when `Origin` is absent).

Every part that has answers in the session (or is open in it, and a file's only part) is
written in one edit (an assembly's other parts are left as they are). The file is written to a temporary file next to the input and renamed
into place:

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
  the link, named after it. A gzipped input is written gzipped. Later saves still write from
  the file as it was opened, never on top of the PMI an earlier save added.

Answers:

- `200 {"path": "<absolute path written>", "bytes": <length written>, "rev": <n>, "report": {…},
  "complete": <bool>, "missing": ["<question id>", …]}`, `report` being `write`'s report (as
  `serve`'s `write`), `missing` the required questions still open after this save (empty, and
  `complete` true, when none are; in an assembly, those of every part written, in part order);
- `409 {"error": "…", "rev": <current>}`: the session moved on, or read-only, or (`--in-place`)
  the file was changed on disk by another program since it was opened (nothing written);
- `422 {"error": "…"}`: an answer the writer refuses (the message names the question); nothing
  written;
- `403 {"error": "…"}`: under an agent, the save would write outside the directories the agent's
  server may use (`--allow`); nothing written.

The command prints `Saved <path> (<bytes> bytes).` for each save.

(Contract change, 2026-11-18: until then the page wrote the STEP with its own engine and sent
the bytes as `application/octet-stream`. With the answers in the command, the command writes,
so a save made from the page and one asked for by an AI client write the same file.)

## The session: `answers` and `events`

The command holds the answers for the run: one answer map (as in `docs/answers.md`) per part,
by part index (`"0"` for a single part), and a revision number `rev` that goes up by one on
every change. At the start it holds:

- `specify` and `read-only`: the answers stored in `FILE`, when Specify wrote it (resume, as
  `analyse` reports them), otherwise none;
- `diff`: the answers the command's own `diff` carried to `NEW` (its `answers.json`, below).

`GET <token>/answers` →

```json
{"rev": 7, "parts": {"0": {"part.material": "6061-T6", "datum.A": [4]}}}
```

`PUT <token>/answers` (`Content-Type: application/json`, the `Origin` rule of `save`, at most
16 MB):

```json
{"rev": 7, "part": 0, "answers": {"part.material": "6061-T6", "datum.A": [4]}}
```

replaces part `part`'s map with `answers` when `rev` is the session's current revision:
`200 {"rev": 8}`. Otherwise `409 {"error": "…", "rev": <current>, "parts": {…}}`, and the page
reapplies its change to the current answers and tries again. Read-only: `409 {"error": "…"}`.
The body must be an object of question ids to answers (400 otherwise); the answers themselves
are checked when they are used (by the page's engine, `save`, and MCP's tools), not here. A
`part` the file does not have is refused (400).

`GET <token>/events` is a stream of server-sent events (`text/event-stream`, for
`EventSource`). On connecting it sends the current state (an `answers` event whose data is
`answers`' body with `"source": "start"`: `{"rev": 7, "parts": {…}, "source": "start"}`), then
one event per change:

```text
event: answers
id: 8
data: {"rev": 8, "part": 0, "answers": {…}, "source": "page"}
```

`source` is `"page"` (a `PUT answers`), `"mcp"` (the agent's tools, docs/session.md) or
`"start"` (the first event).
Every open page receives every change, its own included (it can skip a `rev` it already has).
A comment line (`: keep-alive`) is sent every 15 s. A stream that falls more than 1,000 events
behind is closed; reconnecting, it gets the full state. A reconnecting `EventSource` gets the full
current state again; `Last-Event-ID` is ignored. A `save` also sends

```text
event: saved
data: {"path": "…", "bytes": 12345, "rev": 8, "source": "page"}
```

so every page shows where the file was written, whoever asked.

At most 4 event streams are open at once (a fifth gets 503 with `Retry-After: 1`); they count
toward the 16 connections.

## `diff` mode

`specify-core open --diff OLD NEW` runs the same comparison as `specify-core diff OLD NEW`
when it starts, with the old answers from `--answers FILE`, or else those stored in `OLD`
(a file Specify wrote), or else none (the report then says what changed, with nothing to
carry).

**The carried answers come from the command's `diff`, not from the engine in the page.**
The session needs them before any page or MCP client asks, and the command's comparison is
the one `specify-core diff` and MCP's `diff` give, so the page, the AI and a CI run cannot
disagree. The page does not compare the parts again.

`GET <token>/diff` → exactly what WASM `diff()` returns for the two files (docs/wasm.md, the
`diff(...)` row), computed natively by the same code when the session starts: for a part
`{report, markdown, view}`, `report` being `diff`'s `report.json` (docs/diff.md), `markdown`
the report headed with `NEW`'s name, and `view` the new and old meshes, the `faces` classes,
`gone`, `motion` and `changes`; for an assembly `{report, markdown, parts}`. The page's StepDiff
draws from it without comparing again. (Contract change, 2026-11-18: `GET diff` was
report.json; it is now `{report, markdown, view}` (assembly `{report, markdown, parts}`), as
WASM `diff()`.) The session then starts from `diff`'s `answers.json`. Questions the report leaves to decide are
open questions in the session; answering them (page or MCP) changes the session, and the
report is not recomputed. Save writes `NEW` (beside it, or over it with `--in-place`) from the
session's answers.

## The agent's side

Under `specify-core mcp` the agent reads and changes the same session through MCP tools
(docs/session.md): its answers reach the page as `answers` events with `source: "mcp"`, and
the page's `PUT answers` are what its next call sees. Its `save` tool writes as the page's Save
does and sends the same `saved` event.

### Heartbeat and `closed`

The engine runs in the page, so looking at the part makes no requests (answer changes do:
`PUT answers`). While the page is open, visible or not, the app sends `GET <token>/config` every 30 s (the heartbeat; the answer may be used or
ignored). Any request resets the idle time and cancels a pending close.

`POST <token>/closed`, with the same `Origin` rule as `save` (any body, ignored; 204). Send it
with `navigator.sendBeacon` on `pagehide` only when `!event.persisted`: a page going into the
back/forward cache is not closed, and sends nothing. After `closed` the command stops once no
request has arrived and no event stream has been open for 90 s, printing `Closed.`; a reload, or another open tab's heartbeat,
keeps it running (90 s, because browsers run a long-hidden tab's timers about once a
minute). Without `closed` (a forgotten server, no page sending heartbeats) it stops
after 30 minutes with no request, printing `Stopped: no request for 30 minutes.`.

(Contract addition, after the first version: the heartbeat, the `!event.persisted` rule and
the 90 s delay, which was 3 s.)

Under an agent, "the command stops" means the page server stops: the MCP server and its
session carry on, nothing is printed on stdout (it carries the MCP protocol; notices go to
stderr), and the agent can call `show` again for a new address. When the MCP server exits, its
page servers stop with it.

## Which answers have a body

- A JSON `{"error": "…"}` body (`application/json`), with the extra fields given above: every
  refusal of `POST <token>/save` and `PUT <token>/answers` (400, 403, 408, 409, 411, 413, 415,
  422, 500), a refused `events` stream (503) and a `POST <token>/closed` from another origin
  (403).
- JSON: `config`, `answers`, `diff`, a successful `save` and `PUT answers` (200).
- `events`: `text/event-stream`, never closed by the command but at exit.
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
