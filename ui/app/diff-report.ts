// What a revision report (`GET diff`, `specify-core diff`'s report.json, docs/diff.md) leaves to
// the person: requirements that could not be carried (`unresolved`), carried ones to check
// (`check`) and new questions to specify (`questions`). Each opens its question in the answers.

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
      kind: 'check', question: id(row),
      what: `Check: ${text(row.question) || text(row.id)}`,
      why: `Carried as ${JSON.stringify(row.answer)}; its feature changed, so it may no longer fit.`,
    })),
    ...rows(r.questions).map((row): Decision => ({
      kind: 'specify', question: id(row),
      what: `New: ${text(row.title) || text(row.question) || text(row.id)}`,
      why: 'A new question in this revision.',
    })),
  ];
}
