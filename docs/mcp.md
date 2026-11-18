# MCP server: `specify-core mcp`

`specify-core mcp` lets an LLM client such as Claude Code or Claude Desktop specify a part: it
opens a STEP file, answers Specify's questions, reviews the result and writes AP242 PMI. The tools
call the same steps as `specify-core serve` (`src/api.rs`). The server is in `src/mcp.rs`.

The answers of an opened file are a **session** held by the server (`src/session.rs`), which
`show` also serves to the Specify page in the person's browser, so the agent and the person
edit one set of answers: see [session.md](session.md).

```text
specify-core mcp [--allow DIR]...                      stdio (the client starts it)
specify-core mcp --http [--port N] [--allow DIR]...    Streamable HTTP on 127.0.0.1
```

It speaks MCP revision **2026-07-28**, which is stateless: there is no handshake, and every
request carries `_meta` `io.modelcontextprotocol/protocolVersion` and `clientCapabilities`. It
also speaks the legacy **2025-11-25** revision, entered with `initialize`. It implements
`server/discover`, `tools/list`, `tools/call`, `resources/list`, `resources/templates/list` and
`resources/read`, and in the legacy era also `ping`. When a request names an unsupported
version, the error is `-32022`, and its `data.supported` lists `["2026-07-28", "2025-11-25"]`.

## Configure

**Claude Code** (stdio), from the directory that holds your parts:

```sh
claude mcp add specify -- specify-core mcp
```

(without `--allow` the server may use its working directory; add `--allow DIR` for others).
`show` serves the Specify app, which is not bundled yet: give the server `SPECIFY_APP_DIR` (and
`SPECIFY_WASM_DIR`) as `specify-core open` takes them (docs/open.md), for example
`claude mcp add specify -e SPECIFY_APP_DIR=/path/to/app -- specify-core mcp`.

**Claude Desktop** (stdio), in `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "specify": {
      "command": "/path/to/specify-core",
      "args": ["mcp", "--allow", "/Users/me/parts"]
    }
  }
}
```

Desktop starts the server in a directory of its own choosing. Always give `--allow`.

**HTTP**: run it yourself and give the client the endpoint and token it prints:

```sh
$ specify-core mcp --http --port 8765 --allow ~/parts
MCP endpoint: http://127.0.0.1:8765/mcp
Authorization: Bearer 3f9c…
Allowed directories: /Users/me/parts
```

```sh
claude mcp add --transport http specify http://127.0.0.1:8765/mcp \
  --header "Authorization: Bearer 3f9c…"
```

Without `--port`, the system picks a free port. The token is new on every start.

## Tools

Every tool has an `inputSchema` and an `outputSchema`. A result's `structuredContent` holds the
structured result, and its text content holds the same JSON. A refusal is a tool result with
`isError: true` and `structuredContent.error`: `{code, message, ...}`. The code is `invalid` (a
bad argument, or an answer Specify refuses; `question` names it), `refused` (a path outside
`--allow`, or an existing file without `overwrite`), `unknown_document`, `stale` (also an
in-place `save` over a file changed on disk by another program), `incomplete` (with `missing`)
or `failed`. Protocol errors, such as an unknown tool or bad params, are
JSON-RPC errors.

| Tool | Arguments | Result |
| --- | --- | --- |
| `list_parts` | `path` | `parts`: `[{part, name, product_id, faces, instances, ...}]`, as `serve`'s `parts` |
| `open` | `path`, `part?`, `old?`, `old_answers?`, `in_place?`, `answers_file?`, `overwrite?` | `doc`, `rev`, `open_questions`, `features` (counts by family), `faces`, `existing_pmi`, `resumed`, `warnings?`; with `old`, the comparison's `verdict`, `summary` and `report`. For an assembly opened without `part`, the result is `needs_part: true` with its `parts` |
| `questions` | `doc`, `state?` (`open` by default, `answered`, `all`) | each question as review presents it, with its `state` (`open`, `answered`, `optional`) and its `answer`; a default's `source` names the rule behind it ([rule-sources.md](rule-sources.md#rule-ids)); `rev` |
| `answer` | `doc`, `set?: {id: value or text}`, `clear?: [id]`, `answers_file?`, `overwrite?` | each answer as stored, with `interpreted_from` when it was typed, and its `state`; the `open` ids; the new `rev` |
| `review` | `doc` | `open`, `problems`, `writable`/`missing` as answered, `writable_with_defaults`/`missing_with_defaults`; `rev` |
| `characteristics` | `doc` | `intent`: the preview, as `serve`'s `preview`; `sources` (question ID to the rule behind each answer that is its question's default); `rev` |
| `read_pmi` | `doc` | `existing` (the file's PMI by face), `part_settings`, `resumed`, `saved_answers?`; `rev` |
| `write_pmi` | `doc`, `output`, `accept_defaults?`, `overwrite?` | `report`, as `serve`'s `write` |
| `export_answers` | `doc`, `output?`, `overwrite?` | `answers`: the complete sidecar (docs/answers.md); `rev` |
| `diff` | `old`, `new`, `part?`, `doc?` | `verdict`, `summary`, carried `answers`, `report`, `markdown`, as `specify-core diff` (nothing is written) |
| `changes` | `doc`, `since` (a `rev`) | `rev`, and `changes`: `[{rev, part, source, set, cleared}]` after `since`, oldest first (`source` `page` or `mcp`); if `since` is older than the 256 changes kept, `full: true` and `answers`, every part's answers by part index |
| `show` | `doc`, `browser?` (default `true`), `read_only?` | `url`, `mode`: the Specify page for the document's session on 127.0.0.1 (docs/open.md), started or, if it is running, its address; the browser opened unless `browser: false`. `read_only` makes that page view-only (not for a session opened with `old` or `in_place`). A running page in the other mode is refused (`invalid`, with its `url` and `mode`), never replaced |
| `save` | `doc`, `accept_defaults` (required) | `path`, `bytes`, `rev`, `report`, `complete`, `missing`: the session's answers written as the page's Save writes them, beside the file (or over it, opened `in_place`); open questions never refuse it |

`answer` reads free text for a choice question as `serve`'s `interpret` does. `"M6"` on a hole
becomes the matching `clearance:` or `tapped:` value, `"H7"` on a bore becomes a fit, and an
exact option is taken as it is. Answers are applied in order, each checked against the answers
before it. The call is all-or-nothing: if one answer is refused, nothing changes.

`diff` with `doc` carries that document's answers. The document must be of `old`.

`in_place` on a file whose session is already open changes where its Save writes only when the
call gives it. Under `open` the session keeps the path as given (made absolute, not resolved),
so Save goes beside a link and its `.bak` is named after the link. The directory a file is
named in must be inside `--allow` as well as the file itself (for `path`, `old` and
`old_answers`), and every save, from the agent or the page, is refused (`refused`, 403 from the
page) if it would write outside `--allow`.

`open` with `old` compares the two as `diff` does (the old answers from `old_answers`, else those
stored in `old`) and the session starts from the answers carried to `path`. `old_answers` needs
`old`.

## The session

Each opened file has one session, held in memory, which every part of it opened shares (see
[session.md](session.md)): its answers by part, a `rev` that goes up by one on every change
(from `answer`, `open` with an `answers_file`, or the page's edits), and the last 256 changes.
Opening the same file again (the same path, its contents as opened or as the session's own
in-place save left them, and the same `old`) returns its session, its documents and their
answers. Every tool that reads the answers reads the session as it is now,
page edits included, and returns `rev`; `changes` says what changed since a `rev` (MCP over
stdio cannot push to the agent, so it asks).

`show` starts the page server for the session (a random port and token) and opens the
browser; the page and the tools then share the answers, and the agent's changes reach the page
as `answers` events with `source: "mcp"`. The page server stops as docs/open.md says (90 s after
the page is closed, or 30 minutes without a request), and with the MCP server; `show` again
starts a new one. Under stdio its notices go to stderr.

`save` and the page's Save write the same way (docs/open.md, `save`): `write`'s writer, the
answers stored in the file, beside it as `<stem>.specified.step` (then `-2`, …) or over it when
opened `in_place` (keeping a `.bak`; every save writes from the file as opened). An incomplete
save is written; `complete` and `missing` say what is still open. `accept_defaults` must be
given: `true` only when the person asked for the defaults. A session's own in-place save does
not make its documents `stale`.

**Resources:** each open document has `specify://doc/{doc}/analysis` (the analysis JSON) and
`specify://doc/{doc}/mesh` (triangles by face index). Both are listed in `resources/list` and
described by two templates. Reading an unknown document returns `-32602` (`-32002` in the legacy
era).

## Documents and the memory rule

`open` returns a handle, `doc`. The handle is the SHA-256 of the part's STEP text and its part
index, for example `9b1c…e4-0`. Every later call passes it. The analysis and the answers are
held **in the server's memory, by handle**, and nothing is written unless you ask:

- **Content-addressed.** The handle depends on the file's contents, not its path. A
  byte-identical copy at another path opens to the same handle, and starts from the answers
  held for it; the handle then follows the path opened most recently (and its session).

- **Restart.** When the server stops, its documents and their answers are lost. A call with a
  forgotten handle is refused as `unknown_document`; open the file again.
- **`answers_file`.** Name a `.json` file on `open` or `answer` and the answers are saved to it
  after every change. If the file exists when `open` names it, the answers are loaded from it.
  The file is the same answer map `write`, `diff` and the editor read. An existing file that is
  not such a map (any other text or JSON) is refused, and left as it is, unless the call passes
  `overwrite: true`.
- **Resumed answers.** A file Specify wrote stores its answers, and `open` resumes them.
- **Stale files.** If the file at the recorded path changes after `open`, every call on the
  document is refused as `stale`. Open the file again, or use `diff` to carry the answers.

## Security

- **Paths.** Every path must lie inside an `--allow` directory. Without `--allow`, the server
  uses its working directory. Paths are canonicalised, symbolic links included, so a link or
  `..` that leads outside is refused. A file to be written must be in an existing allowed
  directory. The checks happen before the write and assume a trusted local machine: a symbolic
  link planted between the check and the write, by another local process with write access to
  the allowed directory, is not caught.
- **No overwriting.** `write_pmi` and `export_answers` refuse an existing `output` unless the
  call passes `overwrite: true`. An `answers_file` that already holds an answer map is replaced
  as it is kept up to date; any other existing file only with `overwrite: true`. It may never
  be a STEP file.
- **HTTP.** The server binds to 127.0.0.1 only. Every request needs:
  - `Authorization: Bearer <token>`; without it the answer is 401.
  - `Host: 127.0.0.1:<port>` or `localhost:<port>`; any other host gets 403, which stops DNS
    rebinding.
  - No `Origin` header; any browser `Origin` gets 403, so no web page can call the server.

  The scheme name `Bearer` is matched in any case. At most 16 connections are served at once;
  another is answered 503 and closed.

  2026-07-28 requests must also carry `MCP-Protocol-Version`, `Mcp-Method` and, for
  `tools/call` and `resources/read`, `Mcp-Name`, all matching the body; otherwise the answer is
  400 with `-32020`; so is a `2026-07-28` header on a body that carries no version. Other errors
  map to statuses (`-32601` is 404, an internal error 500, the rest 400). In the 2025-11-25
  era, as its transport says, a JSON-RPC error comes in a 200 body; only a bad or unsupported
  `MCP-Protocol-Version` is 400. The server never issues an `Mcp-Session-Id`. GET is answered 405. There is
  no OAuth and no hosted mode.
- **stdio.** Only protocol messages go to stdout. Diagnostics, such as a panic's message, go to
  stderr. A panic inside a tool call becomes a `failed` tool error; a panic anywhere else in
  serving a request (reading a resource, say) becomes a `-32603` internal error. Either way the
  server carries on.
