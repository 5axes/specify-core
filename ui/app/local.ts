// The page's side of the `specify-core open` contract (specify-core docs/open.md): the page server
// started by `specify-core open` or by the MCP server's `show`. Every URL is relative: the app is
// served under `http://127.0.0.1:<port>/<token>/`, never at `/`.
import type { PmiAnswers } from '../src/types/pmi';
import { requireSessionApi } from './session-api.ts';

export type Mode = 'specify' | 'read-only' | 'diff';

/** `GET config`. */
export interface LocalConfig {
  /** Session API version; absent in specify-core 0.2.0. */
  api?: number;
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

export const getConfig = (signal?: AbortSignal, onResponse?: () => void) => json<LocalConfig>(
  fetch('config', { cache: 'no-store', signal }).then((response) => { onResponse?.(); return response; }),
).then((config) => { requireSessionApi(config.api); return config; });

/** The STEP as on disk (gzipped if the file is); in diff mode ``old`` is OLD's; ``saved`` is what
 * the session's last save wrote (404 before one, 409 if it changed on disk since). */
export async function getFile(rev: 'old' | 'new' | 'saved' = 'new'): Promise<Blob> {
  const response = await fetch(`file?rev=${rev}`, { cache: 'no-store' });
  if (!response.ok) throw await failure(response);
  return response.blob();
}

export const getAnswers = () => json<SessionAnswers>(fetch('answers', { cache: 'no-store' }));

/** Replaces part ``part``'s answers if the session is still at ``rev``: the new rev. A 409 is
 * thrown as `Refused` with the current ``rev`` and ``parts``. */
export const putAnswers = (rev: number, part: number, answers: PmiAnswers, signal?: AbortSignal) => json<{ rev: number }>(fetch('answers', {
  method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ rev, part, answers }), signal,
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
export const save = (rev: number, acceptDefaults: AcceptDefaults, signal?: AbortSignal) => json<Saved>(fetch('save', {
  method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ rev, acceptDefaults }), signal,
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

/** Heartbeat every 30 s, retrying unreachable or busy commands every 4 s. A command is stopped
 * after a full 25 s failure window. Every HTTP response clears that window. Keep probing to recover.
 * A visible page returning, or a timer gap over 10 s, starts a new observation window. */
export function keepAlive(onConfig: (config: LocalConfig) => void, onStopped: (stopped: boolean) => void, onClose: () => void) {
  let closed = false;
  let stopped = false;
  let failedSince: number | null = null;
  let lastTick = Date.now();
  let nextProbe = lastTick + 30_000;
  let active: { controller: AbortController; started: number } | null = null;
  let refreshPending = false;
  let streamProbe = 0;
  const status = (value: boolean) => {
    if (stopped === value) return;
    stopped = value;
    onStopped(value);
  };
  const resumed = (now: number) => {
    failedSince = null;
    nextProbe = now;
    // Give an in-flight probe a fresh response window too. A stale timer must never abort a
    // healthy request made after wake, or disable Save before that request answers.
    if (active) active.started = now;
    lastTick = now;
  };
  const observeTime = () => {
    const now = Date.now();
    if (now < lastTick || now - lastTick > 10_000) resumed(now);
    lastTick = now;
    return now;
  };
  const probe = () => {
    const now = observeTime();
    if (closed || active) return;
    const request = { controller: new AbortController(), started: now, reached: false };
    active = request;
    const finished = (alive: boolean, config?: LocalConfig) => {
      const at = observeTime();
      if (closed || active !== request) return;
      active = null;
      if (alive) {
        failedSince = null;
        status(false);
        if (config) onConfig(config);
      } else if (!request.reached) {
        failedSince ??= request.started;
        if (at - failedSince >= 25_000) status(true);
      }
      nextProbe = at + (config ? 30_000 : 4_000);
      if (refreshPending) {
        refreshPending = false;
        probe();
      }
    };
    getConfig(request.controller.signal, () => {
      if (closed || active !== request) return;
      // Headers alone prove reachability, including 503 or a response whose body cannot be read.
      observeTime();
      request.reached = true;
      failedSince = null;
      status(false);
    }).then((config) => finished(true, config), () => finished(false));
  };
  const tick = () => {
    const now = observeTime();
    if (active && now - active.started >= 5_000) active.controller.abort();
    if (!active && failedSince !== null && now - failedSince >= 25_000) status(true);
    if (!active && now >= nextProbe) probe();
  };
  // Navigation can cancel SSE before pagehide runs. Defer this connection until pagehide can
  // cancel it, so it cannot arrive after the close beacon.
  const streamLost = () => {
    if (closed || streamProbe) return;
    streamProbe = window.setTimeout(() => { streamProbe = 0; probe(); }, 250);
  };
  const refresh = () => { if (active) refreshPending = true; else probe(); };
  const visible = () => {
    if (document.visibilityState !== 'visible' || closed) return;
    resumed(Date.now());
    probe();
  };
  const timer = window.setInterval(tick, 1_000);
  const gone = (event: PageTransitionEvent) => {
    if (event.persisted) return;
    stop();
    onClose();
    navigator.sendBeacon('closed');
  };
  window.addEventListener('pagehide', gone);
  document.addEventListener('visibilitychange', visible);
  const stop = () => {
    closed = true;
    active?.controller.abort();
    window.clearInterval(timer);
    window.clearTimeout(streamProbe);
    window.removeEventListener('pagehide', gone);
    document.removeEventListener('visibilitychange', visible);
  };
  return { streamLost, refresh, stop };
}
