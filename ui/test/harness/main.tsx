// A minimal host for <StepDiff>: two file inputs, and what the component tells its host recorded
// on `window` for the browser test to read. The Specify WASM is the dependency, as a host gets it.
import { createRoot } from 'react-dom/client';
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  createIntentEngine, drawnFromScene, hasGraphics, SpecifyReview, StepDiff, withoutNotesBlock, type DrawnPmi, type StepFile,
} from '../../src/index';
import { stepBytes } from '../../src/model/diff';

declare global {
  interface Window {
    __events: string[];
    /** The Specify host's calls from SpecifyReview ('questions' | 'interpret' | 'preview', and
     * 'onKeep:<ids>' with &keep=host), when ?specify=1. */
    __calls: string[];
    __parts: (string | null)[];
    __setAnswers(answers: unknown): void;
    __setPart(name: string | null): void;
  }
}
window.__events = [];
window.__parts = [];
window.__calls = [];

function Slot({ label, file, pick }: { label: string; file: StepFile | null; pick: (file: StepFile) => void }) {
  return (
    <label className="diff-slot">
      <span className="diff-slot-label">{label}</span>
      <input type="file" onChange={async event => {
        const chosen = event.target.files?.[0];
        if (chosen) pick({ name: chosen.name.replace(/\.gz$/, ''), bytes: await stepBytes(chosen) });
      }} />
      <span className="diff-slot-file">{file ? file.name : 'Choose a STEP file'}</span>
    </label>
  );
}

// ?wasmUrl=… : the engine the host serves itself (the override), not the dependency.
const wasmUrl = new URLSearchParams(location.search).get('wasmUrl') ?? undefined;

function Harness() {
  const [before, setBefore] = useState<StepFile | null>(null);
  const [after, setAfter] = useState<StepFile | null>(null);
  const [answers, setAnswers] = useState<unknown>({});
  const [part, setPart] = useState<string | null>(null);
  useEffect(() => { window.__setAnswers = setAnswers; window.__setPart = setPart; }, []);
  return (
    <main className="diff-page">
      <div className="diff-slots">
        <Slot label="Before" file={before} pick={setBefore} />
        <Slot label="After" file={after} pick={setAfter} />
      </div>
      {before && after && (
        <StepDiff
          before={before} after={after} answers={answers} part={part} wasmUrl={wasmUrl}
          onPart={name => { window.__parts.push(name); setPart(name); }}
          onEvent={name => window.__events.push(name)}
        />
      )}
    </main>
  );
}

// ?specify=1 (and &readOnly=1): <SpecifyReview> on a picked STEP instead. Its engine is the worker's, and
// records each call the component makes of it on window.__calls.
const query = new URLSearchParams(location.search);
const readOnly = query.get('readOnly') === '1';
// &keep=host: the host keeps suggestions itself, as the editor does (onKeep, and kept for the ticks);
// otherwise SpecifyReview keeps one by answering it with its default, as the local app does.
const hostKeeps = query.get('keep') === 'host';

const noAnswers = {};

function SpecifyHost() {
  // Made here, not at module level: the StepDiff page starts no extra worker.
  const worker = useMemo(() => createIntentEngine({ wasmUrl }), []);
  const [open, setOpen] = useState<{ session: any; review: any; engine: any; drawn: DrawnPmi | null } | null>(null);
  const view = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const exports = useRef<HTMLDivElement>(null);
  const [slots, setSlots] = useState<{ view: HTMLElement; panel: HTMLElement; exports: HTMLElement } | null>(null);
  useEffect(() => setSlots({ view: view.current!, panel: panel.current!, exports: exports.current! }), []);
  const [kept, setKept] = useState<readonly string[]>([]);
  const onKeep = (ids: readonly string[]) => { window.__calls.push(`onKeep:${ids.join(',')}`); setKept((was) => [...was, ...ids]); };
  const pick = async (file: File) => {
    const name = file.name.replace(/\.gz$/, '');
    const bytes = await stepBytes(file);
    const sourceId = name;
    const snapshot: any = await worker.request('open', { name, bytes, sourceId });
    const review: any = await worker.request('review', { name, bytes, sourceId, answers: {} });
    const call = (operation: string, answers: unknown) => worker.request<any>(operation, { name, bytes, sourceId, answers });
    const engine = {
      questions: async (_id: string, answers: unknown) => { window.__calls.push('questions'); return call('review', answers); },
      interpret: async (_id: string, answers: unknown, question: string, text: string) => {
        window.__calls.push('interpret');
        return worker.request<any>('interpret', { analysis: snapshot.analysis, answers, question, text });
      },
      preview: async (_id: string, answers: unknown) => { window.__calls.push('preview'); return (await call('review', answers)).intent; },
    };
    const session = { sessionId: sourceId, filename: name, analysis: snapshot.analysis, mesh: snapshot.mesh, questions: review.questions, problems: review.problems };
    // The PMI as drawn, as the editor asks for it: the worker's call, not the host engine's.
    const saved = snapshot.analysis.resume?.answers ?? {};
    const scene = drawnFromScene(await worker.request('scene', { name, bytes, sourceId, state: { answers: saved } }));
    const shown = scene.ok ? withoutNotesBlock(scene.value) : null;
    setOpen({ session, review, engine, drawn: shown && hasGraphics(shown) ? shown : null });
  };
  return (
    <main className="diff-page">
      <input id="specify-file" type="file" onChange={event => { const chosen = event.target.files?.[0]; if (chosen) void pick(chosen); }} />
      <div id="specify-view" style={{ height: 520 }} ref={view} />
      <div id="specify-panel" ref={panel} />
      <div id="specify-exports" ref={exports} />
      {open && slots && (
        <SpecifyReview
          engine={open.engine} session={open.session} slots={slots} readOnly={readOnly} drawn={open.drawn}
          externalAnswers={noAnswers} externalReview={open.review} disabled={false}
          onExport={action => action()} {...(hostKeeps ? { onKeep, kept } : {})}
        />
      )}
    </main>
  );
}

createRoot(document.getElementById('root')!).render(query.get('specify') === '1' ? <SpecifyHost /> : <Harness />);
