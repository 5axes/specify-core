# MCP server: `specify-core mcp`

`specify-core mcp` lets an LLM client such as Claude Code or Claude Desktop specify a part: it
opens a STEP file, answers Specify's questions, reviews the result and writes AP242 PMI. The tools
call the same steps as `specify-core serve` (`src/api.rs`). The server is in `src/mcp.rs`.

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
claude mcp add specify -- specify-core mcp --allow "$PWD"
```

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
`--allow`, or an existing file without `overwrite`), `unknown_document`, `stale`, `incomplete`
(with `missing`) or `failed`. Protocol errors, such as an unknown tool or bad params, are
JSON-RPC errors.

| Tool | Arguments | Result |
| --- | --- | --- |
| `list_parts` | `path` | `parts`: `[{part, name, product_id, faces, instances, ...}]`, as `serve`'s `parts` |
| `open` | `path`, `part?`, `answers_file?`, `overwrite?` | `doc`, `open_questions`, `features` (counts by family), `faces`, `existing_pmi`, `resumed`, `warnings?`. For an assembly opened without `part`, the result is `needs_part: true` with its `parts` |
| `questions` | `doc`, `state?` (`open` by default, `answered`, `all`) | each question as review presents it, with its `state` (`open`, `answered`, `optional`) and its `answer` |
| `answer` | `doc`, `set?: {id: value or text}`, `clear?: [id]`, `answers_file?`, `overwrite?` | each answer as stored, with `interpreted_from` when it was typed, and its `state`; the `open` ids |
| `review` | `doc` | `open`, `problems`, `writable`/`missing` as answered, `writable_with_defaults`/`missing_with_defaults` |
| `characteristics` | `doc` | `intent`: the preview, as `serve`'s `preview` |
| `read_pmi` | `doc` | `existing` (the file's PMI by face), `part_settings`, `resumed`, `saved_answers?` |
| `write_pmi` | `doc`, `output`, `accept_defaults?`, `overwrite?` | `report`, as `serve`'s `write` |
| `export_answers` | `doc`, `output?`, `overwrite?` | `answers`: the complete sidecar (docs/answers.md) |
| `diff` | `old`, `new`, `part?`, `doc?` | `verdict`, `summary`, carried `answers`, `report`, `markdown`, as `specify-core diff` (nothing is written) |

`answer` reads free text for a choice question as `serve`'s `interpret` does. `"M6"` on a hole
becomes the matching `clearance:` or `tapped:` value, `"H7"` on a bore becomes a fit, and an
exact option is taken as it is. Answers are applied in order, each checked against the answers
before it. The call is all-or-nothing: if one answer is refused, nothing changes.

`diff` with `doc` carries that document's answers. The document must be of `old`.

**Resources:** each open document has `specify://doc/{doc}/analysis` (the analysis JSON) and
`specify://doc/{doc}/mesh` (triangles by face index). Both are listed in `resources/list` and
described by two templates. Reading an unknown document returns `-32602` (`-32002` in the legacy
era).

## Documents and the memory rule

`open` returns a handle, `doc`. The handle is the SHA-256 of the part's STEP text and its part
index, for example `9b1c…e4-0`. Every later call passes it. The analysis and the answers are
held **in the server's memory, by handle**, and nothing is written unless you ask:

- **Content-addressed.** The handle depends on the file's contents, not its path. A
  byte-identical copy at another path opens to the same handle, and so shares the answers held
  in memory; the handle then follows the path opened most recently.

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
