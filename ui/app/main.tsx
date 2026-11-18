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
  type DrawnPmi, type IntentReview, type PmiAnswers, type PmiLabSession, type SpecifyEngine, type SpecifySlots, type WorkerClient,
} from '../src/index';
import { stepBytes } from '../src/model/diff';
import { groupQuestions } from '../src/specify/review';
import { saveFile } from '../src/specify/SpecifyReview';
import { changedIds, createSessionStore, type AnswersStore } from './answers';
import { decisions, type Decision } from './diff-report';
import { getAnswers, getConfig, getDiff, getFile, keepAlive, listen, Refused, type LocalConfig } from './local';
import { sessionSaver, type Saver } from './save';

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
  /** Specify's review of ``answers``, for answers that arrive from outside the page. */
  reviewOf(answers: PmiAnswers): Promise<IntentReview>;
}
interface Compared { old: { name: string; bytes: Uint8Array }; next: { name: string; bytes: Uint8Array }; decisions: Decision[]; verdict: string; summary: string }

/** Opens the part on the engine with the session's answers: its analysis and mesh, their review,
 * and the PMI as drawn. */
async function openPart(worker: WorkerClient, name: string, bytes: Uint8Array, at: { part: number; several: boolean }, rev: number, answers: PmiAnswers): Promise<Opened> {
  const sourceId = at.several ? `${name}#${at.part}` : name;
  // A file of one part is opened whole; a part of several is opened as that part.
  const file = { name, bytes, sourceId, ...(at.several ? { part: at.part } : {}) };
  const snapshot = await worker.request<{ analysis: PmiLabSession['analysis']; mesh: PmiLabSession['mesh'] }>('open', file);
  const review = await worker.request<IntentReview>('review', { ...file, answers });
  const store = createSessionStore(at.part, rev, answers, snapshot.analysis.resume?.answers ?? {});
  const engine: SpecifyEngine = {
    // Each commit of the page's answers comes through here: what the session gets once Specify accepts them.
    questions: async (_id, given) => {
      const asked = await worker.request<IntentReview>('review', { ...file, answers: given });
      store.set(given, 'page');
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
  const shown = scene.ok ? withoutNotesBlock(scene.value) : null;
  return {
    ...at, session, review, engine, drawn: shown && hasGraphics(shown) ? shown : null, bytes, store,
    saver: sessionSaver(store), external: { answers, review },
    reviewOf: (given) => worker.request<IntentReview>('review', { ...file, answers: given }),
  };
}

const stem = (name: string) => name.replace(/\.gz$/i, '').replace(/\.(step|stp)$/i, '');
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? '' : 's'}`;

function App() {
  const [config, setConfig] = useState<LocalConfig | null>(null);
  const [opened, setOpened] = useState<Opened | null>(null);
  const [parts, setParts] = useState<PartRow[]>([]);
  const [switching, setSwitching] = useState(false);
  // The part shown, for the session's events, which arrive for every part.
  const shownRef = useRef<Opened | null>(null);
  const [compared, setCompared] = useState<Compared | null>(null);
  const [tab, setTab] = useState<'changes' | 'answers'>('changes');
  const [problem, setProblem] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<{ path: string; missing: string[] | null; byAgent: boolean } | null>(null);
  const [dirty, setDirty] = useState(false);
  const worker = useRef<WorkerClient | null>(null);
  const view = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const exportsBox = useRef<HTMLDivElement>(null);
  const [slots, setSlots] = useState<SpecifySlots | null>(null);
  useEffect(() => setSlots({ view: view.current!, panel: panel.current!, exports: exportsBox.current! }), []);

  useEffect(() => {
    let stop = () => {};
    let unlisten = () => {};
    (async () => {
      const first = await getConfig();
      setConfig(first);
      document.title = `${first.name} — Specify`;
      stop = keepAlive(setConfig);
      worker.current = createIntentEngine({ wasmUrl: first.wasmUrl });
      const [bytes, answers, old, report] = await Promise.all([
        getFile('new').then(stepBytes), getAnswers(),
        first.mode === 'diff' ? getFile('old').then(stepBytes) : null,
        first.mode === 'diff' ? getDiff() : null,
      ]);
      // An assembly's parts: the page shows one at a time, first the first the session has answers for.
      const rows = (await worker.current.request<PartRow[]>('parts', { name: first.name, bytes })).map(({ part, name }) => ({ part, name }));
      const several = rows.length > 1;
      const withAnswers = rows.find((row) => Object.keys(answers.parts[String(row.part)] ?? {}).some((id) => !id.startsWith('_')));
      const at = { part: several ? (withAnswers ?? rows[0]!).part : 0, several };
      setParts(several ? rows : []);
      const part = await openPart(worker.current, first.name, bytes, at, answers.rev, answers.parts[String(at.part)] ?? {});
      if (old && report) {
        const r = report as { verdict?: string; summary?: string };
        setCompared({
          old: { name: first.oldName ?? 'old.step', bytes: old }, next: { name: first.name, bytes },
          decisions: decisions(report), verdict: r.verdict ?? '', summary: r.summary ?? '',
        });
      }
      shownRef.current = part;
      unlisten = listen((event) => {
        shownRef.current?.store.receive(event);
        // An agent's change to another part than the one shown: said, by name.
        if (event.source === 'mcp' && 'part' in event && event.part !== shownRef.current?.part) {
          const row = rows.find((r) => r.part === event.part);
          setOther({ name: row?.name ?? `part ${event.part}`, part: event.part, n: Date.now() });
        }
      }, (event) => {
        shownRef.current?.store.markSaved(event.rev);
        setDirty(shownRef.current?.store.dirty() ?? false);
        setSaved((was) => (was?.path === event.path ? was : { path: event.path, missing: null, byAgent: event.source === 'mcp' }));
        getConfig().then(setConfig, () => {});
      });
      setOpened(part);
    })().catch((error: unknown) => setProblem(message(error)));
    return () => { stop(); unlisten(); worker.current?.dispose(); };
  }, []);

  const [other, setOther] = useState<{ name: string; part: number; n: number } | null>(null);
  useEffect(() => {
    if (!opened) return;
    shownRef.current = opened;
    setDirty(opened.store.dirty());
    opened.store.onProblem((error) => setProblem(`Your change did not reach the session: ${error.message}`));
  }, [opened?.store]);
  /** Shows another part of the file, with the session's answers for it now. */
  const showPart = async (index: number) => {
    if (!opened || !config || !worker.current || index === opened.part) return;
    setSwitching(true); setProblem(null);
    try {
      const now = await getAnswers();
      setOpened(await openPart(worker.current, config.name, opened.bytes, { part: index, several: true }, now.rev, now.parts[String(index)] ?? {}));
      setOther(null); setAgent(null);
    } catch (error) {
      setProblem(`That part could not be opened: ${message(error)}`);
    } finally { setSwitching(false); }
  };

  // The answers given since the last save; closing the tab asks first.
  useEffect(() => opened?.store.subscribe(() => setDirty(opened.store.dirty())), [opened]);
  // Answers from outside the page (an agent's, source "mcp") are shown as they land: the review
  // takes them, the first changed question opens, its faces are lit, and a note says what the agent changed.
  const [agent, setAgent] = useState<{ ids: string[]; n: number } | null>(null);
  const [focus, setFocus] = useState<{ questionId: string; faces?: readonly number[]; scroll?: boolean } | null>(null);
  const [lit, setLit] = useState<{ faces: readonly number[]; n: number } | undefined>(undefined);
  const live = useRef(opened); live.current = opened;
  useEffect(() => opened?.store.subscribe((answers, source, previous) => {
    if (source === 'page') return;
    const ids = changedIds(previous, answers);
    void opened.reviewOf(answers).then((review) => {
      if (live.current?.store !== opened.store) return;
      setOpened((now) => (now ? { ...now, external: { answers, review } } : now));
      if (source !== 'mcp' || !ids.length) return;
      const faces = review.questions.filter((q) => ids.includes(q.id)).flatMap((q) => q.faces);
      const n = Date.now();
      setAgent({ ids, n });
      setTab('answers');
      setFocus({ questionId: ids[0]!, faces, scroll: true });
      setLit({ faces, n });
      window.setTimeout(() => setAgent((shown) => (shown?.n === n ? null : shown)), 8000);
    }, (error: unknown) => setProblem(`An answer from your agent could not be shown: ${message(error)}`));
  }), [opened?.store]);
  // The questions the agent changed are marked in the panel while its note shows.
  useEffect(() => {
    if (!agent) return undefined;
    const marked: Element[] = [];
    const mark = () => {
      for (const id of agent.ids) {
        document.querySelectorAll(`.pmi-lab-panel [data-question="${CSS.escape(id)}"]`).forEach((el) => { el.classList.add('app-agent-lit'); marked.push(el); });
      }
    };
    // After the review has opened the question.
    const timer = window.setTimeout(mark, 100);
    return () => { window.clearTimeout(timer); marked.forEach((el) => el.classList.remove('app-agent-lit')); };
  }, [agent]);
  useEffect(() => {
    if (!dirty) return;
    const guard = (event: BeforeUnloadEvent) => { event.preventDefault(); };
    window.addEventListener('beforeunload', guard);
    return () => window.removeEventListener('beforeunload', guard);
  }, [dirty]);

  const mode = config?.mode;
  const readOnly = !config || config.readOnly;

  // Save writes the answers given, whatever is still open, and says what is. The defaults are taken
  // only when the person asks (the second button).
  const onSave = async (acceptDefaults: boolean) => {
    if (!opened) return;
    setSaving(true); setProblem(null);
    try {
      const result = await opened.saver.save({ acceptDefaults });
      opened.store.markSaved(result.rev); setDirty(opened.store.dirty());
      setSaved({ path: result.path, missing: result.missing, byAgent: false });
      setConfig(await getConfig());
    } catch (error) {
      if (error instanceof Refused && error.status === 409 && typeof error.body.rev === 'number') {
        // The session moved on (an agent answered meanwhile): the page shows its answers before any save.
        const now = await getAnswers().catch(() => null);
        if (now) opened.store.receive({ ...now, source: 'start' });
        setProblem(`Not saved: ${error.message}. The answers now shown are the latest; check them and Save again.`);
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
              <button type="button" className="app-save" onClick={() => void onSave(false)} disabled={!opened || saving}>
                {saving ? 'Saving…' : 'Save'}
              </button>
              <button type="button" className="app-save-defaults" onClick={() => void onSave(true)} disabled={!opened || saving}
                title="Fill every question not answered with Specify's default, then save">
                Save with defaults
              </button>
            </>
          )}
        </div>
        {config && (
          <div className="app-paths">
            {saved && (
              <span className="app-saved" role="status">
                {saved.byAgent ? 'Your agent saved' : 'Saved'} <code>{saved.path}</code>{saved.missing === null ? '' : `: ${saved.missing.length
                  ? `${plural(saved.missing.length, 'question')} still open: ${saved.missing.slice(0, 6).join(', ')}${saved.missing.length > 6 ? ', …' : ''}`
                  : 'complete'}`}
              </span>
            )}
            {config.saveTarget && <span>Save writes <code className="app-target">{config.saveTarget}</code></span>}
          </div>
        )}
      </header>
      {problem && <p className="app-problem" role="alert">{problem}</p>}
      {agent && <p className="app-agent" role="status">Updated by your agent: {agent.ids.join(', ')}</p>}
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
