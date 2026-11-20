# Specify sessions: the agent and the page together

Specify on a person's computer is **agent-first**. The person sets Specify up once as a
normal stdio MCP server in their AI agent, with no addresses or tokens:

```sh
claude mcp add specify -- specify-core mcp
```

The agent opens a part (or two revisions of it) through MCP, and Specify holds that part's
answers in the MCP server's process: the *session*. When the person wants to see the part, the
agent calls `show`, which serves the Specify page (docs/open.md) for that same session on
127.0.0.1 and opens the browser. From then on both sides edit one set of answers: the agent's
answers appear in the page as it makes them, and the person's edits in the page are what the
agent reads next.

`specify-core open FILE` and `specify-core open --diff OLD NEW` are the manual way in: the same
session and the same page, with no agent attached.

## The session

One session per opened file (per pair, for a diff), held in memory by the MCP server:

- the answers, one answer map (docs/answers.md) per part;
- `rev`, a number that goes up by one on every change, whoever makes it;
- the recent changes (the last 256), so a side can ask what changed since a `rev`;
- for a diff, the revision report;
- where Save writes: beside the file by default, or over it when the session was opened
  `in_place` (keeping a `.bak`), as docs/open.md's `save` says.

It starts from the answers stored in the file (a file Specify wrote resumes them), for a diff
too: nothing is carried from the old revision (ADR P11). It lasts as long as the MCP server
process; nothing is written to disk unless a save or an `answers_file` asks for it (docs/mcp.md's
memory rule).

MCP's documents are parts (`doc`, one per part, as today). All the parts of one file share
its session and its `rev`; the page shows the file, and a part the page edits that the agent
has not opened is opened in the session when the page first changes it.

## MCP tools

The existing tools (docs/mcp.md) work on the session, plus three new ones:

| Tool | Arguments | Result |
| --- | --- | --- |
| `open` | as today, plus `old?` (a revision to compare from), `in_place?` (on a session already open, changes where Save writes only when given) | as today, plus `rev`; with `old`, the diff's `verdict`, `summary` and `report` too; the session is the file's own, nothing carried |
| `questions`, `review`, `characteristics`, `read_pmi`, `export_answers` | as today | as today, plus `rev`: they read the session as it is now, page edits included |
| `answer` | as today | as today, plus the new `rev`; the page receives it as an `answers` event with `source: "mcp"` |
| `changes` (new) | `doc`, `since` (a `rev`) | `rev`, and `changes`: `[{rev, part, source, set: {id: answer}, cleared: [id]}]` after `since`, oldest first. If `since` is older than the changes kept, `full: true` and the whole `answers` instead (every part's, by part index, as the page's `GET answers` gives `parts`) |
| `show` (new) | `doc`, `browser?` (default `true`), `read_only?` | `url`, `mode`. Starts the page server for this session on 127.0.0.1 (random port and token), or returns the running one's address; opens the browser unless `browser: false`. A session opened with `old` is shown in `diff` mode: the page shows what changed beside the new file, and the person may answer and save the new revision. `read_only: true` makes that page view-only (no answer changes or Save from the page); the agent can still answer. Only `read_only: true` is refused for a session opened with `old` or `in_place` (as `open` refuses `--read-only` with `--diff` or `--in-place`). A page already running in the other mode is never replaced: `show` is refused with its `url` and `mode` until the person has closed it |
| `save` (new) | `doc`, `accept_defaults` (required: the defaults the person approved, each id with the default value they saw: `{"<part index>": {"<id>": <value>}}` as the page sends them, or `{"<id>": <value>}` meaning the `doc`'s own part; `{}` for none) | `path`, `bytes`, `rev`, `report`: writes the session's answers with the same writer and naming as the page's Save (docs/open.md); plus `complete` and `missing`. Open questions never refuse it: what is answered is written, as the page's Save does. The page gets the same `saved` event |

`write_pmi` stays for writing to a path of the agent's choosing; it does not change where
Save writes.

A default is written only when the person approves it, per id (ADR P9: "we shouldn't create
the default, which then becomes gospel"). `save` requires `accept_defaults`, exactly the defaults
the person approved, each with the value the person saw (`{}` for none): the agent names only
those the person saw and confirmed, never every default unless the person saw the list and
confirmed it. All are checked against the defaults as they stand before any is applied: each
must be an open question with a default on the part it is listed for, and its value that
default (else an `invalid` error naming the part and the id, with `expected` and `current`
when the value differs; nothing written). They are applied together, and only that part gets
them. They become answers (a
new `rev`, an `answers` event with `"defaults": {"<part>": [ids]}`), are stored in the file, and
the rest stay open, unwritten. A save writes only the parts that have at least one answer (keys
starting with `_` not counting) or that the session changed since they joined (so a clear
reaches the file); any other part is left as it is, and with nothing to write the file is saved
as it was opened. MCP's `write_pmi` and `export_answers` (and the session-free exports: `api::export_answers`,
CLI `questions --resolve-defaults --accept-defaults FILE`, serve, WASM `exportAnswers` and
Python, by part only) follow the same rule for defaults:
they write or export what is answered plus exactly the `accept_defaults` given, with the values
the person saw, checked against the current defaults; without them, no default, and open
questions are reported (`complete`, `missing`), never filled. A part joining the session
(opened by the agent, or first changed by the page) is a change: a new `rev` and an event with
`"joined": true`. The page's Save sends the same object as `acceptDefaults`
(docs/open.md). (Contract change, 2026-11-18: `accept_defaults` was `true` or `false`, then
`{"<part>": [ids]}`; it is now `{"<part>": {"<id>": <value>}}`.) An incomplete save is written
(docs/open.md, `save`): a STEP file with some requirements still open is a valid result.

An agent that wants to follow the person's edits calls `changes` with the last `rev` it saw
(for example after the person says they are done, or before each step); MCP over stdio has no
way to push to the agent unasked.

## Lifetimes

- **The MCP server process** owns the sessions. When it exits, they are gone (unless saved or
  kept in an `answers_file`) and any page server it started stops.
- **The page server** runs while a page is open. After the page closes it stops as
  docs/open.md says (90 s after `closed`, or 30 minutes with no request). `show` can be called
  again at any time, for example after the tab was closed, and starts a new one (a new address).
  Several sessions may each have a page.
- **`specify-core open`** holds one session and one page server, and exits when the page
  server stops.

## Rules kept from MCP and the page

- Paths: `open`, `save` and the rest stay inside `--allow` (docs/mcp.md). Save writes beside
  the file, which is inside it.
- Stale files: a file changed on disk after `open` is refused as `stale`, except by the
  session's own `in_place` save, which records what it wrote. A `save` during which the
  session's set of parts changed (the page's first change to another part, which an agent's
  save has no `rev` to notice) is refused as `stale` too, nothing written: read and save again.
- A save whose write fails after its approvals were applied rolls them back (a further `rev`),
  unless the session has moved on since; nothing is written.
- The page server's rules (exact Host, token, Origin on changes, CSP, connection caps) are
  docs/open.md's, unchanged. There is no MCP endpoint over HTTP in the page server: the agent
  talks to the session over its own stdio connection only.
