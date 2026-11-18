// The page's side of the `specify-core open` contract (specify-core docs/open.md). Every URL is
// relative: the app is served under `http://127.0.0.1:<port>/<token>/`, never at `/`.

/** `GET config`. `mode` is the contract's next addition (diff); until the command sends it, it follows `readOnly`. */
export interface LocalConfig {
  name: string;
  readOnly: boolean;
  wasmUrl: string;
  saveTarget: string | null;
  specifyCore: string;
  /** Not in the contract yet: the file's absolute path, shown when the command sends it. */
  path?: string;
  mode?: Mode;
}
export type Mode = 'specify' | 'read-only' | 'diff';

export const modeOf = (config: LocalConfig): Mode => config.mode ?? (config.readOnly ? 'read-only' : 'specify');

async function failure(response: Response): Promise<Error> {
  let reason = `${response.status} ${response.statusText}`.trim();
  try {
    const body = await response.json() as { error?: string };
    if (body.error) reason = body.error;
  } catch { /* no body */ }
  return new Error(reason);
}

export async function getConfig(): Promise<LocalConfig> {
  const response = await fetch('config', { cache: 'no-store' });
  if (!response.ok) throw await failure(response);
  return response.json();
}

/** The STEP as on disk (gzipped if the file is). */
export async function getFile(): Promise<Blob> {
  const response = await fetch('file', { cache: 'no-store' });
  if (!response.ok) throw await failure(response);
  return response.blob();
}

/** Writes the specified STEP to disk; the path written and its size. An ArrayBuffer body: the
 * command needs `Content-Length` and refuses a chunked one. */
export async function save(bytes: Uint8Array): Promise<{ path: string; bytes: number }> {
  const body = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
  const response = await fetch('save', { method: 'POST', headers: { 'Content-Type': 'application/octet-stream' }, body });
  if (!response.ok) throw await failure(response);
  return response.json();
}

/** The heartbeat (`GET config` every 30 s while the page is open) and `closed` when it goes away
 * for good: not when it enters the back/forward cache. ``onConfig`` gets each heartbeat's answer. */
export function keepAlive(onConfig: (config: LocalConfig) => void): () => void {
  const timer = window.setInterval(() => { getConfig().then(onConfig, () => {}); }, 30_000);
  const gone = (event: PageTransitionEvent) => { if (!event.persisted) navigator.sendBeacon('closed'); };
  window.addEventListener('pagehide', gone);
  return () => { window.clearInterval(timer); window.removeEventListener('pagehide', gone); };
}
