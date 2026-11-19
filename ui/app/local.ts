// The page's side of the `specify-core open` contract (specify-core docs/open.md): the page server
// started by `specify-core open` or by the MCP server's `show`. Every URL is relative: the app is
// served under `http://127.0.0.1:<port>/<token>/`, never at `/`.
import type { PmiAnswers } from '../src/types/pmi';

export type Mode = 'specify' | 'read-only' | 'diff';

/** `GET config`. */
export interface LocalConfig {
  mode: Mode;
  /** The file's name (NEW's in diff mode), and its absolute path as given. */
  name: string;
  path: string;
  /** The part index the page is shown for (an agent's `show` on a part of a file of several); otherwise null. */
  part: number | null;
  /** OLD's, in diff mode; otherwise null. */
  oldName: string | null;
  oldPath: string | null;
  readOnly: boolean;
  wasmUrl: string;
  /** Where the next save writes; null when read-only. */
  saveTarget: string | null;
  specifyCore: string;
}

/** The session's answers: one answer map per part, by part index ("0" for a single part). */
export interface SessionAnswers { rev: number; parts: Record<string, PmiAnswers> }
export type Source = 'page' | 'mcp' | 'start';
/** An `answers` event: the start state (``parts``) or one part's change. */
export type AnswersEvent =
  | { rev: number; parts: Record<string, PmiAnswers>; source: 'start' }
  | {
    rev: number; part: number; answers: PmiAnswers; source: 'page' | 'mcp';
    /** A save approved these defaults, by part: question ids now answered with their defaults. */
    defaults?: Record<string, string[]>;
    /** The part joined the session (the agent opened it, or the page first changed it); its answers may be `{}`. */
    joined?: boolean;
  };
export interface SavedEvent { path: string; bytes: number; rev: number; source: 'page' | 'mcp' }
/** A save's answer: what was written, and the required questions still open in it. */
export interface Saved { path: string; bytes: number; rev: number; report: unknown; complete: boolean; missing: string[] }

/** A refusal: the command's `error`, its status, and the body's other fields (``rev``, ``parts``). */
export class Refused extends Error {
  readonly status: number;
  readonly body: Record<string, unknown>;
  constructor(message: string, status: number, body: Record<string, unknown>) { super(message); this.status = status; this.body = body; }
}

async function failure(response: Response): Promise<Refused> {
  let reason = `${response.status} ${response.statusText}`.trim();
  let body: Record<string, unknown> = {};
  try {
    body = await response.json() as Record<string, unknown>;
    if (typeof body.error === 'string') reason = body.error;
  } catch { /* no body */ }
  return new Refused(reason, response.status, body);
}

async function json<T>(request: Promise<Response>): Promise<T> {
  const response = await request;
  if (!response.ok) throw await failure(response);
  return response.json() as Promise<T>;
}

export const getConfig = (signal?: AbortSignal) => json<LocalConfig>(fetch('config', { cache: 'no-store', signal }));

/** The STEP as on disk (gzipped if the file is); in diff mode ``old`` is OLD's. */
export async function getFile(rev: 'old' | 'new' = 'new'): Promise<Blob> {
  const response = await fetch(rev === 'old' ? 'file?rev=old' : 'file?rev=new', { cache: 'no-store' });
  if (!response.ok) throw await failure(response);
  return response.blob();
}

export const getAnswers = () => json<SessionAnswers>(fetch('answers', { cache: 'no-store' }));

/** Replaces part ``part``'s answers if the session is still at ``rev``: the new rev. A 409 is
 * thrown as `Refused` with the current ``rev`` and ``parts``. */
export const putAnswers = (rev: number, part: number, answers: PmiAnswers) => json<{ rev: number }>(fetch('answers', {
  method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ rev, part, answers }),
}));

/** The defaults the person approved, each with the value they saw, by part index:
 * `{"<part>": {"<question id>": <value>}}`; `{}` for none. */
export type AcceptDefaults = Record<string, Record<string, unknown>>;

/** The command writes the specified STEP from the session's answers at ``rev``, after setting
 * ``acceptDefaults`` (exactly the defaults the person approved, per part) as answers. Never refused
 * for open questions; 409 when the session moved on (or its set of parts changed during the save),
 * or (--in-place) the file changed on disk; 400 `{error, part, question}` when a default listed is
 * not an open question with a default there, plus `expected` (the value sent) and `current` (the
 * default now) when its value is no longer that default. A failed write rolls its approvals back. */
export const save = (rev: number, acceptDefaults: AcceptDefaults) => json<Saved>(fetch('save', {
  method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ rev, acceptDefaults }),
}));

/** Diff mode: the command's comparison, exactly what WASM `diff()` returns with the PMI scenes
 * (specify-core docs/open.md): a part's `{report, markdown, view}`, an assembly's `{report, markdown, parts}`;
 * the scenes are `report.pmi.scenes` (an assembly's, each compared part's). */
export interface DiffBody { report: unknown; markdown: string; view?: unknown; parts?: unknown[] }
export const getDiff = () => json<DiffBody>(fetch('diff', { cache: 'no-store' }));

/** The session's changes as they happen (the start state first, again on each reconnect), and
 * each save, whoever asked for it. ``onLive(false)`` when the stream is lost or refused (a 503:
 * the command serves at most 4 streams), ``onLive(true)`` when it is back: it is reopened, waiting
 * 1 s, 2 s, 4 s … up to 30 s between tries. */
export function listen(onAnswers: (event: AnswersEvent) => void, onSaved: (event: SavedEvent) => void, onLive: (live: boolean) => void = () => {}): () => void {
  let source: EventSource | null = null;
  let timer = 0;
  let wait = 1000;
  let stopped = false;
  const connect = () => {
    const stream = new EventSource('events');
    source = stream;
    stream.addEventListener('open', () => { wait = 1000; onLive(true); });
    stream.addEventListener('answers', (event) => onAnswers(JSON.parse((event as MessageEvent<string>).data) as AnswersEvent));
    stream.addEventListener('saved', (event) => onSaved(JSON.parse((event as MessageEvent<string>).data) as SavedEvent));
    stream.addEventListener('error', () => {
      onLive(false);
      // The browser retries a dropped stream itself; a refused one (an HTTP error) it gives up on.
      if (stream.readyState !== EventSource.CLOSED || stopped) return;
      stream.close();
      timer = window.setTimeout(connect, wait);
      wait = Math.min(wait * 2, 30_000);
    });
  };
  connect();
  return () => { stopped = true; window.clearTimeout(timer); source?.close(); };
}

/** Heartbeat every 30 s, with an immediate probe when SSE fails. A refused stream (e.g. all four
 * slots taken) is recoverable while config still answers. Retry failed probes for 25 s. */
export function keepAlive(onConfig: (config: LocalConfig) => void, onStopped: () => void) {
  let stopped = false;
  let checking = false;
  let deadline = 0;
  let retry = 0;
  const probe = () => {
    if (stopped || checking) return;
    checking = true;
    getConfig(AbortSignal.timeout(5_000)).then((config) => {
      checking = false;
      if (stopped) return;
      window.clearTimeout(deadline);
      window.clearTimeout(retry);
      deadline = 0;
      onConfig(config);
    }, () => {
      checking = false;
      if (stopped) return;
      if (!deadline) deadline = window.setTimeout(() => { stop(); onStopped(); }, 25_000);
      window.clearTimeout(retry);
      retry = window.setTimeout(probe, 4_000);
    });
  };
  const timer = window.setInterval(probe, 30_000);
  // A page entering the back/forward cache can come back.
  const gone = (event: PageTransitionEvent) => { if (!event.persisted) navigator.sendBeacon('closed'); };
  window.addEventListener('pagehide', gone);
  const stop = () => {
    stopped = true;
    window.clearInterval(timer);
    window.clearTimeout(deadline);
    window.clearTimeout(retry);
    window.removeEventListener('pagehide', gone);
  };
  return { probe, stop };
}
