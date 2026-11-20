// What pmi-assist chose for the person, said plainly: each default it picked
// that nobody has kept or changed, with why, and what it applied by rule.
// Pure, no DOM.
import type { PmiAnalysis, PmiAnswers, PmiIntent, PmiQuestion } from '../types/pmi';

import { codeOf, purposes } from './choose.ts';
import { describeFaces } from './datums.ts';
import { groupQuestions, needs } from './review.ts';

export interface Chosen {
  readonly id: string;
  /** What was chosen, in a line: "Datum A: bottom face". */
  readonly what: string;
  readonly why: string;
  /** What it is about, without the choice: "Datum A", "6× Ø6.6 holes". */
  readonly about: string;
  /** The choice and a few alternatives, the choice first; empty for a datum. */
  readonly options: readonly { readonly value: string; readonly label: string }[];
  /** Whether there are more alternatives than shown. */
  readonly more: boolean;
  /** A default pmi-assist doubts: a person should look before keeping it. */
  readonly check: boolean;
  /** Its name within its row: "What for", "Position", "Datum A", "Edges". */
  readonly field: string;
}

/** Alternatives shown beside a choice, beyond it. */
const SHOWN = 4;

function optionsOf(q: PmiQuestion, chosen: unknown): { options: Chosen['options']; more: boolean } {
  if (q.kind !== 'choice' || typeof chosen !== 'string') return { options: [], more: false };
  const all = q.options ?? [];
  // A hole's alternatives: one per purpose (the likeliest code of each), not
  // every fit; anything else: its options in order.
  const candidates = q.choices?.length
    ? purposes(q).map((p) => p.choices.find((c) => c.value === chosen)?.value ?? p.choices[0]?.value ?? '')
    : [...all];
  const others = candidates.filter((v) => v && v !== chosen).slice(0, SHOWN);
  const options = [chosen, ...others].map((value) => ({ value, label: label(q, value) }));
  return { options, more: all.length > options.length };
}

function label(q: PmiQuestion, value: string): string {
  if (q.id.startsWith('hole.position:')) return value === 'none' ? 'none' : `⌀${value}`;
  if (q.id.startsWith('part.general_tolerance')) return value.replace('ISO 2768-', '2768-');
  if (q.id === 'part.edges') return value.replace('Break sharp edges ', 'Break ').replace(' max', '').replace('No edge note', 'No note');
  return codeOf(q, value);
}

const PART: Record<string, string> = {
  'part.general_tolerance': 'General tolerance', 'part.surface_finish': 'Surface finish',
  'part.edges': 'Edges', 'part.locating': 'Holes located by',
};

/**
 * The defaults still standing that pmi-assist chose and can say why, and those ``answeredForYou``
 * (left to a default or answered on the person's behalf, still to review) as they stand.
 */
export function chosenForYou(
  questions: readonly PmiQuestion[], answers: PmiAnswers, analysis: PmiAnalysis, answeredForYou: readonly string[] = [],
): Chosen[] {
  const titles = new Map(groupQuestions(questions).map((g) => [g.key, g.title]));
  const out: Chosen[] = [];
  for (const q of questions) {
    const forYou = answeredForYou.includes(q.id);
    const value = q.id in answers ? answers[q.id] : q.default;
    const empty = value === null || value === undefined || (Array.isArray(value) && !value.length);
    // A choice it can explain, or a default it doubts (with the reason it doubts it); one to
    // review it cannot explain is doubted too.
    const check = needs(q, answers) === 'check' || (forYou && !q.basis);
    if ((!q.basis && !check) || (q.id in answers && !forYou) || empty) continue;
    const title = (q.feature && titles.get(q.feature)) ?? '';
    const letter = /^datum\.([A-H])$/.exec(q.id)?.[1];
    let what: string;
    if (letter) what = `Datum ${letter}: ${describeFaces(value as number[], analysis)}`;
    else if (q.id in PART) what = `${PART[q.id]}: ${String(value)}`;
    else if (q.id.startsWith('hole.function:') || q.id.startsWith('diameter.fit:')) what = `${title}: ${codeOf(q, value)}`;
    else if (q.id.startsWith('hole.position:')) what = `${title}: position ⌀${String(value)}`;
    else if (q.id.startsWith('hole.frame:')) what = `${title}: measured from ${String(value)}`;
    else what = `${q.label ?? q.prompt}: ${String(value)}`;
    const about = letter ? `Datum ${letter}` : q.id in PART ? PART[q.id] as string
      : q.id.startsWith('hole.position:') ? `${title} position` : title || (q.label ?? q.prompt);
    const why = check && q.reason ? q.reason : q.basis ?? q.why ?? '';
    const field = letter ? `Datum ${letter}` : q.id in PART ? PART[q.id] as string
      : q.id.startsWith('hole.function:') ? 'What for'
        : q.id.startsWith('hole.position:') ? 'Position'
          : q.id.startsWith('hole.frame:') ? 'Measured from' : about;
    out.push({ id: q.id, what, why, about, check, field, ...optionsOf(q, value) });
  }
  return out;
}

/** What pmi-assist applied by rule, with no question to answer. */
export function appliedByRule(intent: PmiIntent | null): string[] {
  const reqs = intent?.requirements ?? [];
  const out: string[] = [];
  // A pattern is one requirement for many holes: the holes are not counted.
  if (reqs.some((r) => r['mmc'])) {
    out.push('Clearance holes are positioned at Ⓜ (maximum material): '
      + 'the zone grows as the hole does, and a gauge pin checks it.');
  }
  const basic = reqs.filter((r) => r.kind === 'location' && r['basic']);
  if (basic.length) {
    out.push(`${basic.length} basic dimension${basic.length === 1 ? '' : 's'} added from the datums, `
      + 'to say exactly where each positioned hole should be.');
  }
  return out;
}
