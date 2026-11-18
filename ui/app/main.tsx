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
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  as9102, characteristics, createIntentEngine, drawnFromScene, hasGraphics, noteStatements, SpecifyReview, StepDiff, toCsv, withoutNotesBlock, xlsx,
  type DrawnPmi, type IntentReview, type PmiAnswers, type PmiLabSession, type PmiQuestion, type SpecifyEngine, type SpecifySlots, type WorkerClient,
} from '../src/index';
import { stepBytes } from '../src/model/diff';
import { defaultsToWrite, groupQuestions } from '../src/specify/review';
import { saveFile } from '../src/specify/SpecifyReview';
import { changedIds, createSession, sameAnswers, type AnswersSession, type AnswersStore } from './answers';
import { decisions, type Decision } from './diff-report';
import { getAnswers, getConfig, getDiff, getFile, keepAlive, listen, type LocalConfig } from './local';
import { DefaultsChanged, FileChanged, NotShown, sessionSaver, Stale, type Saver } from './save';

declare const __APP_VERSION__: string;
const DOCS = 'https://github.com/pzfreo/specify-core#readme';
const ISSUES = 'https://github.com/pzfreo/specify-core/issues';
/** A part of the file, as the engine's `parts` lists them; the session keeps answers by its index. */
interface PartRow { part: number; name: string }

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
interface Compared { old: { name: string; bytes: Uint8Array }; next: { name: string; bytes: Uint8Array }; decisions: Decision[]; verdict: string; summary: string }

/** Opens the part on the engine with the session's answers: its analysis and mesh, their review,
 * and the PMI as drawn. ``edited``: called on each change the person makes. */
async function openPart(worker: WorkerClient, answersSession: AnswersSession, name: string, bytes: Uint8Array, at: { part: number; several: boolean }, edited: () => void): Promise<Opened> {
  const sourceId = at.several ? `${name}#${at.part}` : name;
  // A file of one part is opened whole; a part of several is opened as that part.
  const file = { name, bytes, sourceId, ...(at.several ? { part: at.part } : {}) };
  const snapshot = await worker.request<{ analysis: PmiLabSession['analysis']; mesh: PmiLabSession['mesh'] }>('open', file);
  const answers = answersSession.answers(at.part);
  const review = await worker.request<IntentReview>('review', { ...file, answers });
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
    preview: async (_id, given) => (await worker.request<IntentReview>('review', { ...file, answers: given })).intent,
  };
  const session: PmiLabSession = {
    sessionId: sourceId, filename: name, analysis: snapshot.analysis, mesh: snapshot.mesh, answers,
    questions: review.questions, open: review.open, problems: review.problems,
  };
  const scene = drawnFromScene(await worker.request('scene', { ...file, state: { answers } }));
  const drawn = scene.ok ? withoutNotesBlock(scene.value) : null;
  const reviewOf = (given: PmiAnswers) => worker.request<IntentReview>('review', { ...file, answers: given });
  const defaultsOf = async (given: PmiAnswers) => defaultsToWrite((await reviewOf(given)).questions, given).map((d) => d.id);
  return {
    ...at, session, review, engine, drawn: drawn && hasGraphics(drawn) ? drawn : null, bytes, store, shown,
    saver: sessionSaver(store, () => shown.answers, defaultsOf), external: { answers, review }, reviewOf,
  };
}

const stem = (name: string) => name.replace(/\.gz$/i, '').replace(/\.(step|stp)$/i, '');
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? '' : 's'}`;

/** What the last save wrote, and the questions still open in it, by their prompts. */
interface SavedLine { path: string; open: string[] | null; byAgent: boolean }

function App() {
  const [config, setConfig] = useState<LocalConfig | null>(null);
  const [opened, setOpened] = useState<Opened | null>(null);
  const [parts, setParts] = useState<PartRow[]>([]);
  const [switching, setSwitching] = useState(false);
  // The session (every part's answers, and what is on disk) and the part shown, for its events.
  const sessionRef = useRef<AnswersSession | null>(null);
  const shownRef = useRef<Opened | null>(null);
  const [compared, setCompared] = useState<Compared | null>(null);
  const [tab, setTab] = useState<'changes' | 'answers'>('changes');
  const [problem, setProblem] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<SavedLine | null>(null);
  const [dirty, setDirty] = useState(false);
  // The event stream: null until it first opens, false while it is lost (retrying).
  const [live, setLive] = useState<boolean | null>(null);
  // An in-place save refused because another program changed the file: Save cannot succeed again.
  const [fileChanged, setFileChanged] = useState(false);
  // Save with defaults: the defaults it would write, listed for the person to confirm (ADR P9).
  const [confirming, setConfirming] = useState<{ id: string; prompt: string; value: string }[] | null>(null);
  const worker = useRef<WorkerClient | null>(null);
  const view = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const exportsBox = useRef<HTMLDivElement>(null);
  const [slots, setSlots] = useState<SpecifySlots | null>(null);
  useEffect(() => setSlots({ view: view.current!, panel: panel.current!, exports: exportsBox.current! }), []);
  // Answers from outside the page (an agent's, source "mcp") are shown as they land: the review
  // takes them, the first changed question opens, its faces are lit, and a note says what the
  // agent changed. The note stays until it is dismissed, the person changes something, or the
  // agent changes something else.
  const [agent, setAgent] = useState<{ ids: string[]; n: number } | null>(null);
  const [other, setOther] = useState<{ name: string; part: number; n: number } | null>(null);
  const [focus, setFocus] = useState<{ questionId: string; faces?: readonly number[]; scroll?: boolean } | null>(null);
  const [lit, setLit] = useState<{ faces: readonly number[]; n: number } | undefined>(undefined);
  const edited = () => setAgent(null);

  useEffect(() => {
    let stop = () => {};
    let unlisten = () => {};
    let unwatch = () => {};
    (async () => {
      const first = await getConfig();
      setConfig(first);
      document.title = `${first.name} — Specify`;
      stop = keepAlive(setConfig);
      worker.current = createIntentEngine({ wasmUrl: first.wasmUrl });
      const [bytes, answers, old, diff] = await Promise.all([
        getFile('new').then(stepBytes), getAnswers(),
        first.mode === 'diff' ? getFile('old').then(stepBytes) : null,
        first.mode === 'diff' ? getDiff() : null,
      ]);
      const answersSession = createSession(answers.rev, answers.parts);
      sessionRef.current = answersSession;
      unwatch = answersSession.subscribe(() => setDirty(answersSession.dirty()));
      // An assembly's parts: the page shows one at a time, first the first the session has answers for.
      const rows = (await worker.current.request<PartRow[]>('parts', { name: first.name, bytes })).map(({ part, name }) => ({ part, name }));
      const several = rows.length > 1;
      const withAnswers = rows.find((row) => Object.keys(answers.parts[String(row.part)] ?? {}).some((id) => !id.startsWith('_')));
      const at = { part: several ? (withAnswers ?? rows[0]!).part : 0, several };
      setParts(several ? rows : []);
      const part = await openPart(worker.current, answersSession, first.name, bytes, at, edited);
      if (old && diff) {
        const { report } = diff;
        const r = report as { verdict?: string; summary?: string };
        setCompared({
          old: { name: first.oldName ?? 'old.step', bytes: old }, next: { name: first.name, bytes },
          decisions: decisions(report), verdict: r.verdict ?? '', summary: r.summary ?? '',
        });
      }
      shownRef.current = part;
      let lost = false;
      unlisten = listen((event) => {
        answersSession.receive(event);
        // An agent's change to another part than the one shown: said, by name.
        if (event.source === 'mcp' && 'part' in event && event.part !== shownRef.current?.part) {
          const row = rows.find((r) => r.part === event.part);
          setOther({ name: row?.name ?? `part ${event.part}`, part: event.part, n: Date.now() });
        }
      }, (event) => {
        answersSession.markSaved(event.rev);
        if (event.source === 'mcp') void agentSaved(event.path);
        getConfig().then(setConfig, () => {});
      }, (up) => {
        setLive(up);
        if (!up) { lost = true; return; }
        // Back after a loss: what changed meanwhile, as the session has it now.
        if (lost) getAnswers().then((now) => answersSession.receive({ ...now, source: 'start' }), () => {});
        lost = false;
      });
      setOpened(part);
    })().catch((error: unknown) => setProblem(message(error)));
    return () => { stop(); unlisten(); unwatch(); worker.current?.dispose(); };
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
      const next = await openPart(worker.current, answersSession, config.name, opened.bytes, { part: index, several: true }, edited);
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
      // The page's own change, as SpecifyReview already shows it.
      if (sameAnswers(answers, shown.answers)) return;
      void opened.reviewOf(answers).then((review) => {
        // Another part shown since, or a newer change on its way (shown when it lands).
        if (current.current?.store !== store || !sameAnswers(store.get(), answers)) return;
        shown.answers = answers;
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
      }, (error: unknown) => setProblem(`An answer from your agent could not be shown: ${message(error)}`));
    };
    const unsubscribe = store.subscribe(show);
    // A change that landed while the part was opening, before this listened.
    if (!sameAnswers(store.get(), shown.answers)) show(store.get(), 'start', shown.answers);
    return unsubscribe;
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

  /** The prompts of questions ``ids`` (the part shown's; an id Specify does not ask here is kept as it is). */
  const prompts = (ids: string[], ...reviews: { questions: PmiQuestion[] }[]) => {
    const prompt = new Map(reviews.flatMap((r) => r.questions.map((q) => [q.id, q.prompt] as const)));
    return ids.map((id) => prompt.get(id) || id);
  };
  // An agent's save says no more than where it wrote: what is still open is what the session's
  // answers for the part shown leave unanswered.
  const agentSaved = async (path: string) => {
    const now = shownRef.current;
    if (!now) return;
    const review = await now.reviewOf(now.store.get()).catch(() => null);
    setSaved({ path, open: review ? prompts(review.open, review) : null, byAgent: true });
  };

  /** Save with defaults, first: the full list of the defaults it would write, to confirm. */
  const askDefaults = async () => {
    if (!opened) return;
    setProblem(null);
    try {
      await opened.store.settled();
      const answers = opened.store.get();
      setConfirming(defaultsToWrite((await opened.reviewOf(answers)).questions, answers));
    } catch (error) { setProblem(`The defaults could not be listed: ${message(error)}`); }
  };

  // Save writes the answers given, whatever is still open, and says what is. Defaults are written
  // only when the person has seen and confirmed the list of them (``defaults``).
  const onSave = async (defaults: readonly string[]) => {
    if (!opened) return;
    setConfirming(null);
    setSaving(true); setProblem(null);
    try {
      const result = await opened.saver.save({ defaults });
      sessionRef.current?.markSaved(result.rev);
      setSaved({ path: result.path, open: prompts(result.missing, opened.external.review, opened.review), byAgent: false });
      setConfig(await getConfig());
    } catch (error) {
      if (error instanceof FileChanged) {
        setFileChanged(true);
        setProblem('Not saved: the file changed on disk since it was opened (another program wrote it), so Save will not write over it. '
          + 'Your answers are still here: keep them with Answers (JSON), then open the file again.');
      } else if (error instanceof DefaultsChanged) {
        setProblem('Not saved: the answers changed, so the defaults to write are not the ones you confirmed. Check the new list.');
        void askDefaults();
      } else if (error instanceof Stale || error instanceof NotShown) {
        // The session moved on (an agent answered meanwhile): the page shows its answers before any save.
        if (error instanceof Stale) {
          const now = await getAnswers().catch(() => null);
          if (now) sessionRef.current?.receive({ ...now, source: 'start' });
        }
        setProblem('Not saved: the answers changed while you saved (your agent answered). The answers now shown are the latest; check them and Save again.');
      } else setProblem(`Not saved: ${message(error)}`);
    } finally { setSaving(false); }
  };

  // Every answer the STEP is written with, the defaults made explicit: what `specify-core write` takes.
  const downloadAnswers = async () => {
    if (!opened || !config || !worker.current) return;
    const answers = await worker.current.request('answers', {
      name: config.name, bytes: opened.bytes, sourceId: opened.session.sessionId, ...(opened.several ? { part: opened.part } : {}), answers: opened.store.get(),
    });
    saveFile(new File([`${JSON.stringify(answers, null, 2)}\n`], `${stem(config.name)}.answers.json`, { type: 'application/json' }));
  };

  // Read only, the review's own exports are not shown; the PMI that is set is listed here instead.
  const readOnlyChars = useMemo(() => {
    if (!opened || !readOnly) return null;
    const titles = new Map(groupQuestions(opened.review.questions).map((g) => [g.key, g.title]));
    return characteristics(opened.review.intent, opened.session.analysis, titles);
  }, [opened, readOnly]);
  const downloadCsv = () => {
    if (!opened || !readOnlyChars || !config) return;
    saveFile(new File([toCsv(readOnlyChars)], `${stem(config.name)}-characteristics.csv`, { type: 'text/csv' }));
  };
  const downloadFai = () => {
    if (!opened || !readOnlyChars || !config) return;
    const book = xlsx(as9102({ name: stem(config.name) }, readOnlyChars, new Set(), noteStatements(opened.review.intent, opened.session.analysis)));
    saveFile(new File([book as BlobPart], `${stem(config.name)}-AS9102.xlsx`, { type: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet' }));
  };
  /** A decision picked in the diff: its question, in the answers. */
  const decide = (d: Decision) => {
    setTab('answers');
    if (d.question) setFocus({ questionId: d.question, scroll: true });
  };

  const showingChanges = mode === 'diff' && tab === 'changes';
  const saveBlocked = fileChanged ? 'The file changed on disk since it was opened: open it again to save' : undefined;
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
          <div className="app-exports" ref={exportsBox}>
            {opened && <button type="button" onClick={() => void downloadAnswers()}>Answers (JSON)</button>}
            {readOnlyChars && <>
              <button type="button" onClick={downloadCsv} disabled={!readOnlyChars.length}>Inspection list (CSV)</button>
              <button type="button" onClick={downloadFai} disabled={!readOnlyChars.length}>AS9102 FAI (Excel)</button>
            </>}
          </div>
          {config && !readOnly && (
            <>
              <button type="button" className="app-save" onClick={() => void onSave([])} disabled={!opened || saving || fileChanged} title={saveBlocked}>
                {saving ? 'Saving…' : 'Save'}
              </button>
              <button type="button" className="app-save-defaults" onClick={() => void askDefaults()} disabled={!opened || saving || fileChanged || confirming !== null}
                title={saveBlocked ?? "List Specify's default for each question not answered; save them once you confirm"}>
                Save with defaults
              </button>
            </>
          )}
        </div>
        {config && (
          <div className="app-paths">
            {saved && (
              <span className="app-saved" role="status">
                {saved.byAgent ? 'Your agent saved' : 'Saved'} <code>{saved.path}</code>{saved.open === null ? '' : `: ${saved.open.length
                  ? `${plural(saved.open.length, 'question')} ${saved.byAgent ? 'still unanswered' : 'still open'}: ${saved.open.slice(0, 6).join('; ')}${saved.open.length > 6 ? '; …' : ''}`
                  : 'complete'}`}
              </span>
            )}
            {config.saveTarget && <span>Save writes <code className="app-target">{config.saveTarget}</code></span>}
          </div>
        )}
      </header>
      {problem && <p className="app-problem" role="alert">{problem}</p>}
      {confirming && (
        <div className="app-defaults" role="dialog" aria-label="Save with defaults">
          {confirming.length ? (
            <>
              <p>Save writes {plural(confirming.length, 'default')} as requirements, as if you had given each answer:</p>
              <ul>{confirming.map((d) => <li key={d.id} data-default={d.id}>{d.prompt} <strong>{d.value}</strong></li>)}</ul>
            </>
          ) : <p>Every question with a default is answered: nothing to fill in.</p>}
          <button type="button" className="app-save" onClick={() => void onSave(confirming.map((d) => d.id))}>
            {confirming.length ? `Save with these ${plural(confirming.length, 'default')}` : 'Save'}
          </button>{' '}
          <button type="button" onClick={() => setConfirming(null)}>Cancel</button>
        </div>
      )}
      {live === false && (
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
          Updated by your agent: {other.name}{' '}
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
        {!opened && !problem && <p className="app-loading">Opening {config?.name ?? 'the part'}…</p>}
        {compared && (
          <div className="app-diff diff-page" hidden={!showingChanges}>
            {/* TODO(StepDiff from the command's comparison): StepDiff compares the two files again in
                the page's engine. specify-core's GET diff now gives WASM diff()'s {report, markdown,
                view} (getDiff's ``compared``), but computed without the PMI scenes (diff()'s
                ``scenes`` false), which StepDiff draws the PMI changes from; drawing from it would
                lose them. Switch once GET diff carries `report.pmi.scenes`. */}
            <StepDiff before={compared.old} after={compared.next} wasmUrl={config?.wasmUrl} />
          </div>
        )}
        <div className={`app-slot app-view${showingChanges ? ' app-under' : ''}`} ref={view} />
        <div className={`app-slot app-panel${showingChanges ? ' app-under' : ''}`} ref={panel} />
        {opened && slots && (
          <SpecifyReview
            key={opened.session.sessionId}
            engine={opened.engine} session={opened.session} slots={slots} readOnly={readOnly} drawn={opened.drawn}
            externalAnswers={opened.external.answers} externalReview={opened.external.review} disabled={saving}
            focusRequest={focus} showFaces={lit}
            onExport={(action) => action()}
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
