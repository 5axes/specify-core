// Ported from Draftwright.io's PmiLabReview; see third_party/README.md.
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import type { PmiAnswers, PmiIntent, PmiInterpretation, PmiLabError, PmiLabSession, PmiQuestion, PmiLabEngine } from '../types/pmi';
import type { IntentReview } from '../types/editor';
import type { PartTint } from '../model/links';
import { editorDefault } from '../model/intent';
import { addedOf, CALL_OUTS, describe, kindsFor, MAY_HAVE_DATUM, nextKey } from './added';
import { codeOf } from './choose';
import { appliedByRule, chosenForYou, type Chosen } from './chosen';
import { datumChips, freeLetter, removal } from './datums';
import { optionHint } from './hints';
import { balloonsBySource, type Characteristic, characteristics, ranges, toCsv } from './characteristics';
import { CharacteristicsTable, ExtraEditor, PurposeChoice, SpecBox, SuggestBox, SuggestionCard, TOLERANCE_PICKS, type ExtraDraft } from './inputs';
import type { PmiLabel } from './labels';
import { createPicker, type Picker } from './picker';
import { answerOf, confirmRow, display, groupQuestions, needs, nextNeeding, questionForFace, rowState, summary, type QuestionGroup } from './review';
import { MODES, type PickMode } from './selection';
import './PmiLab.css';
export type SpecifyEngine = Pick<PmiLabEngine, 'questions' | 'interpret' | 'preview'>;
/** Where the workspace shows Specify: the part in the 3D pane, the questions in the edit pane. */
export interface SpecifySlots {
  view: HTMLElement; panel: HTMLElement; exports: HTMLElement | null;
  /** The selected queue row's: the current question, answered where it is listed. */
  question?: HTMLElement | null;
}
const STATE_TEXT = { required: 'needs an answer', check: 'please check', set: 'answered', ok: 'default' } as const;
/** Short names for the part's own fields; any other part question is shown by its prompt. */
const PART_FIELD: Record<string, string> = {
  'part.material': 'Material', 'part.general_tolerance': 'General tolerance', 'part.locating': 'Holes located by',
  'part.surface_finish': 'Surface finish', 'part.coating': 'Coating or treatment',
  'part.heat_treatment': 'Heat treatment', 'part.edges': 'Edges',
};
/** The part's notes, folded under "Finish and notes". */
const NOTE_FIELDS = new Set(['part.surface_finish', 'part.coating', 'part.heat_treatment', 'part.edges']);
const MODE_TEXT: Record<PickMode, string> = { face: 'Face', feature: 'Hole / feature', coplanar: 'Coplanar faces' };

function asError(error: unknown): PmiLabError {
  if (error && typeof error === 'object' && 'message' in error) return error as PmiLabError;
  return { code: 'failed', message: String(error) };
}

export function SpecifyReview({ engine, session, slots, slotQuestion, onSaved, after, onPick, showFaces, tints, externalAnswers, externalReview, disabled, focusRequest, toReview, onKeep }: {
  engine: SpecifyEngine; session: PmiLabSession; slots: SpecifySlots;
  /** The question the queue's selected row is about: what the question slot shows. */
  slotQuestion?: string | null;
  /** A file handed to the browser to save, by name. */
  onSaved?: (name: string) => void;
  /** Shown after the questions: the drawing's own issues, in the same list. */
  after?: ReactNode;
  /** Faces picked on the part, by a face or a label: the drawing locates what measures them. */
  onPick?: (faces: readonly number[]) => void;
  /** Faces selected elsewhere (the drawing), lit on the part; a new ``n`` each time. */
  showFaces?: { faces: readonly number[]; n: number };
  /** The queue's state on the part, by face; a new map when it changes. */
  tints?: ReadonlyMap<number, PartTint>;
  externalAnswers: PmiAnswers; externalReview: IntentReview; disabled: boolean; focusRequest?: { questionId: string; faces?: readonly number[]; scroll?: boolean } | null;
  /** The editor's decisions to review, by question: the suggestions are exactly these (agreed 2026-10-16). */
  toReview?: readonly string[];
  /** Keeps suggestions as they stand, the editor's way: the same as accepting them in its list. */
  onKeep?: (ids: readonly string[]) => void;
}) {
  const stage = useRef<HTMLDivElement>(null);
  const picker = useRef<Picker | null>(null);
  const [questions, setQuestions] = useState<readonly PmiQuestion[]>(session.questions);
  const [answers, setAnswers] = useState<PmiAnswers>(session.answers ?? {});
  const [resumed, setResumed] = useState(false);
  const [current, setCurrent] = useState<string | null>(null);
  // A refusal is shown by what was refused: a question's id, or null for the panel.
  const [error, setError] = useState<{ at: string | null; problem: PmiLabError } | null>(null);
  const [preview, setPreview] = useState<PmiIntent | null>(externalReview.intent);
  const [selection, setSelection] = useState<number[]>([]);
  const [mode, setMode] = useState<PickMode>('face');
  const [status, setStatus] = useState('');
  // What a label clicked says that the part cannot: until the next question or view.
  const [note, setNote] = useState('');
  // The general dimension a label clicked is, offered to be called out.
  const [noteGeneral, setNoteGeneral] = useState<{ i: number; what: string } | null>(null);
  const [show, setShow] = useState({
    // What the general tolerance covers can be a hundred labels: drawn only when chosen.
    labels: true, defaults: true, existing: true, general: false, farSide: true,
  });
  // A face clicked that no question is about shows what the general tolerance says of it.
  const [peek, setPeek] = useState<number | null>(null);
  // Set by a label clicked: the question it opens is turned to without moving the view.
  const stayPut = useRef(false);
  // The question or added requirement a focus request opened without asking to be scrolled to.
  const quiet = useRef<string | null>(null);
  const [transparency, setTransparency] = useState(50);
  // Specs typed and read, by question: their callouts, which no offered choice has.
  const [typed, setTyped] = useState<Record<string, PmiInterpretation>>({});
  // A datum being added: picked on the part before pmi-assist knows of it.
  const [adding, setAdding] = useState<PmiQuestion | null>(null);
  // A requirement being added or changed, its faces picked on the part.
  const [draft, setDraft] = useState<ExtraDraft | null>(null);
  const [problems, setProblems] = useState(session.problems ?? []);
  // Which one-line row is unfolded: a part setting, the notes, the datums or the added.
  const [unfolded, setUnfolded] = useState<string | null>(null);
  // Show only the rows with something to review: a suggestion, a doubt, a problem.
  const [onlySuggestions, setOnlySuggestions] = useState(false);
  // The review, or every characteristic numbered as its balloon.
  const [view, setView] = useState<'review' | 'table'>('review');
  const [selectedChar, setSelectedChar] = useState<string | null>(null);
  // What is left out of the inspection list: the CSV's, not the PMI's.
  const [skipped, setSkipped] = useState<ReadonlySet<string>>(new Set());
  const toggle = (key: string) => setUnfolded((open) => (open === key ? null : key));
  const rowsBox = useRef<HTMLFieldSetElement>(null);
  const turned = useRef(false);
  const groups = useMemo(() => groupQuestions(questions), [questions]);
  const partGroup = groups.find((g) => g.key === 'part');
  const rows = groups.filter((g) => g !== partGroup);
  // The datums row is always shown, last: it can be added to when nothing needs one.
  const datumGroup = rows.find((g) => g.key === 'datums') ?? { key: 'datums', title: 'Datums', questions: [] };
  const featureRows = rows.filter((g) => g.key !== 'datums');
  // Settings the file already states are not asked; they are shown as read.
  const stated = Object.entries(session.analysis.part ?? {});
  const notes = partGroup?.questions.filter((q) => NOTE_FIELDS.has(q.id)) ?? [];
  const notesSummary = notes.map((q) => String(answerOf(q, answers) ?? ''))
    .filter((v) => v && !/^(none|no edge note)/i.test(v)).join(' · ');
  const draftQuestion = useMemo<PmiQuestion | null>(() => (draft ? {
    id: draft.key, kind: 'faces', prompt: 'Pick faces', faces: draft.faces ?? [], default: draft.faces ?? [],
    why: '', attention: 'routine',
  } : null), [draft?.key, draft?.faces]);
  const currentQuestion = questions.find((q) => q.id === current)
    ?? (adding && adding.id === current ? adding : null)
    ?? (draftQuestion && draftQuestion.id === current ? draftQuestion : null);
  const faces = useMemo(() => new Map(session.analysis.faces.map((f) => [f.id, f])), [session]);
  const titles = useMemo(() => new Map(groups.map((g) => [g.key, g.title])), [groups]);
  const chars = useMemo(
    () => characteristics(preview, session.analysis, titles), [preview, session.analysis, titles],
  );
  const balloons = useMemo(() => balloonsBySource(chars), [chars]);

  // The slot shows the selected row's question whatever else moved ``current``: the session
  // reloading, or an answer opening the next question down the list.
  useEffect(() => {
    if (slots.question && slotQuestion && slotQuestion !== current && questions.some((q) => q.id === slotQuestion)) setCurrent(slotQuestion);
  }, [slots.question, slotQuestion, current, questions]);

  useEffect(() => {
    const id = focusRequest?.questionId;
    if (!id) return;
    // Opened from the queue above, it is not followed down the list unless asked.
    quiet.current = focusRequest.scroll ? null : id;
    // A problem about an added requirement is not a question: it opens as the problem's line does.
    if (!questions.some(question => question.id === id)) { setView('review'); openProblem(id, focusRequest.faces ?? []); return; }
    setView('review'); setOnlySuggestions(false); setCurrent(id);
    setUnfolded(NOTE_FIELDS.has(id) ? `note:${id}` : id.startsWith('part.') ? id : id.startsWith('datum.') ? 'datums' : null);
    // Brought into view only when asked: the queue that asked may be above it in the same column.
    if (focusRequest.scroll) window.setTimeout(() => {
      const field = rowsBox.current?.querySelector<HTMLElement>(`[data-question="${CSS.escape(id)}"]`);
      field?.scrollIntoView({ block: 'center' });
      field?.querySelector<HTMLElement>('input, select')?.focus({ preventScroll: true });
    }, 50);
  }, [focusRequest]);

  // The picker lives as long as the session; React state reaches it through refs.
  const handlers = useRef({ face: (_face: number) => {}, label: (_label: PmiLabel) => {} });
  useEffect(() => {
    if (!stage.current) return undefined;
    const created = createPicker(stage.current, {
      mesh: session.mesh, analysis: session.analysis,
    });
    created.on('change', setSelection);
    created.on('face', (face) => handlers.current.face(face));
    created.on('label', (label) => handlers.current.label(label));
    created.on('hover', (face) => {
      if (face === null) return setStatus('');
      const f = faces.get(face);
      const size = f?.radius ? ` Ø${Number((2 * f.radius).toFixed(3))}` : '';
      setStatus(`face ${face} · ${f?.kind ?? ''}${size}`);
    });
    picker.current = created;
    return () => {
      created.dispose();
      picker.current = null;
    };
  }, [session, faces]);

  useEffect(() => { picker.current?.tint(tints ?? new Map()); }, [tints, session, faces]);

  useEffect(() => {
    if (!showFaces) return;
    // Lit, not turned to: selecting on the drawing, often to drag, should not swing the part round.
    // Nothing selected there gives the part back to the open question.
    picker.current?.highlight(showFaces.faces.length ? showFaces.faces : currentQuestion?.faces ?? []);
  }, [showFaces?.n]);

  // Point the picker at the current question -- only when the question changes:
  // a pick must not be reset by its own answer. Turning to it shows what is asked.
  useEffect(() => {
    const p = picker.current;
    if (!p) return;
    p.highlight(currentQuestion?.faces ?? []);
    // A call-out's faces are the dimension's: a click chooses another, not other faces for it.
    p.setPickable(currentQuestion?.kind === 'faces' && !draft?.callOut);
    // A datum's answer is its faces; an added requirement's holds them.
    const given = currentQuestion?.kind === 'faces' ? answerOf(currentQuestion, answers) : [];
    const value = Array.isArray(given) ? given as number[]
      : given && typeof given === 'object' && Array.isArray((given as { faces?: unknown }).faces)
        ? (given as { faces: number[] }).faces : [];
    p.setSelection(value);
    setSelection(value);
    setMode('face');
    p.setMode('face');
    // The part opens at home; from then on it turns to each thing asked about.
    if (turned.current && currentQuestion?.faces.length && !stayPut.current) p.lookAt(currentQuestion.faces);
    stayPut.current = false;
    if (currentQuestion) turned.current = true;
  }, [currentQuestion?.id]);
  // A face clicked that no question is about is lit, as what its general dimensions are on.
  useEffect(() => {
    if (peek !== null && !currentQuestion) picker.current?.highlight([peek]);
  }, [peek, currentQuestion?.id]);

  useEffect(() => { setNote(''); setNoteGeneral(null); }, [current, view]);

  // The general dimensions chosen -- in the table, or on a face clicked -- drawn though the rest are not.
  const reveal = useMemo(() => {
    const ids = new Set<string>();
    for (const r of chars) {
      if (r.source.startsWith('general:') && selectedChar !== null && (r.id === selectedChar || `${r.n}` === selectedChar)) {
        ids.add(r.source);
      }
    }
    (preview?.general ?? []).forEach((g, i) => {
      const reference = Array.isArray(g['reference']) ? g['reference'] as number[] : [];
      if (peek !== null && (g.faces.includes(peek) || reference.includes(peek))) ids.add(`general:${i}`);
    });
    return ids;
  }, [chars, selectedChar, preview, peek]);

  useEffect(() => {
    picker.current?.annotate(preview, {
      current, show: { default: show.defaults, existing: show.existing, general: show.general }, balloons,
      pending: new Set(questions.filter((q) => needs(q, answers)).map((q) => q.id)), reveal,
    });
    picker.current?.setLabelsVisible(show.labels);
    picker.current?.setFarSideHidden(show.farSide);
  }, [preview, current, show, balloons, questions, answers, reveal]);

  useEffect(() => {
    picker.current?.setTransparency(transparency / 100);
  }, [transparency, session]);

  const previewing = useRef(0);
  const sessionId = useRef(session.sessionId);
  const live = useCallback(<T,>(call: (id: string) => Promise<T>): Promise<T> => call(sessionId.current), []);
  // Global project undo/redo and reopen feed the original UI's local review state.
  useEffect(() => {
    ++previewing.current; // An older preview must not overwrite a host undo/redo.
    setAnswers(externalAnswers);
    setQuestions(externalReview.questions);
    setProblems(externalReview.problems);
    setPreview(externalReview.intent);
    setTyped((typed) => Object.fromEntries(Object.entries(typed).filter(([id, read]) => externalAnswers[id] === read.value)));
  }, [externalAnswers, externalReview]);

  // Previews answer in any order; only the latest asked for is shown.
  const relabel = useCallback(async (next: PmiAnswers) => {
    const mine = ++previewing.current;
    try {
      const shown = await live((id) => engine.preview(id, next));
      if (mine === previewing.current) setPreview(shown);
    } catch {
      // Labels are a view: a failed refresh keeps the last ones.
    }
  }, [engine, live]);

  // Once per part opened -- never again on a re-render, which would throw away
  // what has been answered since. Reached through ``startWith``, set below.
  const startWith = useRef<{ commit: typeof commit; relabel: typeof relabel } | null>(null);
  useEffect(() => {
    const start = startWith.current;
    if (!start) return;
    // A STEP Specify wrote carries on from the answers it was written with.
    const saved = session.answers ?? session.analysis.resume?.answers;
    if (saved && Object.keys(saved).length > 0) {
      setResumed(true);
      void start.commit(null, saved, (asked) =>
        nextNeeding(asked.filter((g) => g.key !== 'part'), saved));
      return;
    }
    // The part's own fields are always in view; start at the first thing on the part.
    const first = groupQuestions(session.questions).filter((g) => g.key !== 'part');
    setCurrent(nextNeeding(first, {}) ?? first[0]?.questions[0]?.id ?? null);
    void start.relabel({});
  }, [session]);

  // pmi-assist decides what follows from the answers, and may refuse them.
  // ``focus`` is the question to show afterwards; by default the one answered
  // stays open, so a choice is seen to have been made.
  const commit = useCallback(async (
    at: string | null, next: PmiAnswers, focus: (asked: QuestionGroup[]) => string | null,
  ) => {
    try {
      const asked = await live((id) => engine.questions(id, next));
      setError(null);
      setAnswers(next);
      setQuestions(asked.questions);
      setProblems(asked.problems ?? []);
      setCurrent(focus(groupQuestions(asked.questions)));
      void relabel(next);
      return true;
    } catch (failure) {
      setError({ at, problem: asError(failure) });
      return false;
    }
  }, [engine, relabel, live]);
  startWith.current = { commit, relabel };

  const answer = (q: PmiQuestion, value: unknown, remove = false) => {
    if (adding && q.id === adding.id) setAdding(null);
    const next: Record<string, unknown> = { ...answers };
    if (remove) delete next[q.id];
    else next[q.id] = value;
    void commit(q.id, next, (asked) => {
      if (q.id.startsWith('part.')) return current;
      // A face pick takes the whole box; once used it closes, and what still
      // needs a person opens -- or nothing, and the row shrinks to its summary.
      // An answer folds its row to its summary; what still needs a person opens.
      return nextNeeding(asked.filter((g) => g.key !== 'part'), next);
    });
  };

  // A typed spec: pmi-assist reads it against the feature, and it is answered
  // as what it means; a refusal comes back as the reason, beside the box.
  const spec = async (q: PmiQuestion, text: string): Promise<string | null> => {
    try {
      const read = await live((id) => engine.interpret(id, answers, q.id, text));
      setTyped((t) => ({ ...t, [q.id]: read }));
      answer(q, read.value);
      return null;
    } catch (failure) {
      return asError(failure).message;
    }
  };

  const chips = useMemo(() => datumChips(preview, session.analysis), [preview, session.analysis]);
  const nextLetter = freeLetter(chips);

  // Adding a datum is picking it: the letter's question if pmi-assist already
  // asks it (one removed earlier), else a pick it learns of once answered.
  const addDatum = () => {
    if (!nextLetter) return;
    const id = `datum.${nextLetter}`;
    if (!questions.some((q) => q.id === id)) {
      setAdding({
        id, kind: 'faces', faces: [], default: [], attention: 'routine',
        prompt: `Pick datum ${nextLetter} on the part: a face, coplanar faces, or a hole or boss.`,
        why: 'Tolerances can then be measured from it.',
      });
    }
    setCurrent(id);
  };

  // Added requirements: picked, then saved as an answer pmi-assist checks.
  const added = addedOf(answers);
  const startExtra = () => {
    const key = nextKey(answers);
    setDraft({ key, kind: '', value: '', datums: [], reference: [] });
    setCurrent(key);
  };
  // A dimension the general tolerance covers, called out to be toleranced apart:
  // its kind, faces and reference given, its tolerance to say.
  const callOut = (i: number, what: string, value = '') => {
    const g = preview?.general?.[i];
    if (!g) return;
    const key = nextKey(answers);
    const reference = Array.isArray(g['reference']) ? g['reference'] as number[] : [];
    setDraft({
      key, kind: g.kind === 'size' ? 'size' : 'distance', value, datums: [], reference, faces: g.faces,
      callOut: what,
    });
    setNoteGeneral(null);
    if (view === 'table') switchView('review');
    setCurrent(key);
    // Its key may be an open draft's, which turning to it would not reset: its faces are set here.
    picker.current?.setPickable(false);
    picker.current?.setSelection([...g.faces]);
    setSelection([...g.faces]);
  };
  // Called out where it was clicked, with one of the usual tolerances: no editor,
  // no scrolling. One pmi-assist refuses opens in the editor, to say why.
  const quickCallOut = async (i: number, what: string, value: string) => {
    const g = preview?.general?.[i];
    if (!g) return;
    const key = nextKey(answers);
    const reference = Array.isArray(g['reference']) ? g['reference'] as number[] : [];
    const extra = g.kind === 'size'
      ? { faces: [...g.faces], kind: 'size' as const, value }
      : { faces: [...g.faces], kind: 'distance' as const, value, reference };
    setNoteGeneral(null);
    if (await commit(key, { ...answers, [key]: extra }, () => null)) {
      setNote(`Called out ${what} ${value}.`);
    } else {
      callOut(i, what, value);
    }
  };
  const pickFrom = () => {
    if (!draft) return;
    setDraft({ ...draft, reference: selection });
    picker.current?.setSelection([]);
    setSelection([]);
  };
  const editExtra = (key: string) => {
    const extra = added.find(([k]) => k === key)?.[1];
    if (!extra) return;
    setDraft({ key, kind: extra.kind, value: extra.value, datums: extra.datums ?? [], reference: extra.reference ?? [] });
    setCurrent(key);
  };
  const saveExtra = () => {
    if (!draft || !draft.kind) return;
    const oriented = MAY_HAVE_DATUM.includes(draft.kind) && draft.datums.length > 0;
    const extra = {
      // A call-out keeps its dimension's faces, whatever is lit on the part.
      faces: draft.callOut && draft.faces ? [...draft.faces] : selection, kind: draft.kind, value: draft.value.trim(), ...(oriented ? { datums: draft.datums } : {}),
      ...(draft.kind === 'distance' ? { reference: draft.reference } : {}),
    };
    // A new datum beyond C has no proposal: its question opens to pick it.
    const unpicked = (extra.datums ?? []).find((x) => !'ABC'.includes(x) && !chips.some((c) => c.letter === x));
    void commit(draft.key, { ...answers, [draft.key]: extra }, () => {
      setDraft(null);
      return unpicked ? `datum.${unpicked}` : null;
    });
  };
  const removeExtra = (key: string) => {
    const next: Record<string, unknown> = { ...answers };
    delete next[key];
    void commit(key, next, () => { setDraft(null); return null; });
  };

  // What pmi-assist chose, to keep (answered as it stands) or change (opened).
  const chosen = toReview ? chosenForYou(questions, answers, session.analysis, toReview).filter((c) => toReview.includes(c.id))
    : chosenForYou(questions, answers, session.analysis);
  const chosenById = new Map(chosen.map((c) => [c.id, c]));
  const applied = appliedByRule(preview);
  const card = (c: Chosen) => (
    <SuggestionCard
      key={c.id} c={c} onKeep={() => keepChosen([c.id])} onPick={(value) => pickChosen(c.id, value)}
      onMore={() => changeChosen(c.id)}
    />
  );
  // Each problem shown in the row it is about.
  const rowOf = (about: string) => (about.startsWith('extra.') ? 'added'
    : about.startsWith('datum.') ? 'datums'
      : groups.find((g) => g.questions.some((q) => q.id === about))?.key ?? null);
  const problemsIn = (key: string) => problems.filter((p) => rowOf(p.about) === key);
  const problemLines = (key: string) => problemsIn(key).map((p) => (
    <button
      key={`${p.about}:${p.message}`} type="button" className={`problem ${p.severity}`}
      onClick={() => openProblem(p.about, p.faces)}
    >
      {p.message}
    </button>
  ));
  const keepChosen = (ids: readonly string[]) => {
    if (onKeep) { onKeep(ids); return; }
    const next: Record<string, unknown> = { ...answers };
    for (const id of ids) {
      const q = questions.find((x) => x.id === id);
      if (q) next[id] = q.default;
    }
    void commit(ids[0] ?? null, next, (asked) => settled(ids, asked, next));
  };
  // After keeping or choosing: the open question stays open unless it was the
  // one settled, which folds, and what still needs a person opens.
  const settled = (ids: readonly string[], asked: QuestionGroup[], next: PmiAnswers) =>
    (current && ids.includes(current) ? nextNeeding(asked.filter((x) => x.key !== 'part'), next) : current);
  // Choosing an alternative answers it there and then; it leaves the list.
  const pickChosen = (id: string, value: string) => {
    const next = { ...answers, [id]: value };
    void commit(id, next, (asked) => settled([id], asked, next));
  };
  // Opens the question in full and brings it into view -- every time, even
  // when it is already the open one, or the tap would seem to do nothing.
  const changeChosen = (id: string) => {
    if (id.startsWith('part.')) {
      setUnfolded(NOTE_FIELDS.has(id) ? `note:${id}` : id);
      rowsBox.current?.scrollTo({ top: 0, behavior: 'smooth' });
      return;
    }
    setCurrent(id);
    window.setTimeout(() => {
      rowsBox.current?.querySelector('.pmi-lab-item.on')?.scrollIntoView({ block: 'center', behavior: 'smooth' });
    }, 60);
  };

  // A problem opens what it is about and turns to it.
  const openProblem = (about: string, faces: readonly number[]) => {
    if (about.startsWith('extra.')) editExtra(about);
    else if (questions.some((q) => q.id === about)) setCurrent(about);
    if (faces.length) picker.current?.lookAt(faces);
  };

  const removeDatum = (letter: string) => {
    const { answers: next, warning } = removal(letter, chips, answers, preview);
    if (warning && !window.confirm(`${warning}\n\nRemove datum ${letter}?`)) return;
    void commit(`datum.${letter}`, next, (asked) =>
      (asked.some((g) => g.questions.some((x) => x.id === current)) ? current : null));
  };

  // "Looks right" on a row: its checks are accepted and the next row needing a person opens.
  const confirm = (g: QuestionGroup) => {
    const next = confirmRow(g, answers);
    // Confirmed, the row folds; what still needs a person opens, if anything.
    void commit(g.questions[0]?.id ?? null, next, (asked) =>
      nextNeeding(asked.filter((x) => x.key !== 'part'), next));
  };

  handlers.current = {
    face: (face) => {
      // A call-out not yet given a tolerance gives way to whatever is clicked next.
      if (draft?.callOut && !draft.value.trim()) {
        setDraft(null);
        setCurrent(null);
      } else if (currentQuestion?.kind === 'faces') return;
      const id = questionForFace(groups, face);
      setPeek(id ? null : face);
      setNote('');
      setNoteGeneral(null);
      if (id) setCurrent(id);
      onPick?.([face]);
    },
    // A label opens its question, which turns to it; one already open, or the
    // file's own PMI, which no question is about, is turned to here.
    // A label is what it is on: that is shown, and its row -- in the table its
    // characteristic, in the review its question, or for what the general
    // tolerance covers, which has none, what allows it.
    // A label clicked is already in sight: the view stays as it is, and what it is
    // on -- for a dimension, both ends -- is lit.
    label: (label) => {
      picker.current?.highlight([...label.faces, ...(label.span?.reference ?? [])]);
      onPick?.(label.faces);
      stayPut.current = view !== 'table' && Boolean(label.question) && label.question !== current;
      if (view === 'table') {
        const n = balloons.get(label.id.split(':').slice(0, 2).join(':'))?.[0];
        if (n !== undefined) setSelectedChar(`${n}`);
      } else if (label.question?.startsWith('extra.')) editExtra(label.question);
      else if (label.question && label.question !== current) setCurrent(label.question);
      else if (label.group === 'general') {
        const general = chars.find((r) => r.source === 'part:general_tolerance')?.text ?? 'General tolerance';
        const tol = label.tolerance ? `, ${label.tolerance.upper}` : '';
        setNote(`${general}${tol} on ${label.cells[0] ?? 'this'}.`);
        const i = Number(label.id.split(':')[1]);
        const row = chars.find((r) => r.source === `general:${i}`);
        setNoteGeneral(Number.isInteger(i) ? { i, what: row?.text ?? label.cells[0] ?? '' } : null);
      }
    },
  };

  const here = rows.find((g) => g.questions.some((q) => q.id === current));
  const errorAt = (id: string) => (error?.at === id ? error.problem : null);
  const shown = new Set(questions.map((q) => q.id));
  const looseError = error && (error.at === null || !shown.has(error.at)) ? error.problem : null;

  // Keep the open row in view as it moves -- but not at the start, when the
  // part's own fields, above the rows, come first.
  const scrolled = useRef(false);
  useEffect(() => {
    if (!here) return;
    if (scrolled.current && quiet.current !== current) rowsBox.current?.querySelector('.pmi-lab-row.current')?.scrollIntoView({ block: 'nearest' });
    scrolled.current = true;
  }, [here?.key]);
  // An added requirement or call-out opened is brought into sight, wherever it was opened from.
  useEffect(() => {
    if (draft && quiet.current !== draft.key) rowsBox.current?.querySelector('.pmi-lab-extra')?.scrollIntoView?.({ block: 'nearest' });
  }, [draft?.key]);
  // Quiet lasts one visit: once the person moves on, rows are followed again.
  useEffect(() => {
    if (quiet.current !== current && quiet.current !== draft?.key) quiet.current = null;
  }, [current, draft?.key]);

  // The balloon numbers of what a row specifies, beside its title.
  const rowBalloons = (g: QuestionGroup) => {
    const numbers = chars.filter((c) => g.questions.some((q) => q.id === c.question)).map((c) => c.n);
    return numbers.length ? <span className="bal" title="Balloon numbers">{ranges(numbers)}</span> : null;
  };
  const skip = (keys: readonly string[], out: boolean) => setSkipped((was) => {
    const next = new Set(was);
    for (const k of keys) if (out) next.add(k); else next.delete(k);
    return next;
  });
  // Where a characteristic's PMI is changed: its question, or for a part's
  // note, the part's. What the general tolerance covers has none of its own.
  const editChar = (row: Characteristic) => {
    const id = row.question ?? (row.source.startsWith('part:') ? `part.${row.source.slice(5)}` : null);
    if (!id || !(id.startsWith('extra.') || questions.some((q) => q.id === id))) return null;
    return () => {
      switchView('review');
      if (id.startsWith('extra.')) editExtra(id);
      else setCurrent(id);
    };
  };
  // A characteristic chosen in the table is shown on the part.
  const showChar = (id: string, faces: readonly number[]) => {
    setSelectedChar(id);
    picker.current?.highlight(faces);
    picker.current?.lookAt(faces);
  };
  const switchView = (next: 'review' | 'table') => {
    setView(next);
    if (next === 'review') picker.current?.highlight(currentQuestion?.faces ?? []);
  };
  const downloadCsv = () => {
    const stem = session.filename.replace(/\.(step|stp|p21)$/i, '');
    saveFile(new File([toCsv(chars, skipped)], `${stem}-characteristics.csv`, { type: 'text/csv' }));
    onSaved?.(`${stem}-characteristics.csv`);
  };

  // A row: its head and summary, a check to confirm, and when open its questions.
  const questionItem = (q: PmiQuestion, g: QuestionGroup, titled = true) => (
    <QuestionItem titled={titled}
      key={`${q.id}:${JSON.stringify(answers[q.id])}`} q={q} answers={answers} active={q.id === current}
      selection={selection} mode={mode} error={errorAt(q.id)} typed={typed[q.id] ?? null}
      onSpec={(text) => spec(q, text)}
      onOpen={() => setCurrent(q.id)}
      onAnswer={(value) => answer(q, value)}
      onConfirm={() => confirm(g)}
      onReset={() => answer(q, undefined, true)}
      onMode={(m) => { setMode(m); picker.current?.setMode(m); }}
      onDefaultSelection={() => {
        const value = Array.isArray(q.default) ? q.default as number[] : [];
        picker.current?.setSelection(value);
        setSelection(value);
      }}
    />
  );
  // ``extra`` goes under the head, open or not (the datums row's chips).
  // ``sum`` and ``onHead`` replace the row's own summary and open/close, for
  // rows that are more than their questions (the datums).
  const renderRow = (
    g: QuestionGroup, extra?: ReactNode, sum?: string, onHead?: () => void, unfoldedHere = false,
  ) => {
    const state = rowState(g, answers);
    const open = g === here;
    const expanded = open || unfoldedHere;
    const firstOpen = () => (g.questions.find((q) => needs(q, answers)) ?? g.questions[0])?.id ?? null;
    // Folded, a row shows its choices still standing as cards, to keep or change.
    const cards = open ? [] : g.questions.flatMap((q) => chosenById.get(q.id) ?? []);
    const rowProblems = problemLines(g.key);
    if (onlySuggestions && !open && !cards.length && !rowProblems.length && state !== 'required') return null;
    return (
      <section key={g.key} className={`pmi-lab-row ${state}${open ? ' current' : ''}`}>
        <button
          type="button" className="pmi-lab-row-head" aria-expanded={expanded}
          onClick={() => {
            if (!onHead) setCurrent(open ? null : firstOpen());
            else { if (open) setCurrent(null); onHead(); }
          }}
        >
          {/* Only what needs a person, or was changed, is marked. */}
          {state !== 'ok' && <span className="st">{STATE_TEXT[state]}</span>}
          <Chevron open={expanded} />
          <b>{g.title}</b>
          {rowBalloons(g)}
          <span className="sum">{sum ?? summary(g, answers)}</span>
        </button>
        {rowProblems}
        {extra}
        {cards.length > 0 && <div className="pmi-lab-cards">{cards.map(card)}</div>}
        {/* Not twice: the current question shown in its queue row is answered there. */}
        {open && g.questions.filter((q) => !(slots.question && q.id === current)).map((q) => questionItem(q, g))}
        {g.key === 'datums' && adding && (
          <>
            <QuestionItem
              q={adding} answers={answers} active selection={selection} mode={mode} error={null} typed={null}
              onSpec={async () => null} onOpen={() => {}} onAnswer={(value) => answer(adding, value)}
              onConfirm={() => {}} onReset={() => {}}
              onMode={(m) => { setMode(m); picker.current?.setMode(m); }}
              onDefaultSelection={() => {}}
            />
            <button type="button" className="pmi-lab-cancel" onClick={() => { setAdding(null); setCurrent(null); }}>
              Cancel
            </button>
          </>
        )}
      </section>
    );
  };

  return (<>
    {createPortal(<div className="pmi-lab pmi-lab-pane">
      <div className="pmi-lab-view">
        <div className="pmi-lab-stage" ref={stage} />
        <div className="pmi-lab-nav">
          {/* What the view shows: about the part, so on it, not in the panel; folded,
              so it does not cover the part. */}
          <details className="pmi-lab-show">
            <summary title="What the 3D view shows">⚙ Show ▾</summary>
            <div>
              {(['labels', 'defaults', 'existing', 'general', 'farSide'] as const).map((key) => (
                <label key={key}>
                  <input
                    type="checkbox" checked={show[key]}
                    onChange={(event) => setShow({ ...show, [key]: event.target.checked })}
                  />{' '}
                  {{
                    labels: 'Labels', defaults: 'Defaults to check', existing: 'PMI from the file',
                    general: 'General dimensions', farSide: 'Hide far side',
                  }[key]}
                </label>
              ))}
            </div>
          </details>
        </div>
        {/* As step-pmi-viewer has them: above the orientation cube. */}
        <div className="pmi-lab-pad">
          <div>
            <button type="button" title="Roll a quarter turn anticlockwise" onClick={() => picker.current?.navigator.roll(-1)}>↺</button>
            <button type="button" title="Roll a quarter turn clockwise" onClick={() => picker.current?.navigator.roll(1)}>↻</button>
          </div>
          <div>
            <button type="button" title="Zoom in" onClick={() => picker.current?.navigator.dolly(1 / 1.25)}>+</button>
            <button type="button" title="Zoom out" onClick={() => picker.current?.navigator.dolly(1.25)}>−</button>
            <button type="button" className="wide" title="Fit the part" onClick={() => picker.current?.navigator.fit()}>fit</button>
            <button type="button" title="The opening view" onClick={() => picker.current?.navigator.home()}>⌂</button>
          </div>
          <label title="Transparency">
            <span aria-hidden="true">◐</span>
            <input
              type="range" min={0} max={90} value={transparency} aria-label="Transparency"
              onChange={(event) => setTransparency(event.target.valueAsNumber)}
            />
          </label>
        </div>
        <div className="pmi-lab-status">
          {status || (picker.current?.missing.length ? `faces ${picker.current.missing.join(', ')} could not be shown` : '')}
        </div>
      </div>
    </div>, slots.view)}
    {createPortal(<div className="pmi-lab pmi-lab-pane">
      <aside className="pmi-lab-panel">
        <header>
          <div className="title">
            <strong>{session.filename}</strong>
          </div>
          {note && <p className="pmi-lab-note" role="status">{note}</p>}
          {note && noteGeneral && (
            <div className="pmi-lab-quick" role="group" aria-label={`Call out ${noteGeneral.what}`}>
              <span>Call out {noteGeneral.what}:</span>
              {[...TOLERANCE_PICKS, ...(preview?.general?.[noteGeneral.i]?.kind === 'size' ? ['H7', 'g6'] : [])].map((t) => (
                <button key={t} type="button" onClick={() => void quickCallOut(noteGeneral.i, noteGeneral.what, t)}>
                  {t}
                </button>
              ))}
              <button type="button" onClick={() => callOut(noteGeneral.i, noteGeneral.what)}>Other…</button>
            </div>
          )}
          {resumed && (
            <p className="pmi-lab-resumed" role="status">
              Opened with the answers this STEP was saved with: change any of them below.
            </p>
          )}
          <div className="pmi-lab-views" role="tablist">
            <button
              type="button" role="tab" aria-selected={view === 'review'} className={view === 'review' ? 'on' : ''}
              onClick={() => switchView('review')}
            >
              Review/Edit
            </button>
            <button
              type="button" role="tab" aria-selected={view === 'table'} className={view === 'table' ? 'on' : ''}
              onClick={() => switchView('table')}
            >
              Characteristics ({chars[chars.length - 1]?.n ?? 0})
            </button>
          </div>
          {view === 'review' && (chosen.length > 0 || problems.length > 0) && (
            <div className="pmi-lab-review">
              {chosen.length > 0 && (
                <span>{chosen.length} suggestion{chosen.length === 1 ? '' : 's'}</span>
              )}
              {chosen.length > 1 && (
                <button type="button" className="ok" onClick={() => keepChosen(chosen.map((c) => c.id))}>
                  ✓ Keep all
                </button>
              )}
              {problems.length > 0 && (
                <button
                  type="button" className="warn"
                  onClick={() => openProblem(problems[0]?.about ?? '', problems[0]?.faces ?? [])}
                >
                  {problems.length} problem{problems.length === 1 ? '' : 's'}
                </button>
              )}
              <label>
                <input type="checkbox" checked={onlySuggestions} onChange={(e) => setOnlySuggestions(e.target.checked)} />
                {' '}only these
              </label>
            </div>
          )}
        </header>
        <fieldset className="pmi-lab-rows" ref={rowsBox} disabled={disabled}>
          {view === 'table' ? (
            <CharacteristicsTable
              rows={chars} selected={selectedChar} onSelect={showChar} onDownload={downloadCsv}
              skipped={skipped} onSkip={skip} onEdit={editChar}
              onCallOut={(row) => (row.source.startsWith('general:')
                ? () => callOut(Number(row.source.slice('general:'.length)), row.text) : null)}
              generalTolerance={preview?.part['general_tolerance'] ?? ''}
              calledOut={added.filter(([, extra]) => CALL_OUTS.includes(extra.kind)).length}
            />
          ) : (<>
          {(partGroup || stated.length > 0) && (
            <section className="pmi-lab-fields">
              {stated.map(([key, value]) => (
                <div key={key} className="pmi-lab-line stated">
                  <span className="name">{PART_FIELD[`part.${key}`] ?? key}</span>
                  <span className="value">{value} <small>(from the file)</small></span>
                </div>
              ))}
              {partGroup?.questions.filter((q) => !NOTE_FIELDS.has(q.id)).map((q) => {
                const c = chosenById.get(q.id);
                // Shown in its queue row instead while it is being answered there.
                if (slots.question && q.id === current) return null;
                if (needs(q, answers) === 'required' || unfolded === q.id) {
                  return (
                    <PartField
                      key={`${q.id}:${JSON.stringify(answers[q.id])}`} q={q} answers={answers} error={errorAt(q.id)}
                      onAnswer={(value) => { setUnfolded(null); answer(q, value); }}
                    />
                  );
                }
                if (c) return card(c);
                if (onlySuggestions) return null;
                return <FoldedLine key={q.id} name={PART_FIELD[q.id] ?? q.prompt} value={partValue(q, answers)} onOpen={() => toggle(q.id)} />;
              })}
              {notes.length > 0 && (unfolded === 'notes' || unfolded?.startsWith('note:') ? (
                <div className="pmi-lab-notes">
                  <button type="button" className="pmi-lab-line open" aria-expanded onClick={() => setUnfolded(null)}>
                    <span className="name"><Chevron open />Finish and notes</span><span className="value">fold</span>
                  </button>
                  {notes.filter((q) => !(slots.question && q.id === current)).map((q) => (unfolded === `note:${q.id}` ? (
                    <PartField
                      key={`${q.id}:${JSON.stringify(answers[q.id])}`} q={q} answers={answers} error={errorAt(q.id)}
                      onAnswer={(value) => { setUnfolded('notes'); answer(q, value); }}
                    />
                  ) : (
                    <FoldedLine
                      key={q.id} name={PART_FIELD[q.id] ?? q.prompt} value={partValue(q, answers)}
                      onOpen={() => setUnfolded(`note:${q.id}`)}
                    />
                  )))}
                </div>
              ) : (
                <>
                  {!onlySuggestions && (
                    <FoldedLine name="Finish and notes" value={notesSummary || 'none'} onOpen={() => setUnfolded('notes')} />
                  )}
                  {notes.flatMap((q) => chosenById.get(q.id) ?? []).map(card)}
                </>
              ))}
            </section>
          )}
          <h2 className="pmi-lab-rows-title">On the part</h2>
          {featureRows.map((g) => renderRow(g))}
          {(!onlySuggestions || draft || problemsIn('added').length > 0) && (
          <section className={`pmi-lab-row ${added.length ? 'set' : 'ok'}${draft ? ' current' : ''}`}>
            <button
              type="button" className="pmi-lab-row-head" aria-expanded={unfolded === 'added' || Boolean(draft)}
              onClick={() => (draft ? undefined : toggle('added'))}
            >
              <Chevron open={unfolded === 'added' || Boolean(draft)} />
              <b>Added by you</b>
              <span className="sum">
                {added.length ? added.map(([, extra]) => describe(extra)).join(' · ') : 'a flatness, orientation, fit, finish or called-out dimension on any face'}
              </span>
            </button>
            {(unfolded === 'added' || draft) && (
              <div className="pmi-lab-datums">
                {added.map(([key, extra]) => (
                  <span key={key} className="chip">
                    <button type="button" className="what-button" onClick={() => editExtra(key)}>
                      {describe(extra)}
                    </button>
                    <button type="button" className="x" aria-label={`Remove ${describe(extra)}`} onClick={() => removeExtra(key)}>
                      ✕
                    </button>
                  </span>
                ))}
                {!draft && <button type="button" className="add" onClick={startExtra}>+ Add a requirement</button>}
              </div>
            )}
            {draft && (
              <ExtraEditor
                draft={draft} selection={selection} kinds={kindsFor(selection, session.analysis)}
                letters={chips.map((c) => c.letter)} newLetter={nextLetter}
                error={errorAt(draft.key)?.message ?? null}
                editing={added.some(([k]) => k === draft.key)}
                onChange={setDraft} onSave={saveExtra} onRemove={() => removeExtra(draft.key)}
                onCancel={() => { setDraft(null); setCurrent(null); }}
                onPickFrom={pickFrom}
              />
            )}
            {problemLines('added')}
          </section>
          )}
          {renderRow(datumGroup, unfolded === 'datums' || here === datumGroup || adding ? (
            <div className="pmi-lab-datums">
              {chips.map((c) => (
                <span key={c.letter} className={`chip${c.existing ? ' existing' : ''}`}>
                  <button
                    type="button" className="letter" disabled={!questions.some((q) => q.id === `datum.${c.letter}`)}
                    onClick={() => setCurrent(`datum.${c.letter}`)} aria-label={`Datum ${c.letter}`}
                  >
                    {c.letter}
                  </button>
                  <span className="what">{c.existing ? `${c.text}, from the file` : c.text}</span>
                  {!c.existing && !(`datum.${c.letter}` in answers) && <small className="sg">suggested</small>}
                  {!c.existing && (
                    <button
                      type="button" className="x" aria-label={`Remove datum ${c.letter}`}
                      onClick={() => removeDatum(c.letter)}
                    >
                      ✕
                    </button>
                  )}
                </span>
              ))}
              {!chips.length && <span className="none">None needed: no tolerance is measured from one.</span>}
              {nextLetter && !adding && (
                <button type="button" className="add" onClick={addDatum}>+ Add datum</button>
              )}
            </div>
          ) : null, chips.length ? chips.map((c) => `${c.letter} ${c.text}`).join(' · ') : 'none needed', () => toggle('datums'),
          unfolded === 'datums' || Boolean(adding))}
          {applied.length > 0 && (
            <div className="pmi-lab-rules">
              <b>Applied by rule</b>
              {applied.map((line) => <div key={line}>{line}</div>)}
            </div>
          )}
          {after}
          </>)}
        </fieldset>
        {looseError && <footer><p role="alert" className="pmi-lab-error">{looseError.message}</p></footer>}
      </aside>
    </div>, slots.panel)}
    {/* The current question again in the selected queue row, so it is answered where it is listed. */}
    {slots.question && (() => {
      const g = groups.find((x) => x.questions.some((q) => q.id === current));
      const q = g?.questions.find((x) => x.id === current);
      // The part's own fields (material, tolerance, finish and notes) in the same box as in
      // the list, with its suggestions; a feature's question as its chips.
      return g && q && createPortal(<div className="pmi-lab pmi-lab-inline">{g === partGroup
        ? <PartField key={`${q.id}:${JSON.stringify(answers[q.id])}`} q={q} answers={answers} error={errorAt(q.id)} onAnswer={(value) => answer(q, value)} />
        : questionItem(q, g, false)}</div>, slots.question);
    })()}
    {slots.exports && createPortal(
      <button type="button" onClick={downloadCsv} disabled={!chars.length} title="Every dimension and tolerance, numbered, with its nominal and limits">
        Inspection list (CSV)
      </button>, slots.exports)}
  </>);
}

/** Hands ``file`` to the browser to save, under its own name. */
export function saveFile(file: File): void {
  const url = URL.createObjectURL(file);
  const link = document.createElement('a');
  link.href = url;
  link.download = file.name;
  document.body.append(link);
  link.click();
  link.remove();
  // After the click has handed the data to the download.
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** A setting folded to one line: its name and value; a tap unfolds it. */
function FoldedLine({ name, value, onOpen }: { name: string; value: string; onOpen: () => void }) {
  return (
    <button type="button" className="pmi-lab-line" aria-expanded={false} onClick={onOpen}>
      <span className="name"><Chevron open={false} />{name}</span>
      <span className="value">{value}</span>
    </button>
  );
}

/** ▸ folded, ▾ open: every row says which it is and that a tap changes it. */
function Chevron({ open }: { open: boolean }) {
  return <span className={`pmi-lab-chev${open ? ' open' : ''}`} aria-hidden="true">▸</span>;
}

/** A part setting's value in a line: its answer, else its default. */
function partValue(q: PmiQuestion, answers: PmiAnswers): string {
  const value = answerOf(q, answers);
  return typeof value === 'string' && value ? value.replace(/ \(default\)$/, '') : '—';
}

/** One of the part's own settings, unfolded: a text box, suggesting if it can, or a list. */
function PartField({ q, answers, error, onAnswer }: {
  q: PmiQuestion; answers: PmiAnswers; error: PmiLabError | null; onAnswer: (value: unknown) => void;
}) {
  const need = needs(q, answers);
  const label = PART_FIELD[q.id] ?? q.prompt;
  const value = answerOf(q, answers);
  const placeholder = need === 'required' ? 'e.g. aluminium 6082-T6' : String(q.default ?? '');
  const name = (
    <span className="name">
      {label}
      {need === 'required' && <em> required</em>}
    </span>
  );
  const rest = (
    <>
      {need === 'required' && <span className="why">{q.why}</span>}
      {q.kind === 'choice' && typeof value === 'string' && optionHint(q, value) && (
        <span className="why">{optionHint(q, value)}</span>
      )}
      {error && <span role="alert" className="pmi-lab-error">{error.message}</span>}
    </>
  );
  // A suggesting box is not wrapped in a label: a click on a suggestion would
  // focus the box again and reopen the list.
  if (q.kind === 'text' && q.suggestions?.length) {
    return (
      <div className={`pmi-lab-field${need ? ` ${need}` : ''}`} title={q.why} role="group" aria-label={label} data-question={q.id}>
        {name}
        <SuggestBox
          label={label} value={typeof answers[q.id] === 'string' ? answers[q.id] as string : ''}
          suggestions={q.suggestions} placeholder={placeholder} invalid={need === 'required'}
          onAnswer={onAnswer}
        />
        {rest}
      </div>
    );
  }
  return (
    <label className={`pmi-lab-field${need ? ` ${need}` : ''}`} title={q.why} data-question={q.id}>
      {name}
      {q.kind === 'choice' ? (
        <select value={typeof value === 'string' ? value : ''} onChange={(event) => onAnswer(event.target.value)}>
          {(q.options ?? []).map((option) => (
            <option key={option} value={option}>{option}{option === editorDefault(q) ? ' (default)' : ''}</option>
          ))}
        </select>
      ) : (
        <input
          type="text" defaultValue={typeof answers[q.id] === 'string' ? answers[q.id] as string : ''}
          placeholder={placeholder}
          onBlur={(event) => {
            const typedText = event.target.value.trim();
            if (typedText && typedText !== answers[q.id]) onAnswer(typedText);
          }}
          onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur(); }}
        />
      )}
      {rest}
    </label>
  );
}

/** A question's value in a line: as its row summary says it, else as given. */
function shortValue(q: PmiQuestion, value: unknown): string {
  return display(q, value) || (typeof value === 'string' && value ? value : '—');
}

function QuestionItem({
  q, answers, active, selection, mode, error, typed, onOpen, onAnswer, onSpec, onConfirm, onReset, onMode,
  onDefaultSelection, titled = true,
}: {
  /** Whether it shows its prompt: not under a queue row that already does. */
  titled?: boolean;
  q: PmiQuestion; answers: PmiAnswers; active: boolean; selection: number[]; mode: PickMode;
  error: PmiLabError | null; typed: PmiInterpretation | null; onOpen: () => void; onAnswer: (value: unknown) => void;
  onSpec: (text: string) => Promise<string | null>; onConfirm: () => void;
  onReset: () => void; onMode: (mode: PickMode) => void; onDefaultSelection: () => void;
}) {
  const need = needs(q, answers);
  const optional = q.attention === 'refine' ? ' (optional)' : '';
  // What an option means: the one pointed at, else the one chosen (a phone cannot point).
  const [hinted, setHinted] = useState<string | null>(null);
  const chosen = answerOf(q, answers);
  const hintFor = hinted ?? (typeof chosen === 'string' ? chosen : '');
  const hint = hintFor ? optionHint(q, hintFor) : null;
  // What the answer puts on the drawing: its choice's callout, or a typed spec's.
  const callout = q.choices?.find((c) => c.value === chosen)?.callout
    ?? (typed && typed.value === chosen ? typed.callout : undefined);
  if (!active) {
    return (
      <button type="button" className={`pmi-lab-item${need ? ` ${need}` : ''}`} onClick={onOpen} data-question={q.id}>
        <span className="prompt">{q.prompt}{optional}</span>
        <span className="value">{shortValue(q, answerOf(q, answers))} ›</span>
      </button>
    );
  }
  return (
    <div className={`pmi-lab-item on${need ? ` ${need}` : ''}`} data-question={q.id}>
      <div className="prompt">
        {titled && <>{q.prompt}{optional}</>}
        {q.id in answers && q.default !== null && (
          <button type="button" className="reset" onClick={onReset}>Reset to default</button>
        )}
      </div>
      {q.reason && need === 'check' && (
        <div className="reason">
          {q.reason}{' '}
          <button type="button" className="ok" onClick={onConfirm}>✓ Looks right</button>
        </div>
      )}
      <div className="why">{q.why}</div>
      {q.kind === 'choice' && (
        <>
          {q.choices?.length ? (
            <PurposeChoice q={q} value={chosen} onAnswer={onAnswer} onPoint={setHinted} />
          ) : (
            <div className="opts" onMouseLeave={() => setHinted(null)}>
              {(q.options ?? []).map((option) => (
                <button
                  key={option} type="button" onClick={() => onAnswer(option)}
                  onMouseEnter={() => setHinted(option)} onFocus={() => setHinted(option)}
                  onBlur={() => setHinted(null)}
                  className={`${chosen === option ? 'on' : ''}${option === q.default ? ' default' : ''}`}
                >
                  {option}
                </button>
              ))}
            </div>
          )}
          {callout && <div className="callout"><span>On the drawing</span> {callout}</div>}
          {hint && <div className="hint"><b>{codeOf(q, hintFor)}</b> {hint}</div>}
          <SpecBox q={q} codes={(q.choices ?? []).map((c) => c.code)} onSpec={onSpec} />
        </>
      )}
      {q.kind === 'text' && (
        <input
          type="text" defaultValue={typeof answers[q.id] === 'string' ? answers[q.id] as string : ''}
          aria-label={q.prompt}
          onBlur={(event) => { const value = event.target.value.trim(); if (value) onAnswer(value); }}
          onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur(); }}
        />
      )}
      {q.kind === 'faces' && (
        <>
          <div className="opts">
            {MODES.map((m) => (
              <button key={m} type="button" className={m === mode ? 'on' : ''} onClick={() => onMode(m)}>
                {MODE_TEXT[m]}
              </button>
            ))}
          </div>
          <div className="faces">{selection.length ? `faces ${selection.join(', ')}` : 'nothing selected'}</div>
          <div className="opts">
            <button type="button" onClick={() => onAnswer(selection)}>Use selection</button>
            <button type="button" onClick={() => onAnswer([])}>None</button>
            <button type="button" onClick={onDefaultSelection}>Default</button>
          </div>
          <div className="why">Click to select, shift-click to add. Drag to turn the part.</div>
        </>
      )}
      {error && <p role="alert" className="pmi-lab-error">{error.message}</p>}
    </div>
  );
}
