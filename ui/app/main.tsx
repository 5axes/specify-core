// Specify on this computer: the app `specify-core open FILE` serves (specify-core docs/open.md).
// One part, from the command; Save writes it back to disk through the command. Nothing is stored in
// the browser and nothing is fetched from anywhere but the page's own origin.
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/600.css';
import '../src/specify/PmiLab.css';
import './app.css';
import { createRoot } from 'react-dom/client';
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  as9102, characteristics, createIntentEngine, drawnFromScene, hasGraphics, noteStatements, SpecifyReview, toCsv, withoutNotesBlock, xlsx,
  type DrawnPmi, type IntentReview, type PmiAnswers, type PmiLabSession, type SpecifyEngine, type SpecifySlots, type WorkerClient,
} from '../src/index';
import { stepBytes } from '../src/model/diff';
import { groupQuestions } from '../src/specify/review';
import { saveFile } from '../src/specify/SpecifyReview';
import { createAnswersStore, type AnswersStore } from './answers';
import { getConfig, getFile, keepAlive, modeOf, type LocalConfig } from './local';
import { engineSaver, type Saver } from './save';

declare const __APP_VERSION__: string;
const DOCS = 'https://github.com/pzfreo/specify-core#readme';
const ISSUES = 'https://github.com/pzfreo/specify-core/issues';

interface Opened {
  session: PmiLabSession; review: IntentReview; engine: SpecifyEngine; drawn: DrawnPmi | null;
  bytes: Uint8Array; store: AnswersStore; saver: Saver;
  /** What SpecifyReview starts from; a new object only when the answers change outside the page (stage 2). */
  external: { answers: PmiAnswers; review: IntentReview };
}

/** Opens the part on the engine: its analysis and mesh, the review of its saved answers (a STEP
 * Specify wrote carries them), and the PMI as drawn. */
async function openPart(worker: WorkerClient, name: string, bytes: Uint8Array): Promise<Opened> {
  const sourceId = name;
  const file = { name, bytes, sourceId };
  const snapshot = await worker.request<{ analysis: PmiLabSession['analysis']; mesh: PmiLabSession['mesh'] }>('open', file);
  const saved: PmiAnswers = snapshot.analysis.resume?.answers ?? {};
  const review = await worker.request<IntentReview>('review', { ...file, answers: saved });
  const store = createAnswersStore(saved);
  const engine: SpecifyEngine = {
    // Each commit of the page's answers comes through here: what the store keeps once Specify accepts them.
    questions: async (_id, answers) => {
      const asked = await worker.request<IntentReview>('review', { ...file, answers });
      store.set(answers, 'page');
      return asked;
    },
    interpret: (_id, answers, question, text) => worker.request('interpret', { analysis: snapshot.analysis, answers, question, text }),
    preview: async (_id, answers) => (await worker.request<IntentReview>('review', { ...file, answers })).intent,
  };
  const session: PmiLabSession = {
    sessionId: sourceId, filename: name, analysis: snapshot.analysis, mesh: snapshot.mesh,
    questions: review.questions, open: review.open, problems: review.problems,
  };
  const scene = drawnFromScene(await worker.request('scene', { ...file, state: { answers: saved } }));
  const shown = scene.ok ? withoutNotesBlock(scene.value) : null;
  return {
    session, review, engine, drawn: shown && hasGraphics(shown) ? shown : null, bytes, store,
    saver: engineSaver(worker, file, store), external: { answers: saved, review },
  };
}

const stem = (name: string) => name.replace(/\.gz$/i, '').replace(/\.(step|stp)$/i, '');

function App() {
  const [config, setConfig] = useState<LocalConfig | null>(null);
  const [opened, setOpened] = useState<Opened | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [refused, setRefused] = useState<{ missing: string[]; message: string } | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const worker = useRef<WorkerClient | null>(null);
  const view = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const exportsBox = useRef<HTMLDivElement>(null);
  const [slots, setSlots] = useState<SpecifySlots | null>(null);
  useEffect(() => setSlots({ view: view.current!, panel: panel.current!, exports: exportsBox.current! }), []);

  useEffect(() => {
    let stop = () => {};
    (async () => {
      const first = await getConfig();
      setConfig(first);
      document.title = `${first.name} — Specify`;
      stop = keepAlive(setConfig);
      if (modeOf(first) === 'diff') throw new Error('This version of the Specify app cannot compare two files. Update specify-core.');
      worker.current = createIntentEngine({ wasmUrl: first.wasmUrl });
      const bytes = await stepBytes(await getFile());
      setOpened(await openPart(worker.current, first.name, bytes));
    })().catch((error: unknown) => setProblem(error instanceof Error ? error.message : String(error)));
    return () => { stop(); worker.current?.dispose(); };
  }, []);

  // The answers given since the last save; closing the tab asks first.
  useEffect(() => opened?.store.subscribe(() => setDirty(opened.store.dirty())), [opened]);
  useEffect(() => {
    if (!dirty) return;
    const guard = (event: BeforeUnloadEvent) => { event.preventDefault(); };
    window.addEventListener('beforeunload', guard);
    return () => window.removeEventListener('beforeunload', guard);
  }, [dirty]);

  const readOnly = config ? modeOf(config) === 'read-only' : true;

  // Save writes the answers given. The defaults are taken only when the person asks (the second button).
  const onSave = async (acceptDefaults: boolean) => {
    if (!opened) return;
    setSaving(true); setProblem(null); setRefused(null);
    try {
      const result = await opened.saver.save({ acceptDefaults });
      if (!result.ok) { setRefused(result); return; }
      opened.store.markSaved(); setDirty(false);
      setSaved(result.path);
      setConfig(await getConfig());
    } catch (error) {
      setProblem(`Not saved: ${error instanceof Error ? error.message : String(error)}`);
    } finally { setSaving(false); }
  };

  // Every answer the STEP is written with, the defaults made explicit: what `specify-core write` takes.
  const downloadAnswers = async () => {
    if (!opened || !config || !worker.current) return;
    const answers = await worker.current.request('answers', { name: config.name, bytes: opened.bytes, sourceId: config.name, answers: opened.store.get() });
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

  return (
    <div className="app">
      <header className="app-header">
        <div className="app-title">
          <strong className="app-brand">Specify</strong>
          {config && <span className="app-file">{config.name}</span>}
          {config?.path && <code className="app-path">{config.path}</code>}
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
            </>
          )}
        </div>
        {config && (
          <div className="app-paths">
            {saved && <span className="app-saved" role="status">Saved <code>{saved}</code></span>}
            {config.saveTarget && <span>Save writes <code className="app-target">{config.saveTarget}</code></span>}
          </div>
        )}
      </header>
      {problem && <p className="app-problem" role="alert">{problem}</p>}
      {refused && (
        <p className="app-problem" role="alert">
          Not saved: {refused.missing.length ? `still to answer: ${refused.missing.join(', ')}` : refused.message}.
          {' '}Answer them, or take Specify's default for each:{' '}
          <button type="button" onClick={() => void onSave(true)} disabled={saving}>Save with defaults</button>
        </p>
      )}
      <main className="app-main">
        {!opened && !problem && <p className="app-loading">Opening {config?.name ?? 'the part'}…</p>}
        <div className="app-slot app-view" ref={view} />
        <div className="app-slot app-panel" ref={panel} />
        {opened && slots && (
          <SpecifyReview
            engine={opened.engine} session={opened.session} slots={slots} readOnly={readOnly} drawn={opened.drawn}
            externalAnswers={opened.external.answers} externalReview={opened.external.review} disabled={saving}
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
