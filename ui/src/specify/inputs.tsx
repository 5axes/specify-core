// The lab's answer inputs: choosing by purpose or by code, typing a spec, and
// a material box that suggests as it is typed but takes any text.
import { type ReactNode, useEffect, useId, useMemo, useRef, useState } from 'react';

import type { PmiChoice, PmiQuestion, PmiSuggestion } from '../types/pmi';

import { KIND_TEXT, MAY_HAVE_DATUM, NEEDS_DATUM, type ExtraKind } from './added';
import { byBalloon, type Characteristic, inspectKey } from './characteristics';
import type { Chosen } from './chosen';
import { fmt } from './labels';

import { codeOf, matchSuggestions, purposeOf, purposes } from './choose';

const SPEC_EXAMPLE: readonly (readonly [string, string])[] = [
  ['hole.function:', 'e.g. H7, M5, clearance M6'],
  ['diameter.fit:', 'e.g. g6, M10, diamond knurl'],
  ['hole.position:', 'e.g. 0.15'],
  ['hole.location:', 'e.g. ±0.03'],
];

/**
 * What the feature is for, as large buttons naming the code each gives, and
 * every code in a row beneath for someone who knows which they want.
 */
export function PurposeChoice({ q, value, onAnswer, onPoint }: {
  q: PmiQuestion; value: unknown; onAnswer: (value: string) => void; onPoint: (value: string | null) => void;
}) {
  const at = purposeOf(q, value);
  const choices = q.choices ?? [];
  return (
    <>
      <div className="pmi-lab-purposes">
        {purposes(q).map((p) => {
          const on = p.purpose === at;
          const first = p.choices[0] as PmiChoice;
          return (
            <button
              key={p.purpose} type="button" className={on ? 'on' : ''}
              onClick={() => { if (!on) onAnswer(first.value); }}
              onMouseEnter={() => onPoint(first.value)} onMouseLeave={() => onPoint(null)}
            >
              <span className="label">{p.label}</span>
              <span className="code">
                {on ? codeOf(q, value) : first.code}
                {!on && p.choices.length > 1 ? ' …' : ''}
              </span>
            </button>
          );
        })}
      </div>
      <div className="opts codes" onMouseLeave={() => onPoint(null)}>
        {choices.map((c) => (
          <button
            key={c.value} type="button" onClick={() => onAnswer(c.value)}
            onMouseEnter={() => onPoint(c.value)} onFocus={() => onPoint(c.value)} onBlur={() => onPoint(null)}
            className={`${c.value === value ? 'on' : ''}${c.value === q.default ? ' default' : ''}`}
          >
            {c.code}
          </button>
        ))}
      </div>
    </>
  );
}

/** A spec typed for ``q``, read by pmi-assist; ``onSpec`` resolves to why it was refused, or null. */
export function SpecBox({ q, codes, onSpec }: {
  q: PmiQuestion; codes: readonly string[]; onSpec: (text: string) => Promise<string | null>;
}) {
  const [text, setText] = useState('');
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const list = useId();
  const example = SPEC_EXAMPLE.find(([prefix]) => q.id.startsWith(prefix))?.[1] ?? '';
  const submit = async () => {
    const typed = text.trim();
    if (!typed || busy) return;
    setBusy(true);
    const refused = await onSpec(typed);
    setBusy(false);
    setProblem(refused);
    if (!refused) setText('');
  };
  return (
    <form className="pmi-lab-spec" onSubmit={(event) => { event.preventDefault(); void submit(); }}>
      <div className="row">
        <input
          type="text" value={text} list={list} placeholder={example || 'Or type a value'}
          aria-label={`Type a spec: ${q.prompt}`} autoCapitalize="off" autoCorrect="off" spellCheck={false}
          onChange={(event) => { setText(event.target.value); setProblem(null); }}
        />
        <button type="submit" disabled={!text.trim() || busy}>{busy ? '…' : 'Use'}</button>
      </div>
      <datalist id={list}>{codes.map((code) => <option key={code} value={code} />)}</datalist>
      {problem && <p role="alert" className="pmi-lab-error">{problem}</p>}
    </form>
  );
}

/**
 * A text box that suggests as it is typed -- by family, matching other names
 * a material goes by -- and takes whatever is typed if nothing suits.
 */
export function SuggestBox({ label, value, suggestions, placeholder, invalid, onAnswer }: {
  label: string; value: string; suggestions: readonly PmiSuggestion[]; placeholder: string; invalid: boolean;
  onAnswer: (value: string) => void;
}) {
  const [text, setText] = useState(value);
  // The host project can change the value through undo/redo.
  useEffect(() => setText(value), [value]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const list = useId();
  // Opened on what is already chosen, everything is offered to browse.
  const matches = useMemo(
    () => matchSuggestions(suggestions, text === value ? '' : text),
    [suggestions, text, value],
  );
  const commit = (chosen: string) => {
    const typed = chosen.trim();
    setText(typed);
    setOpen(false);
    setActive(-1);
    if (typed && typed !== value) onAnswer(typed);
  };
  const move = (step: number) => {
    setOpen(true);
    setActive((at) => Math.max(-1, Math.min(matches.length - 1, at + step)));
  };
  let group = '';
  return (
    <div className="pmi-lab-suggest">
      <input
        type="text" role="combobox" value={text} placeholder={placeholder} aria-label={label}
        className={invalid ? 'invalid' : ''}
        aria-expanded={open && matches.length > 0} aria-controls={list} aria-autocomplete="list"
        aria-activedescendant={active >= 0 ? `${list}-${active}` : undefined}
        onFocus={() => setOpen(true)}
        onChange={(event) => { setText(event.target.value); setOpen(true); setActive(-1); }}
        // Leaving the box keeps a catalogue choice; anything else typed waits for Enter, so a
        // half-typed "alu" is not taken as the answer.
        onBlur={() => {
          const typed = text.trim();
          if (suggestions.some((s) => s.value.toLowerCase() === typed.toLowerCase())) commit(suggestions.find((s) => s.value.toLowerCase() === typed.toLowerCase())!.value);
          else setOpen(false);
        }}
        onKeyDown={(event) => {
          if (event.key === 'ArrowDown') { event.preventDefault(); move(1); }
          else if (event.key === 'ArrowUp') { event.preventDefault(); move(-1); }
          else if (event.key === 'Escape') { setText(value); setOpen(false); setActive(-1); }
          else if (event.key === 'Enter' || event.key === 'Tab') {
            // Enter and Tab both mean "done here": Tab then moves on as usual.
            if (event.key === 'Enter') event.preventDefault();
            commit(open && active >= 0 ? matches[active]?.value ?? text : text);
          }
        }}
      />
      {open && matches.length > 0 && (
        <ul id={list} role="listbox" className="pmi-lab-suggestions">
          {matches.map((s, i) => {
            const heading = s.group !== group ? s.group : null;
            group = s.group;
            return (
              <li
                key={s.value} id={`${list}-${i}`} role="option" aria-selected={i === active}
                className={`${i === active ? 'active' : ''}${heading ? ' first' : ''}`}
                data-group={heading ?? undefined}
                // Chosen before the box's blur commits what was typed.
                onMouseDown={(event) => { event.preventDefault(); commit(s.value); }}
              >
                <span className="name">{s.value}</span>
                {s.also.length > 0 && <span className="also">{s.also.slice(0, 3).join(' · ')}</span>}
              </li>
            );
          })}
        </ul>
      )}
      {!open && text.trim() !== value && text.trim() !== '' && <small className="pmi-lab-pending">Press Enter to use “{text.trim()}”, or Esc to go back.</small>}
    </div>
  );
}

/** The tolerances a call-out is most often given, one tap each. */
export const TOLERANCE_PICKS = ['±0.1', '±0.05', '±0.02', '+0.1/-0', '+0/-0.1'];

export interface ExtraDraft {
  readonly key: string;
  readonly kind: ExtraKind | '';
  readonly value: string;
  readonly datums: readonly string[];
  /** A distance's flat faces it is measured from. */
  readonly reference: readonly number[];
  /** What a dimension called out of the general tolerance is: "⌀8", "24 from datum A". */
  readonly callOut?: string;
  /** The faces it starts on, when it is called out. */
  readonly faces?: readonly number[];
}

/**
 * Adding a requirement where no question asked: faces picked on the part,
 * then what they take -- only what suits them -- its value, and for an
 * orientation the datum it is measured from. A distance is picked twice:
 * the flat faces it is measured from, then what it runs to.
 */
export function ExtraEditor({
  draft, selection, kinds, letters, newLetter, error, editing, onChange, onSave, onRemove, onCancel, onPickFrom,
}: {
  draft: ExtraDraft; selection: readonly number[]; kinds: readonly ExtraKind[];
  /** The datums the part has, and the letter a new one would take. */
  letters: readonly string[]; newLetter: string | null;
  error: string | null; editing: boolean; onChange: (draft: ExtraDraft) => void; onSave: () => void;
  onRemove: () => void; onCancel: () => void;
  /** Takes the faces picked as what a distance is measured from, and clears the pick. */
  onPickFrom: () => void;
}) {
  // A dimension called out from what the general tolerance covers keeps its kind.
  // So does a distance whose faces it is measured from are picked, while its target is.
  const kind = draft.kind && (
    kinds.includes(draft.kind) || draft.callOut || (draft.kind === 'distance' && draft.reference.length)
  ) ? draft.kind : '';
  const needsDatum = Boolean(kind) && NEEDS_DATUM.includes(kind as ExtraKind);
  const oriented = Boolean(kind) && MAY_HAVE_DATUM.includes(kind as ExtraKind);
  const distance = kind === 'distance';
  const ready = Boolean(
    kind && draft.value.trim() && (!needsDatum || draft.datums.length) && (!distance || draft.reference.length),
  );
  return (
    <div className="pmi-lab-item on pmi-lab-extra">
      <div className="prompt">
        {draft.callOut ? `Call out ${draft.callOut}` : editing ? 'Change this requirement' : 'Add a requirement'}
      </div>
      {draft.callOut && <div className="why">Give it a tolerance of its own, instead of the general one.</div>}
      {/* A call-out's faces are its dimension's, named in its prompt: no faces to pick or show. */}
      {distance && !draft.callOut && (
        <div className="faces">
          {draft.reference.length ? `from faces ${draft.reference.join(', ')}` : 'Pick the flat face(s) it is measured from, then:'}
          {!draft.callOut && (
            <button type="button" disabled={!selection.length} onClick={onPickFrom}>Measure from these</button>
          )}
        </div>
      )}
      {!draft.callOut && (
        <div className="faces">
          {selection.length
            ? `${distance ? 'to ' : ''}faces ${selection.join(', ')}`
            : distance && draft.reference.length ? 'Now pick the face(s) or hole it runs to.'
              : 'Click the face(s), hole or boss on the part.'}
        </div>
      )}
      {selection.length > 0 && !kinds.length && (
        <div className="why">Pick flat faces, or the cylinder(s) of one hole or boss.</div>
      )}
      {kinds.length > 0 && !draft.callOut && (
        <div className="opts">
          {kinds.map((k) => (
            <button
              key={k} type="button" className={k === kind ? 'on' : ''}
              // One that needs a datum starts on the first there is (or a new A);
              // a profile starts with none, as it may well want none.
              onClick={() => onChange({
                ...draft,
                kind: k,
                reference: k === 'distance' ? draft.reference : [],
                datums: NEEDS_DATUM.includes(k)
                  ? (draft.datums.length ? draft.datums : [letters[0] ?? newLetter ?? 'A'])
                  : k === 'profile' ? [] : draft.datums,
              })}
            >
              {KIND_TEXT[k]}
              {!letters.length && NEEDS_DATUM.includes(k) && <small> · adds a datum</small>}
            </button>
          ))}
        </div>
      )}
      {kind && (
        <div className="pmi-lab-spec">
          <div className="row">
            <input
              type="text" value={draft.value}
              aria-label={kind === 'fit' ? 'Fit' : kind === 'finish' ? 'Roughness (Ra, µm)' : 'Tolerance (mm)'}
              placeholder={kind === 'fit' ? 'e.g. H7 for a hole, g6 for a shaft'
                : kind === 'finish' ? 'Ra in µm, e.g. 0.8'
                  : kind === 'size' ? '0.05 for ±0.05, +0.1/-0, or a fit such as H7'
                    : kind === 'distance' ? '0.05 for ±0.05, or +0.1/-0' : 'tolerance in mm, e.g. 0.05'}
              autoCapitalize="off" autoCorrect="off" spellCheck={false}
              onChange={(event) => onChange({ ...draft, value: event.target.value })}
            />
            {kind !== 'fit' && kind !== 'finish' && <span className="tolerance-unit">mm</span>}
          </div>
          {/* No ± key on most keyboards: the usual ones are a tap, and 0.05 alone means ±0.05. */}
          {(kind === 'size' || kind === 'distance') && (
            <div className="opts tolerances">
              {[...TOLERANCE_PICKS, ...(kind === 'size' ? ['H7', 'g6'] : [])].map((t) => (
                <button
                  key={t} type="button" className={draft.value.trim() === t ? 'on' : ''}
                  onClick={() => onChange({ ...draft, value: t })}
                >
                  {t}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
      {oriented && (
        <>
          <div className="opts">
            <span className="why">{needsDatum ? 'Measured from' : 'Measured from (optional: none controls form only)'}</span>
            {[...letters, ...(newLetter ? [newLetter] : [])].map((x) => {
              const at = draft.datums.indexOf(x);
              return (
                <button
                  key={x} type="button" className={at >= 0 ? 'on' : ''}
                  onClick={() => onChange({
                    ...draft,
                    datums: at >= 0 ? draft.datums.filter((d) => d !== x) : [...draft.datums, x].slice(0, 3),
                  })}
                >
                  {letters.includes(x) ? x : `+ new datum ${x}`}
                </button>
              );
            })}
          </div>
          {draft.datums.length > 1 && <div className="why">In order: {draft.datums.join(' | ')}</div>}
          {draft.datums.filter((x) => !letters.includes(x)).map((x) => (
            <div key={x} className="why">
              {'ABC'.includes(x)
                ? `Datum ${x} will be added, proposed from the part; check it under Datums.`
                : `Datum ${x} will be added; pick its faces under Datums next.`}
            </div>
          ))}
        </>
      )}
      {error && <p role="alert" className="pmi-lab-error">{error}</p>}
      <div className="opts">
        <button
          type="button" className="ok" disabled={!ready || !(draft.callOut ? draft.faces?.length : selection.length)}
          onClick={onSave}
        >
          {editing ? 'Save' : 'Add'}
        </button>
        {editing && <button type="button" onClick={onRemove}>Remove</button>}
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
}

/**
 * A choice pmi-assist made, in the row it belongs to: what it is about, the
 * choice ticked beside a few alternatives -- one tap keeps or changes it --
 * and why it was chosen, or why it is doubted.
 */
export function SuggestionCard({ c, onKeep, onPick, onMore }: {
  c: Chosen; onKeep: () => void; onPick: (value: string) => void; onMore: () => void;
}) {
  return (
    <div className={`pmi-lab-suggestion${c.check ? ' check' : ''}`}>
      <div className="field">{c.field}{c.check && <em> · please check</em>}</div>
      {/* Not an answer until it is kept: no tick, and a Keep that makes it one. */}
      <div className="suggested" data-suggestion={c.id}>
        <span>Suggested: <b>{c.what.split(': ').slice(1).join(': ') || c.options[0]?.label || c.what}</b></span>
        <button type="button" className="keep" title="Keep this suggestion as the answer" onClick={onKeep}>Keep</button>
      </div>
      <div className="btns">
        {c.options.length ? (
          <>
            {c.options.slice(1).map((o) => (
              <button key={o.value} type="button" title={`Choose ${o.label} instead`} onClick={() => onPick(o.value)}>{o.label}</button>
            ))}
            {c.more && <button type="button" className="more" onClick={onMore}>More…</button>}
          </>
        ) : (
          <button type="button" className="more" onClick={onMore}>Pick on the part…</button>
        )}
      </div>
      {c.why && <div className="why">{c.why}</div>}
    </div>
  );
}

/**
 * Every characteristic, numbered as its balloon: a row chosen is shown on the
 * part. One row per balloon, opened to one per feature it is on, as an
 * inspector lists them. What the general tolerance covers is one row, opened
 * on demand: there may be a hundred, and few of them matter.
 */
export function CharacteristicsTable({
  rows, selected, onSelect, onDownload, skipped, onSkip, onEdit, onCallOut, generalTolerance, calledOut, readOnly = false, exportable = true,
}: {
  /** A view of what is set: no tick boxes, edit or call-out column, or CSV. */
  readOnly?: boolean;
  /** Hosts owning saved-file exports suppress preview downloads and CSV inclusion controls. */
  exportable?: boolean;
  rows: readonly Characteristic[]; selected: string | null;
  onSelect: (id: string, faces: readonly number[]) => void; onDownload: () => void;
  /** What is left out of the inspection list (``inspectKey``), and leaving out or back. */
  skipped: ReadonlySet<string>; onSkip: (keys: readonly string[], skip: boolean) => void;
  /** Opens where a row's PMI is changed; absent where it cannot be. */
  onEdit: (row: Characteristic) => (() => void) | null;
  /** Calls a dimension the general tolerance covers out of it, to tolerance it apart. */
  onCallOut: (row: Characteristic) => (() => void) | null;
  /** The general tolerance ("ISO 2768-m"), and how many dimensions are called out of it. */
  generalTolerance: string; calledOut: number;
}) {
  // A balloon on several features is one row, opened to show each of them.
  const [open, setOpen] = useState<ReadonlySet<number>>(new Set());
  // Opened or closed by hand; until then, a general dimension chosen on the part opens them.
  const [generalOpen, setGeneralOpen] = useState<boolean | null>(null);
  const showGeneral = generalOpen ?? rows.some(
    (r) => r.source.startsWith('general:') && (r.id === selected || `${r.n}` === selected),
  );
  // A row chosen on the part is brought into sight.
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    box.current?.querySelector('tr.on')?.scrollIntoView?.({ block: 'nearest' });
  }, [selected]);
  const n = (v: number | null, sign = false) => (v === null ? '' : `${sign && v > 0 ? '+' : ''}${fmt(v)}`);
  const groups = byBalloon(rows);
  const firstGeneral = groups.findIndex((group) => group[0]?.source.startsWith('general:'));
  const left = rows.filter((r) => skipped.has(inspectKey(r))).length;
  // A balloon is inspected or not as a whole; ``balloon`` is given on its first row.
  const line = (
    key: string, r: Characteristic, cells: { bal: ReactNode; feature: ReactNode }, faces: readonly number[],
    toggle?: () => void, balloon?: readonly Characteristic[],
  ) => {
    const keys = (balloon ?? [r]).map(inspectKey);
    const out = keys.every((k) => skipped.has(k));
    const edit = balloon ? onEdit(r) : null;
    const callOut = balloon && !edit ? onCallOut(r) : null;
    return (
    <tr
      key={key} className={`${key === selected ? 'on' : ''}${r.sub && !toggle ? ' sub' : ''}${out ? ' skip' : ''}`} tabIndex={0}
      onClick={() => { toggle?.(); onSelect(key, faces); }}
      onKeyDown={(e) => { if (e.key === 'Enter') { toggle?.(); onSelect(key, faces); } }}
    >
      {!readOnly && exportable && (
        <td onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
          {balloon && (
            <input
              type="checkbox" checked={!out} aria-label={r.n ? `Inspect ${r.n}` : `Inspect ${r.text}`} title="In the inspection list (CSV)"
              onChange={(e) => onSkip(keys, !e.target.checked)}
            />
          )}
        </td>
      )}
      <td>{cells.bal}</td>
      <td>{cells.feature}</td>
      <td className="text">{r.text}</td>
      <td>{n(r.nominal)}</td>
      <td className="tol">
        {n(r.upper, true)}
        {r.lower !== null && <><br />{fmt(r.lower)}</>}
      </td>
      <td>{r.datums}</td>
      {!readOnly && <td onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
        {edit && (
          <button type="button" className="edit" aria-label={`Edit ${r.n}`} title="Change or remove it" onClick={edit}>
            ✎
          </button>
        )}
        {callOut && (
          <button type="button" className="call-out" title="Give it a tolerance of its own" onClick={callOut}>
            Call out
          </button>
        )}
      </td>}
    </tr>
    );
  };
  return (
    <div className={`pmi-lab-table${readOnly ? ' view' : ''}`} ref={box}>
      <div className="pmi-lab-table-head">
        {readOnly ? <span>{groups.length} characteristic{groups.length === 1 ? '' : 's'} set on this part.</span> : <>
        <span>
          {groups.length} characteristic{groups.length === 1 ? '' : 's'}
          {rows.length > groups.length ? `, ${rows.length} features to measure` : ''}: what is specified here, what the general tolerance covers and what the file states.
          {exportable && left > 0 && ` ${left} left out of the CSV.`}
        </span>
        {exportable && <span className="btns">
          <button type="button" className="csv" onClick={onDownload} disabled={left === rows.length}>⬇ Download CSV</button>
        </span>}
        </>}
      </div>
      <table>
        <thead>
          <tr>
            {!readOnly && exportable && <th title="In the inspection list (CSV)">✓</th>}
            <th>#</th><th>Feature</th><th>Characteristic</th><th>Nom.</th><th>Tol.</th><th>Datums</th>
            {!readOnly && <th />}
          </tr>
        </thead>
        <tbody>
          {groups.flatMap((g, gi) => {
            const [head] = g;
            if (!head) return [];
            const general = head.source.startsWith('general:');
            const first = general && gi === firstGeneral;
            const summary = first ? [generalRow(groups.filter((x) => x[0]?.source.startsWith('general:')))] : [];
            if (general && !showGeneral) return summary;
            return [...summary, ...balloonRows(g, head)];
          })}
        </tbody>
      </table>
    </div>
  );

  // What the general tolerance covers, in one row: ticked or not as a whole.
  function generalRow(all: readonly (readonly Characteristic[])[]) {
    const keys = all.flat().map(inspectKey);
    const out = keys.every((k) => skipped.has(k));
    const toggle = () => setGeneralOpen(!showGeneral);
    return (
      <tr
        key="general" className={`general-summary${out ? ' skip' : ''}`} tabIndex={0} aria-expanded={showGeneral}
        onClick={toggle}
        onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggle(); } }}
      >
        {!readOnly && exportable && (
          <td onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
            <input
              type="checkbox" checked={!out} aria-label="Inspect what the general tolerance covers"
              title="In the inspection list (CSV)" onChange={(e) => onSkip(keys, !e.target.checked)}
            />
          </td>
        )}
        <td />
        <td colSpan={readOnly ? 5 : 6}>
          <span className="chev" aria-hidden>{showGeneral ? '▾' : '▸'}</span>{' '}
          {all.length} dimension{all.length === 1 ? '' : 's'} under {generalTolerance || 'the general tolerance'}
          {calledOut > 0 && ` · ${calledOut} called out`}
        </td>
      </tr>
    );
  }

  function balloonRows(g: readonly Characteristic[], head: Characteristic) {
    if (g.length === 1) return [line(head.id, head, { bal: <span className="bal">{head.n ? head.id : '—'}</span>, feature: head.feature }, head.faces, undefined, g)];
    const isOpen = open.has(head.n);
    // What the rows share is said once; a run of basic dimensions is counted.
    const same = g.every((r) => r.text === head.text);
    const shown = same ? head : { ...head, text: `${g.length} basic dimensions`, nominal: null };
    const under = (r: Characteristic) =>
      r.feature.startsWith(`${r.group}, `) ? r.feature.slice(r.group.length + 2) : r.feature;
    const toggle = () => setOpen((was) => {
      const next = new Set(was);
      if (!next.delete(head.n)) next.add(head.n);
      return next;
    });
    return [
      line(`${head.n}`, shown, {
        bal: <span className="bal">{head.n}</span>,
        feature: <><span className="chev" aria-hidden>{isOpen ? '▾' : '▸'}</span> {head.group}</>,
      }, g.flatMap((r) => r.faces), toggle, g),
      ...(isOpen ? g.map((r) => line(r.id, r, { bal: <span className="bal">{r.id}</span>, feature: under(r) }, r.faces)) : []),
    ];
  }
}
