/**
 * Two revisions of a STEP file in a GitHub repository, compared in the browser by Specify's
 * diff and drawn as the pull request comment's picture is: the new part, its faces coloured by
 * what changed, with the old part's gone or moved faces over it in translucent red. The STEP
 * diff action links its picture here: /diff?repo=owner/name&base=SHA&head=SHA&path=a/b.step
 * (&was=old/path.step for a renamed file). Public repositories only: raw.githubusercontent.com
 * serves them to any page.
 */

export interface DiffQuery { repo: string; base: string; head: string; path: string; was: string }

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
  const relative = (p: string) => p !== '' && !p.startsWith('/') && !p.split('/').some(s => s === '' || s === '.' || s === '..');
  if (!REPO.test(repo) || !SHA.test(base) || !SHA.test(head) || !relative(path) || !relative(was)) {
    throw new Error('This link does not name a repository, two commits and a STEP file.');
  }
  return { repo, base, head, path, was };
}

/** The file at a commit of a public repository, or null where it has none. */
export async function fileAt(repo: string, sha: string, path: string): Promise<Uint8Array | null> {
  const url = `https://raw.githubusercontent.com/${repo}/${sha}/${path.split('/').map(encodeURIComponent).join('/')}`;
  const response = await fetch(url);
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(`GitHub answered ${response.status} for ${path} at ${sha.slice(0, 7)}.`);
  return new Uint8Array(await response.arrayBuffer());
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
