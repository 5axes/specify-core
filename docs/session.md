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

It starts from the answers stored in the file (a file Specify wrote resumes them), or, for a
diff, from the answers `diff` carried to the new revision. It lasts as long as the MCP server
process; nothing is written to disk unless a save or an `answers_file` asks for it (docs/mcp.md's
memory rule).

MCP's documents are parts (`doc`, one per part, as today). All the parts of one file share
its session and its `rev`; the page shows the file, and a part the page edits that the agent
has not opened is opened in the session when the page first changes it.

## MCP tools

The existing tools (docs/mcp.md) work on the session, plus three new ones:

| Tool | Arguments | Result |
| --- | --- | --- |
| `open` | as today, plus `old?` (a revision to compare from), `old_answers?` (the old answers' file; else those stored in `old`, else none), `in_place?` (on a session already open, changes where Save writes only when given) | as today, plus `rev`; with `old`, the diff's `verdict`, `summary` and `report` too, and the session starts from the carried answers |
| `questions`, `review`, `characteristics`, `read_pmi`, `export_answers` | as today | as today, plus `rev`: they read the session as it is now, page edits included |
| `answer` | as today | as today, plus the new `rev`; the page receives it as an `answers` event with `source: "mcp"` |
| `changes` (new) | `doc`, `since` (a `rev`) | `rev`, and `changes`: `[{rev, part, source, set: {id: answer}, cleared: [id]}]` after `since`, oldest first. If `since` is older than the changes kept, `full: true` and the whole `answers` instead (every part's, by part index, as the page's `GET answers` gives `parts`) |
| `show` (new) | `doc`, `browser?` (default `true`), `read_only?` | `url`, `mode`. Starts the page server for this session on 127.0.0.1 (random port and token), or returns the running one's address; opens the browser unless `browser: false`. `read_only: true` makes that page view-only (no answer changes or Save from the page); the agent can still answer. Not for a session opened with `old` or `in_place` (as `open` refuses `--read-only` with `--diff` or `--in-place`). A page already running in the other mode is never replaced: `show` is refused with its `url` and `mode` until the person has closed it |
| `save` (new) | `doc`, `accept_defaults` (required) | `path`, `bytes`, `rev`, `report`: writes the session's answers with the same writer and naming as the page's Save (docs/open.md); plus `complete` and `missing`. Open questions never refuse it: what is answered is written, as the page's Save does. The page gets the same `saved` event |

`write_pmi` stays for writing to a path of the agent's choosing; it does not change where
Save writes.

`accept_defaults` is never assumed: `save` requires it, and the agent sets it to `true` only
when the person asked for defaults (the page's Save likewise). An incomplete save is written
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
  session's own `in_place` save, which records what it wrote.
- The page server's rules (exact Host, token, Origin on changes, CSP, connection caps) are
  docs/open.md's, unchanged. There is no MCP endpoint over HTTP in the page server: the agent
  talks to the session over its own stdio connection only.
