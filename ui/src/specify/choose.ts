// Choosing an answer by what the feature is for or by its code, and finding a
// material as it is typed. Pure, no DOM.
import type { PmiChoice, PmiQuestion, PmiSuggestion } from '../types/pmi';

export interface Purpose {
  readonly purpose: string;
  readonly label: string;
  readonly choices: readonly PmiChoice[];
}

/** ``q``'s choices by purpose, in the order offered, "nothing in particular" last. */
export function purposes(q: PmiQuestion): Purpose[] {
  const out = new Map<string, { purpose: string; label: string; choices: PmiChoice[] }>();
  for (const choice of q.choices ?? []) {
    const group = out.get(choice.purpose) ?? { purpose: choice.purpose, label: choice.label, choices: [] };
    group.choices.push(choice);
    out.set(choice.purpose, group);
  }
  const all = [...out.values()];
  return [...all.filter((p) => p.purpose !== 'general'), ...all.filter((p) => p.purpose === 'general')];
}

/** The purpose an answer serves: its choice's, or for a typed answer, its kind's. */
export function purposeOf(q: PmiQuestion, value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const choice = q.choices?.find((c) => c.value === value);
  if (choice) return choice.purpose;
  if (value === 'general') return 'general';
  const kind = value.split(':')[0] ?? '';
  if (q.id.startsWith('hole.function:')) return kind;
  return kind === 'thread' || kind === 'knurl' ? kind : 'fit';
}

/** An answer as an engineer names it: its choice's code, else as given. */
export function codeOf(q: PmiQuestion, value: unknown): string {
  if (typeof value !== 'string') return '';
  const choice = q.choices?.find((c) => c.value === value);
  if (choice) return choice.code;
  const [kind, spec] = value.split(':');
  if (spec === undefined) return value;
  return kind === 'fit' ? spec : `${spec} ${kind}`;
}

/**
 * Suggestions matching ``query``: every word of it found in the name, its
 * family or another name it goes by -- "delrin" finds POM, "1.4301" finds 304.
 * Names that start with what was typed come first, then names containing it,
 * then matches on another name only. Empty ``query``: everything, in order.
 */
export function matchSuggestions(
  suggestions: readonly PmiSuggestion[], query: string, limit = 40,
): PmiSuggestion[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return suggestions.slice(0, limit);
  const scored: { s: PmiSuggestion; rank: number; at: number }[] = [];
  suggestions.forEach((s, at) => {
    const name = s.value.toLowerCase();
    const haystack = [name, s.group.toLowerCase(), ...s.also.map((a) => a.toLowerCase())].join(' ');
    if (!words.every((w) => haystack.includes(w))) return;
    const phrase = words.join(' ');
    const rank = name.startsWith(phrase) ? 0 : name.includes(phrase) ? 1 : words.every((w) => name.includes(w)) ? 2 : 3;
    scored.push({ s, rank, at });
  });
  return scored.sort((a, b) => a.rank - b.rank || a.at - b.at).slice(0, limit).map((x) => x.s);
}
