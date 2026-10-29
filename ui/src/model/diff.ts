/**
 * Two revisions of a STEP file in a GitHub repository, compared in the browser by Specify's
 * diff and drawn as the pull request comment's picture is: the new part, its faces coloured by
 * what changed, with the old part's gone or moved faces over it in translucent red. The STEP
 * diff action links its picture here: /diff?repo=owner/name&base=SHA&head=SHA&path=a/b.step
 * (&was=old/path.step for a renamed file, &part=NAME for one part of an assembly, by its
 * name in the comparison's parts list); a pasted pull request or commit's address gives
 * the same link. Public repositories only: raw.githubusercontent.com and GitHub's API serve them
 * to any page. Without a link, the page compares two STEP files the person picks, in the
 * browser: they are never uploaded.
 *
 * Another page can hand it the two files (docs/DIFF-HANDOFF.md): it opens /diff, which posts
 * it `{ type: 'draftwright:diff-ready' }`, and answers with `handedFiles`'s message. The files
 * go from that page to this one in the browser, and are compared here like two picked ones.
 */

export interface DiffQuery { repo: string; base: string; head: string; path: string; was: string; part: string }

const REPO = /^[A-Za-z0-9-]+\/[A-Za-z0-9._-]+$/;
const SHA = /^[0-9a-f]{7,40}$/;

/** The query, checked: a repository, two commits and a relative path, nothing else fetched. */
export function diffQuery(search: string): DiffQuery {
  const query = new URLSearchParams(search);
  const repo = query.get('repo') ?? '';
  const base = query.get('base') ?? '';
  const head = query.get('head') ?? '';
  const path = query.get('path') ?? '';
  const was = query.get('was') || path;
  // A part's name, to show it: nothing is fetched by it.
  const part = query.get('part') ?? '';
  const relative = (p: string) => p !== '' && !p.startsWith('/') && !p.split('/').some(s => s === '' || s === '.' || s === '..');
  if (!REPO.test(repo) || !SHA.test(base) || !SHA.test(head) || !relative(path) || !relative(was) || part.length > 200) {
    throw new Error('This link does not name a repository, two commits and a STEP file.');
  }
  return { repo, base, head, path, was, part };
}

/** The file at a commit of a public repository, or null where it has none. */
export async function fileAt(repo: string, sha: string, path: string): Promise<Uint8Array | null> {
  const url = `https://raw.githubusercontent.com/${repo}/${sha}/${path.split('/').map(encodeURIComponent).join('/')}`;
  const response = await fetch(url);
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(`GitHub answered ${response.status} for ${path} at ${sha.slice(0, 7)}.`);
  return new Uint8Array(await response.arrayBuffer());
}

/** A pull request or a commit on GitHub, as its page's address names it. */
export type GithubLink = { repo: string } & ({ kind: 'pull'; number: number } | { kind: 'commit'; sha: string });

/** A pasted GitHub address (`github.com/owner/name/pull/12`, `…/commit/<sha>`), or null. */
export function githubLink(text: string): GithubLink | null {
  const m = text.trim().match(/^(?:https?:\/\/)?(?:www\.)?github\.com\/([A-Za-z0-9-]+\/[A-Za-z0-9._-]+)\/(pull|commit)\/([0-9a-fA-F]+)(?:[/?#].*)?$/);
  if (!m || !REPO.test(m[1]!)) return null;
  if (m[2] === 'pull') return /^\d+$/.test(m[3]!) ? { kind: 'pull', repo: m[1]!, number: Number(m[3]) } : null;
  return SHA.test(m[3]!.toLowerCase()) ? { kind: 'commit', repo: m[1]!, sha: m[3]!.toLowerCase() } : null;
}

/** A STEP file a link changed, as the diff page's own link names it (`?repo=…&path=…`). */
export interface ChangedStep { path: string; was: string; query: string }

const isStep = (path: string) => /\.(step|stp)$/i.test(path);

/**
 * The STEP files a pull request or commit changed (renamed or modified; not added or removed,
 * which have no other revision to compare), each with its diff link: a pull request between its
 * base and head, a commit against its first parent. Public repositories only: GitHub's API
 * answers them without a sign-in.
 */
export async function changedSteps(link: GithubLink): Promise<{ base: string; head: string; files: ChangedStep[] }> {
  const api = async <T>(path: string): Promise<T> => {
    const response = await fetch(`https://api.github.com/repos/${link.repo}/${path}`, { headers: { Accept: 'application/vnd.github+json' } });
    if (response.status === 404) throw new Error(`GitHub has no public ${link.kind === 'pull' ? 'pull request' : 'commit'} there: is the repository private?`);
    if (response.status === 403 || response.status === 429) throw new Error('GitHub is limiting requests from here for now. Try again in a while.');
    if (!response.ok) throw new Error(`GitHub answered ${response.status}.`);
    return response.json() as Promise<T>;
  };
  type File = { filename: string; status: string; previous_filename?: string };
  let base: string, head: string, files: File[];
  if (link.kind === 'pull') {
    const pull = await api<{ base: { sha: string }; head: { sha: string } }>(`pulls/${link.number}`);
    [base, head] = [pull.base.sha, pull.head.sha];
    files = await api<File[]>(`pulls/${link.number}/files?per_page=100`);
  } else {
    const commit = await api<{ sha: string; parents: { sha: string }[]; files?: File[] }>(`commits/${link.sha}`);
    if (!commit.parents[0]) throw new Error('That commit has no parent to compare it with.');
    [base, head, files] = [commit.parents[0].sha, commit.sha, commit.files ?? []];
  }
  const steps = files
    .filter(f => isStep(f.filename) && (f.status === 'modified' || f.status === 'renamed' || f.status === 'changed'))
    .map(f => {
      const was = f.previous_filename ?? f.filename;
      const query = new URLSearchParams({ repo: link.repo, base, head, path: f.filename, ...was !== f.filename ? { was } : {} });
      return { path: f.filename, was, query: `?${query}` };
    });
  return { base, head, files: steps };
}

/** A picked file's STEP text: a gzipped one (.step.gz) is unpacked here, in the browser. */
export async function stepBytes(file: Blob): Promise<Uint8Array> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  if (bytes[0] !== 0x1f || bytes[1] !== 0x8b) return bytes;
  const unpacked = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip'));
  return new Uint8Array(await new Response(unpacked).arrayBuffer());
}

const escape = (text: string) => text.replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);

/**
 * The report's Markdown as HTML: its headings, paragraphs, lists, tables, bold and code. Its
 * own heading and summary are left out: the page shows them above the part. Each "What changed"
 * line is numbered, `<li data-change="0">` and on, in the order of the view's `changes`.
 */
export function reportHtml(markdown: string): string {
  const inline = (text: string) => escape(text)
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  const html: string[] = [];
  const lines = markdown.split('\n');
  const first = lines.findIndex(line => line.trim() !== '');
  if (lines[first]?.startsWith('#')) {
    const summary = lines.findIndex((line, i) => i > first && line.trim() !== '');
    lines.splice(first, summary - first + 1);
  }
  let changes = false;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!;
    if (!line.trim() || line.startsWith('![')) continue;
    const listed = changes;
    changes = line.trim() === '**What changed**';
    if (line.startsWith('#')) {
      const level = Math.min(line.match(/^#+/)![0].length + 1, 6);
      html.push(`<h${level}>${inline(line.replace(/^#+\s*/, ''))}</h${level}>`);
    } else if (line.startsWith('|')) {
      const rows: string[][] = [];
      for (; i < lines.length && lines[i]!.startsWith('|'); i++) {
        const cells = lines[i]!.split('|').slice(1, -1).map(c => c.trim());
        if (!cells.every(c => /^-*$/.test(c))) rows.push(cells);
      }
      i--;
      const [head, ...body] = rows;
      html.push(`<table><thead><tr>${(head ?? []).map(c => `<th>${inline(c)}</th>`).join('')}</tr></thead><tbody>${
        body.map(r => `<tr>${r.map(c => `<td>${inline(c)}</td>`).join('')}</tr>`).join('')}</tbody></table>`);
    } else if (/^\s*- /.test(line)) {
      const items: string[] = [];
      for (; i < lines.length && /^\s*- /.test(lines[i]!); i++) {
        const text = inline(lines[i]!.replace(/^\s*- /, ''));
        items.push(listed ? `<li data-change="${items.length}" tabindex="0">${text}</li>` : `<li>${text}</li>`);
      }
      i--;
      html.push(`<ul>${items.join('')}</ul>`);
    } else html.push(`<p>${inline(line)}</p>`);
  }
  return html.join('\n');
}

/** What /diff posts the page that opened it, once it listens. */
export const HANDOFF_READY = 'draftwright:diff-ready';
/** A file another page hands /diff, read as a Blob. */
export interface Handed { name: string; data: Blob }

const handed = (file: unknown): Handed | null => {
  if (!file || typeof file !== 'object') return null;
  const { name, data } = file as { name?: unknown; data?: unknown };
  if (typeof name !== 'string' || !name.trim()) return null;
  const blob = data instanceof Blob ? data
    : data instanceof ArrayBuffer || ArrayBuffer.isView(data) ? new Blob([data as BlobPart]) : null;
  // A file's name only, for the page to show: no folders, and not too long.
  return blob ? { name: name.split(/[\\/]/).pop()!.slice(0, 200), data: blob } : null;
};

/**
 * The two files in a message another page posts /diff, or null if it is not one:
 * `{ type: 'draftwright:diff', before: { name, data }, after: { name, data } }`, each `data`
 * a Blob (or File), an ArrayBuffer or a typed array, of a STEP file or a gzipped one.
 */
export function handedFiles(message: unknown): { before: Handed; after: Handed } | null {
  if (!message || typeof message !== 'object' || (message as { type?: unknown }).type !== 'draftwright:diff') return null;
  const before = handed((message as { before?: unknown }).before);
  const after = handed((message as { after?: unknown }).after);
  return before && after ? { before, after } : null;
}
