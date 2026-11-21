// What a revision report (`GET diff`, `specify-core diff`'s report.json, docs/diff.md) leaves to
// the person: the new file's requirements on a feature that moved or changed size (`check`).
// Diff reads the two files' PMI only (ADR P11): it carries no answers and asks no questions, so a
// check names no question. (`unresolved` and `questions` are read too, from older reports.)

export interface Decision {
  kind: 'unresolved' | 'check' | 'specify';
  /** The question it is about, when the report names one. */
  question: string | null;
  /** A short label, and why it needs a decision. */
  what: string;
  why: string;
}

type Row = Record<string, unknown>;
const rows = (value: unknown): Row[] => (Array.isArray(value) ? value.filter((r): r is Row => Boolean(r) && typeof r === 'object') : []);
const text = (value: unknown) => (typeof value === 'string' ? value : '');

export function decisions(report: unknown): Decision[] {
  const r = (report ?? {}) as Row;
  const id = (row: Row) => text(row.id) || null;
  return [
    ...rows(r.unresolved).map((row): Decision => ({
      kind: 'unresolved', question: id(row),
      what: `Not carried: ${text(row.question) || text(row.title) || text(row.id)}`,
      why: text(row.reason) || 'This requirement could not be applied to the new revision.',
    })),
    ...rows(r.check).map((row): Decision => ({
      kind: 'check', question: null,
      what: `Check: ${text(row.requirement) || text(row.id)}`,
      why: 'Its feature moved or changed size, so it may no longer fit.',
    })),
    ...rows(r.questions).map((row): Decision => ({
      kind: 'specify', question: id(row),
      what: `New: ${text(row.title) || text(row.question) || text(row.id)}`,
      why: 'A new question in this revision.',
    })),
  ];
}
