// Specify on this computer: the app the page server serves (specify-core docs/open.md), started by
// `specify-core open` or by an agent's `show` (docs/session.md). The answers live in the command's
// session: the page sends its changes there and hears everyone else's (an agent's) as they land.
// Save asks the command to write the STEP. Nothing is stored in the browser and nothing is fetched
// from anywhere but the page's own origin.
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/600.css';
import '../src/specify/PmiLab.css';
import './app.css';
import { createRoot } from 'react-dom/client';
import { useEffect, useRef, useState } from 'react';
import {
  as9102, createIntentEngine, drawnFromScene, hasGraphics, SpecifyReview, StepDiff, toCsv, withoutNotesBlock, xlsx,
  type DrawnPmi, type IntentReview, type PmiAnswers, type PmiLabSession, type PmiQuestion, type SpecifyEngine, type SpecifySlots, type WorkerClient,
} from '../src/index';
import { stepBytes } from '../src/model/diff';
import { SessionApiMismatch } from './session-api';
import { defaultsToWrite } from '../src/specify/review';
import { saveFile } from '../src/specify/SpecifyReview';
import type { StoredInspection } from '../src/specify/saved-inspection';
import { changedIds, createSession, exportableAnswers, sameAnswers, type AnswersSession, type AnswersStore } from './answers';
import { decisions, type Decision } from './diff-report';
import { getAnswers, getConfig, getDiff, getFile, keepAlive, listen, type DiffBody, type LocalConfig } from './local';
import { DefaultRefused, DefaultsChanged, FileChanged, NotShown, defaultParts, matchMissing, sessionSaver, Stale, writtenParts, type Default, type SavedWithAnswers, type Saver } from './save';

declare const __APP_VERSION__: string;
const DOCS = 'https://github.com/pzfreo/specify-core#readme';
const ISSUES = 'https://github.com/pzfreo/specify-core/issues';
/** A part of the file, as the engine's `parts` lists them; the session keeps answers by its index.
 * ``written``: the file stores Specify's answers for it. */
interface PartRow { part: number; name: string; written: boolean }

/** Part ``part`` of the file, as the engine opens it: a file of one part whole, a part of several as that part. */
const fileOf = (name: string, bytes: Uint8Array, part: number, several: boolean) => ({
  name, bytes, sourceId: several ? `${name}#${part}` : name, ...(several ? { part } : {}),
});

/** The defaults a save with defaults lists for the session's answers ``parts``: those of every part
 * of the file (``indexes``), answered or not, in part order. */
async function defaultsOf(worker: WorkerClient, name: string, bytes: Uint8Array, several: boolean, parts: Record<string, PmiAnswers>, indexes: readonly number[]): Promise<Default[]> {
  const out: Default[] = [];
  for (const part of defaultParts(parts, indexes, several)) {
    const answers = parts[String(part)] ?? {};
    const { questions, open } = await worker.request<IntentReview>('review', { ...fileOf(name, bytes, part, several), answers });
    const raw = new Map(questions.map((q) => [q.id, JSON.stringify(q.default)] as const));
    // The command takes only open questions' defaults (not the process, a full-thread depth or a
    // parts-list row, which are never "open"): approving any other is a 400.
    const asked = new Set(open);
    out.push(...defaultsToWrite(questions, answers).filter((d) => asked.has(d.id)).map((d) => ({ ...d, part, raw: raw.get(d.id)! })));
  }
  return out;
}

interface Opened {
  /** The part shown: its index in the session, and whether the file has others. */
  part: number; several: boolean;
  session: PmiLabSession; review: IntentReview; engine: SpecifyEngine; drawn: DrawnPmi | null;
  bytes: Uint8Array; store: AnswersStore; saver: Saver;
  /** What SpecifyReview starts from; a new object only when the answers change outside the page. */
  external: { answers: PmiAnswers; review: IntentReview };
  /** The answers SpecifyReview shows (and so bases the person's next change on): its own last
   * change, or the last answers given to it from outside. */
  shown: { answers: PmiAnswers };
  /** Specify's review of ``answers``, for answers that arrive from outside the page. */
  reviewOf(answers: PmiAnswers): Promise<IntentReview>;
}
/** The revision comparison the command gave (``body``: WASM `diff()`'s, with the PMI scenes), drawn as it is. */
interface Compared { old: { name: string; bytes: Uint8Array }; next: { name: string; bytes: Uint8Array }; body: DiffBody; decisions: Decision[]; verdict: string; summary: string }

/** Opens the part on the engine with the session's answers: its analysis and mesh, their review,
 * and the PMI as drawn. ``edited``: called on each change the person makes. */
async function openPart(worker: WorkerClient, answersSession: AnswersSession, name: string, bytes: Uint8Array, at: { part: number; several: boolean }, readOnly: boolean, edited: () => void, indexes: () => readonly number[]): Promise<Opened> {
  const file = fileOf(name, bytes, at.part, at.several);
  const { sourceId } = file;
  const snapshot = await worker.request<{ analysis: PmiLabSession['analysis']; mesh: PmiLabSession['mesh']; provenance?: PmiLabSession['provenance'] }>('open', file);
  const answers = answersSession.answers(at.part);
  const reviewOf = (given: PmiAnswers) => worker.request<IntentReview>(readOnly ? 'review-given' : 'review', { ...file, answers: given });
  const review = await reviewOf(answers);
  const store = answersSession.store(at.part, snapshot.analysis.resume?.answers ?? {});
  const shown = { answers };
  const engine: SpecifyEngine = {
    // Each commit of the page's answers comes through here: what the session gets once Specify
    // accepts them. The change is the one from the answers the page showed when it was made, so an
    // agent's answer that arrived while Specify reviewed it is kept.
    questions: async (_id, given) => {
      const base = shown.answers;
      const asked = await worker.request<IntentReview>('review', { ...file, answers: given });
      shown.answers = given;
      store.set(given, base);
      if (!sameAnswers(given, base)) edited();
      return asked;
    },
    interpret: (_id, given, question, text) => worker.request('interpret', { analysis: snapshot.analysis, answers: given, question, text }),
    preview: async (_id, given) => (await reviewOf(given)).intent,
  };
  const session: PmiLabSession = {
    sessionId: sourceId, filename: name, analysis: snapshot.analysis, mesh: snapshot.mesh, answers,
    ...(snapshot.provenance ? { provenance: snapshot.provenance } : {}),
    questions: review.questions, open: review.open, problems: review.problems,
  };
  const scene = drawnFromScene(await worker.request('scene', { ...file, state: { answers } }));
  const drawn = scene.ok ? withoutNotesBlock(scene.value) : null;
  const defaults = (parts: Record<string, PmiAnswers>) => defaultsOf(worker, name, bytes, at.several, parts, indexes());
  return {
    ...at, session, review, engine, drawn: drawn && hasGraphics(drawn) ? drawn : null, bytes, store, shown,
    saver: sessionSaver(store, answersSession, () => shown.answers, defaults), external: { answers, review }, reviewOf,
  };
}

/** The PMI the saved file ``bytes`` has for the part shown, as drawn: null when it has no graphics,
 * undefined when the scene could not be made (the view is then left as it is). */
async function savedDrawn(worker: WorkerClient, bytes: Uint8Array, at: { part: number; several: boolean }): Promise<DrawnPmi | null | undefined> {
  const scene = await worker.request('file-scene', { bytes, ...(at.several ? { part: at.part } : {}) }).then(drawnFromScene, () => null);
  if (!scene) return undefined;
  const drawn = scene.ok ? withoutNotesBlock(scene.value) : null;
  return drawn && hasGraphics(drawn) ? drawn : null;
}

const stem = (name: string) => name.replace(/\.gz$/i, '').replace(/\.(step|stp)$/i, '');
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? '' : 's'}`;

interface InspectionStep { path: string; name: string; bytes: Uint8Array }
const inspectionName = (path: string) => path.split(/[\\/]/).pop() ?? path;
const SAVE_FIRST = 'Save first: the inspection files describe the saved STEP';

/** What the last save wrote, and the questions still open in it, by their short labels. */
interface SavedLine { path: string; open: string[] | null; byAgent: boolean; assemblyCount?: number; matching?: boolean }

function App() {
  const [config, setConfig] = useState<LocalConfig | null>(null);
  const [opened, setOpened] = useState<Opened | null>(null);
  const [parts, setParts] = useState<PartRow[]>([]);
  const [switching, setSwitching] = useState(false);
  // The session (every part's answers, and what is on disk) and the part shown, for its events.
  const sessionRef = useRef<AnswersSession | null>(null);
  // The part indexes of the file, for the defaults a save lists (every part, answered or not).
  const partIndexes = useRef<number[]>([0]);
  const shownRef = useRef<Opened | null>(null);
  const [compared, setCompared] = useState<Compared | null>(null);
  const [tab, setTab] = useState<'changes' | 'answers'>('changes');
  const [problem, setProblem] = useState<string | null>(null);
  const [openingError, setOpeningError] = useState<string | null>(null);
  const [apiProblem, setApiProblem] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<SavedLine | null>(null);
  const [dirty, setDirty] = useState(false);
  // The event stream: null until it first opens, false while it is lost (retrying).
  const [live, setLive] = useState<boolean | null>(null);
  const [stopped, setStopped] = useState(false);
  // An in-place save refused because another program changed the file: Save cannot succeed again.
  const [fileChanged, setFileChanged] = useState(false);
  // Save with defaults: the defaults it would write, of every part it writes, listed for the person
  // to confirm (ADR P9). ``written``: the parts the save writes (those with answers, and a file's only
  // part). ``shownLeftOut``: the part shown has no answers in the session, so the save does not write it.
  const [confirming, setConfirming] = useState<{ defaults: Default[]; written: number[]; shownLeftOut: boolean } | null>(null);
  // Answers that arrived from outside the page but could not be shown (their review failed): Save
  // is refused until they are, so the page offers to show them again.
  const [unshown, setUnshown] = useState<string | null>(null);
  const reshow = useRef<() => void>(() => {});
  const refreshConfig = useRef<() => void>(() => {});
  const worker = useRef<WorkerClient | null>(null);
  const view = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const [inspectionStep, setInspectionStep] = useState<InspectionStep | null>(null);
  const [inspectionData, setInspectionData] = useState<(StoredInspection & { part: number }) | null>(null);
  const [inspectionProblem, setInspectionProblem] = useState<string | null>(null);
  const inspectionLoad = useRef(0);
  const savedRevision = useRef(-1);
  // The bytes the session's last save wrote: the 3D view draws their PMI, for every part shown since.
  const savedBytes = useRef<Uint8Array | null>(null);
  const heardSaves = useRef(new Set<string>());
  const captureSaved = useRef<(saved: { path: string; rev: number }) => void>(() => {});
  const [slots, setSlots] = useState<SpecifySlots | null>(null);
  useEffect(() => setSlots({ view: view.current!, panel: panel.current!, exports: null }), []);
  // Answers from outside the page (an agent's, source "mcp") are shown as they land: the review
  // takes them, the first changed question opens, its faces are lit, and a note says what the
  // agent changed. The note stays until it is dismissed, the person changes something, or the
  // agent changes something else.
  const [agent, setAgent] = useState<{ ids: string[]; n: number } | null>(null);
  const [other, setOther] = useState<{ name: string; part: number; n: number; joined: boolean } | null>(null);
  const [focus, setFocus] = useState<{ questionId: string; faces?: readonly number[]; scroll?: boolean } | null>(null);
  const [lit, setLit] = useState<{ faces: readonly number[]; n: number } | undefined>(undefined);
  const edited = () => setAgent(null);

  useEffect(() => {
    let stop = () => {};
    let unlisten = () => {};
    let unwatch = () => {};
    let ended = false;
    (async () => {
      const first = await getConfig();
      setConfig(first);
      document.title = `${first.name} — Specify`;
      const heartbeat = keepAlive(setConfig, (stopped) => {
        if (stopped) setConfirming(null);
        setStopped(stopped);
      }, () => {
        ended = true;
        unlisten();
        sessionRef.current?.close();
      });
      stop = heartbeat.stop;
      refreshConfig.current = heartbeat.refresh;
      worker.current = createIntentEngine({ wasmUrl: first.wasmUrl });
      const [bytes, answers, old, diff] = await Promise.all([
        getFile('new').then(stepBytes), getAnswers(),
        first.mode === 'diff' ? getFile('old').then(stepBytes) : null,
        first.mode === 'diff' ? getDiff() : null,
      ]);
      const answersSession = createSession(answers.rev, answers.parts);
      if (first.readOnly) setInspectionStep({ path: first.path, name: first.name, bytes });
      captureSaved.current = (event) => {
        if (event.rev < savedRevision.current || ended) return;
        savedRevision.current = event.rev;
        const load = ++inspectionLoad.current;
        setInspectionStep(null);
        setInspectionData(null);
        setInspectionProblem(null);
        // The exports come from the file as saved, a copy or the input in place (`file?rev=saved`).
        void (async () => {
          const written = await getFile('saved').then(stepBytes);
          if (ended || load !== inspectionLoad.current) return;
          setInspectionStep({ path: event.path, name: inspectionName(event.path), bytes: written });
          savedBytes.current = written;
          // The 3D view draws the PMI as the save wrote it: the scene built on opening shows only
          // what the file had then (specify-ui#14). A scene that fails leaves the view as it was.
          const shown = shownRef.current;
          if (!shown || !worker.current) return;
          const drawn = await savedDrawn(worker.current, written, shown);
          if (drawn !== undefined && !ended && load === inspectionLoad.current) {
            setOpened((now) => (now?.store === shown.store ? { ...now, drawn } : now));
          }
        })().catch((error: unknown) => {
          if (!ended && load === inspectionLoad.current) setInspectionProblem(message(error));
        });
      };
      sessionRef.current = answersSession;
      if (ended) answersSession.close();
      unwatch = answersSession.subscribe(() => setDirty(answersSession.dirty()));
      // An assembly's parts: the page shows one at a time, first the first the session has answers for.
      const listed = (await worker.current.request<PartRow[]>('parts', { name: first.name, bytes })).map(({ part, name, written }) => ({ part, name, written: written === true }));
      const several = listed.length > 1;
      let rows = listed;
      const withAnswers = rows.find((row) => Object.keys(answers.parts[String(row.part)] ?? {}).some((id) => !id.startsWith('_')));
      // The part the agent's `show` named (config.part), else the first with answers.
      const asked = rows.find((row) => row.part === first.part);
      const at = { part: several ? (asked ?? withAnswers ?? rows[0]!).part : 0, several };
      setParts(several ? rows : []);
      partIndexes.current = rows.map((r) => r.part);
      const part = await openPart(worker.current, answersSession, first.name, bytes, at, first.readOnly, edited, () => partIndexes.current);
      if (old && diff) {
        const r = diff.report as { verdict?: string; summary?: string };
        setCompared({
          old: { name: first.oldName ?? 'old.step', bytes: old }, next: { name: first.name, bytes }, body: diff,
          decisions: decisions(diff.report), verdict: r.verdict ?? '', summary: r.summary ?? '',
        });
      }
      shownRef.current = part;
      let lost = false;
      if (!ended) unlisten = listen((event) => {
        // One rev can carry an event per part; a repeat of one already received is ignored.
        if (!answersSession.receive(event)) return;
        // A part joining the session is added to the page's model: the picker lists it, and the
        // session (so the defaults a save lists) holds it, with the answers it has.
        if ('part' in event && several && !rows.some((r) => r.part === event.part)) {
          rows = [...rows, { part: event.part, name: `Part ${event.part}`, written: false }];
          setParts(rows);
          partIndexes.current = rows.map((r) => r.part);
        }
        // An agent's change to another part than the one shown: said, by name.
        if (event.source === 'mcp' && 'part' in event && event.part !== shownRef.current?.part) {
          const row = rows.find((r) => r.part === event.part);
          const joined = event.joined === true && !Object.keys(event.answers).some((id) => !id.startsWith('_'));
          setOther({ name: row?.name ?? `part ${event.part}`, part: event.part, n: Date.now(), joined });
        }
      }, (event) => {
        answersSession.markSaved(event.rev);
        heardSaves.current.add(`${event.rev}:${event.path}`);
        captureSaved.current(event);
        if (event.source === 'mcp') void agentSaved(event.path);
        heartbeat.refresh();
      }, (up) => {
        setLive(up);
        if (!up) { lost = true; heartbeat.streamLost(); return; }
        // Back after a loss: what changed meanwhile, as the session has it now.
        if (lost) getAnswers().then((now) => answersSession.receive({ ...now, source: 'start' }), () => {});
        lost = false;
      });
      setOpened(part);
      // The other parts the session holds answers for: what the file stores for each, so answers an
      // agent gave them before the page opened count as unsaved. A part Specify did not write
      // stores none; one it wrote is read from the file, after the part shown is up.
      for (const row of rows) {
        if (row.part === at.part || !(String(row.part) in answers.parts)) continue;
        if (!row.written) { answersSession.fileAnswers(row.part, {}); continue; }
        const snapshot = await worker.current.request<{ analysis: PmiLabSession['analysis'] }>('open', fileOf(first.name, bytes, row.part, true)).catch(() => null);
        if (snapshot) answersSession.fileAnswers(row.part, snapshot.analysis.resume?.answers ?? {});
      }
    })().catch((error: unknown) => {
      if (ended) return;
      if (error instanceof SessionApiMismatch) setApiProblem(error.message);
      else setOpeningError(message(error));
    });
    return () => { stop(); unlisten(); unwatch(); sessionRef.current?.close(); worker.current?.dispose(); };
  }, []);

  useEffect(() => {
    if (!opened) return;
    shownRef.current = opened;
    opened.store.onProblem((error) => setProblem(`Your change did not reach the session: ${error.message}`));
  }, [opened?.store]);

  /** Shows another part of the file, with the session's answers for it now. */
  const showPart = async (index: number) => {
    const answersSession = sessionRef.current;
    if (!opened || !config || !worker.current || !answersSession || index === opened.part) return;
    setSwitching(true); setProblem(null);
    try {
      // The part left keeps its unsaved answers: in the session, and counted as unsaved.
      await opened.store.settled();
      let next = await openPart(worker.current, answersSession, config.name, opened.bytes, { part: index, several: true }, config.readOnly, edited, () => partIndexes.current);
      // After a save, the part's PMI is drawn from the saved file, as for the part shown then.
      const saved = savedBytes.current;
      const drawn = saved ? await savedDrawn(worker.current, saved, next) : undefined;
      if (drawn !== undefined) next = { ...next, drawn };
      opened.store.close();
      setOpened(next);
      setOther(null); setAgent(null);
    } catch (error) {
      setProblem(`That part could not be opened: ${message(error)}`);
    } finally { setSwitching(false); }
  };

  const current = useRef(opened); current.current = opened;
  useEffect(() => {
    if (!opened) return undefined;
    const { store, shown } = opened;
    // The questions the agent changed that are not shown yet.
    const fromAgent = new Set<string>();
    const show = (answers: PmiAnswers, source: string, previous: PmiAnswers) => {
      if (source === 'mcp') for (const id of changedIds(previous, answers)) fromAgent.add(id);
      // The page's own change, as SpecifyReview already shows it (or answers back to what it shows).
      if (sameAnswers(answers, shown.answers)) { setUnshown(null); return; }
      void opened.reviewOf(answers).then((review) => {
        // Another part shown since, or a newer change on its way (shown when it lands).
        if (current.current?.store !== store || !sameAnswers(store.get(), answers)) return;
        shown.answers = answers;
        setUnshown(null);
        setOpened((now) => (now?.store === store ? { ...now, external: { answers, review } } : now));
        const ids = [...fromAgent];
        fromAgent.clear();
        if (!ids.length) return;
        const faces = review.questions.filter((q) => ids.includes(q.id)).flatMap((q) => q.faces);
        const n = Date.now();
        setAgent({ ids, n });
        setTab('answers');
        setFocus({ questionId: ids[0]!, faces, scroll: true });
        setLit({ faces, n });
      }, (error: unknown) => {
        if (current.current?.store !== store || !sameAnswers(store.get(), answers)) return;
        // Not shown, so not saved: said, with a way to try again (a later change also tries again).
        setUnshown(message(error));
      });
    };
    // The session's answers, shown again if the page does not show them yet.
    const again = () => { if (current.current?.store === store && !sameAnswers(store.get(), shown.answers)) show(store.get(), 'start', shown.answers); };
    reshow.current = again;
    const unsubscribe = store.subscribe(show);
    // A change that landed while the part was opening, before this listened.
    again();
    return () => { unsubscribe(); setUnshown(null); };
  }, [opened?.store]);
  // The questions the agent changed are marked in the panel while its note shows: marked now and
  // again whenever the panel re-renders (the review opens the question after this runs).
  useEffect(() => {
    const root = panel.current;
    if (!agent || !root) return undefined;
    const selector = agent.ids.map((id) => `[data-question="${CSS.escape(id)}"]`).join(', ');
    const mark = () => root.querySelectorAll(selector).forEach((el) => { if (!el.classList.contains('app-agent-lit')) el.classList.add('app-agent-lit'); });
    mark();
    const observer = new MutationObserver(mark);
    observer.observe(root, { childList: true, subtree: true });
    return () => { observer.disconnect(); root.querySelectorAll('.app-agent-lit').forEach((el) => el.classList.remove('app-agent-lit')); };
  }, [agent]);
  useEffect(() => {
    if (!dirty) return;
    const guard = (event: BeforeUnloadEvent) => { event.preventDefault(); };
    window.addEventListener('beforeunload', guard);
    return () => window.removeEventListener('beforeunload', guard);
  }, [dirty]);

  const mode = config?.mode;
  const readOnly = !config || config.readOnly;

  /** The short labels of questions ``ids`` (the part shown's; an id Specify does not ask here is kept as it is). */
  const prompts = (ids: string[], ...reviews: { questions: PmiQuestion[] }[]) => {
    const prompt = new Map(reviews.flatMap((r) => r.questions.map((q) => [q.id, q.label ?? q.prompt] as const)));
    return ids.map((id) => prompt.get(id) || id);
  };
  // An agent's save says no more than where it wrote: what is still open is what the session's
  // answers for the part shown leave unanswered.
  const agentSaved = async (path: string) => {
    const now = shownRef.current;
    const line = { path, open: null, byAgent: true };
    setSaved(line);
    const review = await now?.reviewOf(now.store.get()).catch(() => null);
    if (review) setSaved((saved) => saved === line ? { ...line, open: prompts(review.open, review) } : saved);
  };

  const assemblyQuestions = async (result: SavedWithAnswers, opened: Opened) => {
    if (!worker.current || !config) return null;
    const engine = worker.current;
    const reviewed = await Promise.all(
      Object.entries(result.answers).sort(([a], [b]) => Number(a) - Number(b)).map(async ([index, answers]) => {
        const part = Number(index);
        const name = parts.find((row) => row.part === part)?.name ?? `Part ${part}`;
        const file = fileOf(config.name, opened.bytes, part, true);
        const { questions, open } = await engine.request<IntentReview>('review-given', { ...file, answers });
        const byId = new Map(questions.map((q) => [q.id, q.label ?? q.prompt]));
        const labels = open.map((id) => byId.get(id));
        return {
          open,
          labels: labels.every((label) => label !== undefined) ? labels.map((label) => `${name} — ${label}`) : null,
          required: Object.keys(answers).some((id) => !id.startsWith('_')),
        };
      }),
    );
    return matchMissing(result.missing, reviewed);
  };

  /** Save with defaults, first: the full list of the defaults it would write, in every part
   * of the file, to confirm. */
  const askDefaults = async (keepProblem = false) => {
    if (stopped || !opened || !config || !worker.current || !sessionRef.current) return;
    if (!keepProblem) setProblem(null);
    try {
      await opened.store.settled();
      const parts = sessionRef.current.parts();
      const defaults = await defaultsOf(worker.current, config.name, opened.bytes, opened.several, parts, partIndexes.current);
      // Written: every part with a default listed, and those with answers (and a file's only part).
      const written = [...new Set([...writtenParts(parts, opened.several), ...defaults.map((d) => d.part)])].sort((a, b) => a - b);
      setConfirming({ defaults, written, shownLeftOut: opened.several && !written.includes(opened.part) });
    } catch (error) { setProblem(`The defaults could not be listed: ${message(error)}`); }
  };

  // Save writes the answers given, whatever is still open, and says what is. Defaults are written
  // only when the person has seen and confirmed the list of them (``defaults``).
  const onSave = async (defaults: readonly Default[]) => {
    if (stopped || !opened) return;
    setConfirming(null);
    setSaving(true); setProblem(null);
    try {
      const result = await opened.saver.save({ defaults });
      sessionRef.current?.markSaved(result.rev);
      // The saved events are ordered by the command. A delayed HTTP reply must not replace a
      // later save at the same answer revision (for example an agent saving again meanwhile).
      if (!heardSaves.current.has(`${result.rev}:${result.path}`)) captureSaved.current(result);
      if (opened.several && result.missing.length) {
        const line = { path: result.path, open: null, byAgent: false, assemblyCount: result.missing.length, matching: true };
        setSaved(line);
        void assemblyQuestions(result, opened).catch(() => null).then((open) => setSaved((now) => now === line ? { ...line, open, matching: false } : now));
      } else {
        setSaved({ path: result.path, open: prompts(result.missing, opened.external.review, opened.review), byAgent: false });
      }
      refreshConfig.current();
    } catch (error) {
      if (sessionRef.current?.signal.aborted) return;
      // A failed write rolls its approvals back in the session: whatever the page was told while
      // they were applied is replaced by the session as it is now, so none shows as applied.
      if (!(error instanceof NotShown || error instanceof DefaultsChanged)) {
        const now = await getAnswers().catch(() => null);
        if (now) sessionRef.current?.receive({ ...now, source: 'start' });
      }
      if (error instanceof FileChanged) {
        setFileChanged(true);
        setProblem('Not saved: the file changed on disk since it was opened (another program wrote it), so Save will not write over it. '
          + 'Your answers are still here: keep them with Answers (JSON), then open the file again.');
      } else if (error instanceof DefaultsChanged) {
        setProblem('Not saved: the answers changed, so the defaults to write are not the ones you confirmed. Check the new list.');
        void askDefaults(true);
      } else if (error instanceof DefaultRefused) {
        // The command refused a default it was sent: said by the part and the question, and the list is made again.
        const row = error.part === null ? undefined : parts.find((r) => r.part === error.part);
        const where = error.part === null ? '' : ` on ${row?.name ?? `part ${error.part}`}`;
        const listed = defaults.find((d) => d.id === error.question && d.part === error.part);
        const shownValue = (value: unknown) => typeof value === 'string' ? value : JSON.stringify(value);
        const what = error.question;
        setProblem(error.values
          ? `Not saved: the default for ${what}${where} changed. You confirmed ${listed?.value ?? shownValue(error.values.expected)}; it is now ${shownValue(error.values.current)}. Nothing was written; check the new list and confirm again.`
          : `Not saved: the default for ${what}${where} was refused (${error.message}). Nothing was written; check the new list.`);
        void askDefaults(true);
      } else if (error instanceof NotShown && unshown) {
        // The answers that could not be shown: tried again; until they show, Save cannot write them.
        setProblem('Not saved: the latest answers in the session are not shown yet (see below). Trying to show them again.');
        reshow.current();
      } else if (error instanceof Stale || error instanceof NotShown) {
        // The session moved on (an agent answered meanwhile): the page shows its answers before any save.
        reshow.current();
        // The answers, or the set of parts, changed while saving: the defaults are listed again for the person to confirm.
        if (error instanceof Stale && defaults.length) {
          setProblem('Not saved: the answers or the parts changed while you saved (your agent changed them). The defaults are listed again; check them and confirm.');
          void askDefaults(true);
        } else setProblem('Not saved: the answers changed while you saved (your agent answered). The answers now shown are the latest; check them and Save again.');
      } else setProblem(`Not saved: ${message(error)}`);
    } finally { setSaving(false); }
  };

  // The answers given and kept, including any defaults already approved in the session.
  const downloadAnswers = () => {
    if (!opened || !config) return;
    const answers = exportableAnswers(opened.store.get());
    saveFile(new File([`${JSON.stringify(answers, null, 2)}\n`], `${stem(config.name)}.answers.json`, { type: 'application/json' }));
  };

  // A part's exports are derived only from the retained saved/opened STEP. Live answers and
  // subsequent edits cannot refresh this snapshot. A different part reads the same saved bytes.
  useEffect(() => {
    if (!inspectionStep || !opened || !worker.current) return;
    let stale = false;
    setInspectionData(null);
    setInspectionProblem(null);
    const file = fileOf(inspectionStep.name, inspectionStep.bytes, opened.part, opened.several);
    void worker.current.request<StoredInspection>('file-inspection', file).then((data) => {
      if (!stale) setInspectionData({ ...data, part: opened.part });
    }, (error: unknown) => { if (!stale) setInspectionProblem(message(error)); });
    return () => { stale = true; };
  }, [inspectionStep, opened?.part]);
  const inspectionReady = inspectionStep !== null && inspectionData !== null && inspectionData.part === opened?.part;
  const downloadCsv = () => {
    if (!inspectionReady || !inspectionStep || !inspectionData) return;
    saveFile(new File([toCsv(inspectionData.chars)], `${stem(inspectionStep.name)}-characteristics.csv`, { type: 'text/csv' }));
  };
  const downloadFai = () => {
    if (!inspectionReady || !inspectionStep || !inspectionData) return;
    const book = xlsx(as9102({ name: stem(inspectionStep.name) }, inspectionData.chars, new Set(), inspectionData.notes));
    saveFile(new File([book as BlobPart], `${stem(inspectionStep.name)}-AS9102.xlsx`, { type: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet' }));
  };
  const inspectionTitle = inspectionProblem ?? (!inspectionStep && savedRevision.current < 0 ? SAVE_FIRST : !inspectionReady ? 'Reading the saved STEP…' : undefined);
  /** A decision picked in the diff: its question, in the answers. */
  const decide = (d: Decision) => {
    setTab('answers');
    if (d.question) setFocus({ questionId: d.question, scroll: true });
  };

  const showingChanges = mode === 'diff' && tab === 'changes';
  const saveBlocked = apiProblem ?? (openingError ? 'Save is disabled until the file and its answers can be loaded. Correct the session answers, then reload this page.'
    : stopped ? 'Specify has stopped' : fileChanged ? 'The file changed on disk since it was opened: open it again to save' : undefined);
  return (
    <div className="app">
      <header className="app-header">
        <div className="app-title">
          <strong className="app-brand">Specify</strong>
          {config && mode === 'diff' && <span className="app-file app-old" title={config.oldPath ?? ''}>{config.oldName}</span>}
          {config && mode === 'diff' && <span aria-hidden="true">→</span>}
          {config && <span className="app-file">{config.name}</span>}
          {config && <code className="app-path">{mode === 'diff' ? `${config.oldPath} → ${config.path}` : config.path}</code>}
          {parts.length > 0 && opened && (
            <label className="app-part">Part{' '}
              <select value={opened.part} disabled={switching} onChange={(event) => void showPart(Number(event.target.value))}>
                {parts.map((row) => <option key={row.part} value={row.part}>{row.name}</option>)}
              </select>
            </label>
          )}
          {readOnly && config && <span className="app-badge">View only</span>}
          {dirty && <span className="app-dirty">Unsaved changes</span>}
        </div>
        <div className="app-actions">
          <div className="app-exports">
            {opened && <button type="button" onClick={downloadAnswers}>Answers (JSON)</button>}
            <button type="button" onClick={downloadCsv} disabled={!inspectionReady} title={inspectionTitle}>Inspection list (CSV)</button>
            <button type="button" onClick={downloadFai} disabled={!inspectionReady} title={inspectionTitle}>AS9102 FAI (Excel)</button>
            {inspectionStep && <span className="app-inspection-source">
              from <code>{inspectionStep.name}</code>
            </span>}
            {inspectionProblem && <span className="app-problem" role="alert">Inspection exports unavailable. {inspectionProblem}</span>}
          </div>
          {config && !readOnly && (
            <>
              <button type="button" className="app-save" onClick={() => void onSave([])} disabled={!opened || saving || Boolean(saveBlocked)} title={saveBlocked}>
                {saving && !stopped ? 'Saving…' : 'Save'}
              </button>
              <button type="button" className="app-save-defaults" onClick={() => void askDefaults()} disabled={!opened || saving || Boolean(saveBlocked) || confirming !== null}
                title={saveBlocked ?? "List Specify's default for each question not answered; save them once you confirm"}>
                Save with defaults
              </button>
            </>
          )}
        </div>
        {config && (
          <div className="app-paths">
            {saved && (
              <div className="app-saved" role="status">
                {saved.byAgent ? 'Your agent saved' : 'Saved'} <code>{saved.path}</code>{saved.assemblyCount === undefined && saved.open !== null && `: ${saved.open.length
                  ? `${plural(saved.open.length, 'question')} ${saved.byAgent ? 'still unanswered' : 'still open'}: ${saved.open.slice(0, 6).join('; ')}${saved.open.length > 6 ? '; …' : ''}`
                  : 'complete'}`}
                {saved.assemblyCount !== undefined && (saved.open ? (
                  <details className="app-open-questions">
                    <summary>{plural(saved.assemblyCount, 'question')} still open</summary>
                    <ul>{saved.open.map((question, i) => <li key={i}>{question}</li>)}</ul>
                  </details>
                ) : ` · ${plural(saved.assemblyCount, 'question')} still open${saved.matching ? '' : '. Specify could not match the questions to individual parts.'}`)}
              </div>
            )}
            {!stopped && config.saveTarget && <span>Save writes <code className="app-target">{config.saveTarget}</code></span>}
          </div>
        )}
      </header>
      {apiProblem && <p className="app-problem app-api-mismatch" role="alert">{apiProblem}</p>}
      {openingError && <p className="app-problem app-open-failed" role="alert">
        Could not open {config?.name ?? 'the part'}: {openingError}{' '}
        Save is disabled until the file and its answers can be loaded. Correct the session answers, then reload this page.
      </p>}
      {problem && <p className="app-problem" role="alert">{problem}</p>}
      {unshown && (
        <p className="app-problem app-unshown" role="alert">
          The latest answers in the session (from your agent or another window) could not be shown, so Save
          waits until they are: {unshown}{' '}
          <button type="button" onClick={() => reshow.current()}>Show the latest answers</button>
        </p>
      )}
      {confirming && !stopped && (
        <div className="app-defaults" role="dialog" aria-label="Save with defaults">
          {confirming.defaults.length ? (
            <>
              <p>Save writes {plural(confirming.defaults.length, 'default')} as requirements, as if you had given each answer:</p>
              {[...new Set(confirming.defaults.map((d) => d.part))].map((part) => {
                const list = <ul>{confirming.defaults.filter((d) => d.part === part).map((d) => <li key={d.id} data-default={d.id}>{d.prompt} <strong>{d.value}</strong></li>)}</ul>;
                // An assembly's defaults, by part: every part the save writes, not only the one shown.
                return opened?.several ? (
                  <section key={part} data-part={part}>
                    <h4>{parts.find((row) => row.part === part)?.name ?? `Part ${part}`}{part === opened.part ? ' (shown)' : ''}</h4>
                    {list}
                  </section>
                ) : <div key={part}>{list}</div>;
              })}
            </>
          ) : confirming.written.length ? <p>Every question with a default is answered: nothing to fill in.</p>
            : <p>No question has a default to approve.</p>}
          {opened?.several && confirming.written.length > 0 && (
            <p data-written>This save writes {confirming.written.map((part) => parts.find((row) => row.part === part)?.name ?? `Part ${part}`).join(', ')};
              a part with no answers and no default listed is left as it is.</p>
          )}
          {confirming.shownLeftOut && <p>The part shown has no answers and no default to approve, so this save does not write it.</p>}
          <button type="button" className="app-save" onClick={() => void onSave(confirming.defaults)}>
            {confirming.defaults.length ? `Save with these ${plural(confirming.defaults.length, 'default')}` : 'Save'}
          </button>{' '}
          <button type="button" onClick={() => setConfirming(null)}>Cancel</button>
        </div>
      )}
      {stopped && config ? (
        <p className="app-offline app-stopped" role="status">
          Specify has stopped. Run <code>specify-core open …</code> again (or ask your agent to show it).
          {' '}Your saved file is at <code>{saved?.path ?? config.path}</code>.
        </p>
      ) : live === false && (
        <p className="app-offline" role="status">
          Not receiving live updates from the session: reconnecting. Changes your agent makes meanwhile show when it is back.
        </p>
      )}
      {agent && (
        <p className="app-agent" role="status">
          Updated by your agent: {prompts(agent.ids, ...(opened ? [opened.external.review, opened.review] : [])).join('; ')}{' '}
          <button type="button" className="app-dismiss" onClick={() => setAgent(null)}>Dismiss</button>
        </p>
      )}
      {other && (
        <p className="app-agent" role="status">
          {other.joined ? 'Your agent opened' : 'Updated by your agent:'} {other.name}{' '}
          <button type="button" onClick={() => void showPart(other.part)}>Show it</button>
        </p>
      )}
      {mode === 'diff' && (
        <nav className="app-tabs" role="tablist">
          <button type="button" role="tab" aria-selected={tab === 'changes'} onClick={() => setTab('changes')}>What changed</button>
          <button type="button" role="tab" aria-selected={tab === 'answers'} onClick={() => setTab('answers')}>
            Requirements for {config?.name}{compared?.decisions.length ? ` (${plural(compared.decisions.length, 'decision')})` : ''}
          </button>
          {compared && compared.decisions.length > 0 && (
            <div className="app-decide" aria-label="To decide">
              <span>To decide:</span>
              {compared.decisions.map((d, i) => (
                <button key={i} type="button" className="app-decision" title={d.why} onClick={() => decide(d)}>{d.what}</button>
              ))}
            </div>
          )}
        </nav>
      )}
      <main className="app-main">
        {!opened && !problem && !openingError && !apiProblem && !stopped && <p className="app-loading">Opening {config?.name ?? 'the part'}…</p>}
        {compared && (
          <div className="app-diff diff-page" hidden={!showingChanges}>
            <StepDiff before={compared.old} after={compared.next} compared={compared.body} wasmUrl={config?.wasmUrl} />
          </div>
        )}
        <div className={`app-slot app-view${showingChanges ? ' app-under' : ''}`} ref={view} />
        <div className={`app-slot app-panel${showingChanges ? ' app-under' : ''}`} ref={panel} />
        {opened && slots && (
          <SpecifyReview
            key={opened.session.sessionId}
            engine={opened.engine} session={opened.session} slots={slots} readOnly={readOnly} drawn={opened.drawn}
            inspectionExports={false}
            externalAnswers={opened.external.answers} externalReview={opened.external.review} disabled={saving || stopped}
            focusRequest={focus} showFaces={lit}
            onExport={(_action, format) => { if (format === 'xlsx') downloadFai(); else downloadCsv(); }}
          />
        )}
      </main>
      <footer className="app-about">
        Specify {config?.specifyCore ?? ''} · app {__APP_VERSION__} · runs on this computer; the file is not uploaded ·{' '}
        <a href={DOCS} target="_blank" rel="noreferrer">Docs</a> ·{' '}
        <a href={ISSUES} target="_blank" rel="noreferrer">Report a problem</a>
      </footer>
    </div>
  );
}

createRoot(document.getElementById('root')!).render(<App />);
