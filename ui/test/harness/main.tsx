// A minimal host for <StepDiff>: two file inputs, and what the component tells its host recorded
// on `window` for the browser test to read. The Specify WASM is the dependency, as a host gets it.
import { createRoot } from 'react-dom/client';
import { useEffect, useState } from 'react';
import { StepDiff, type StepFile } from '../../src/index';
import { stepBytes } from '../../src/model/diff';

declare global {
  interface Window {
    __events: string[];
    __parts: (string | null)[];
    __setAnswers(answers: unknown): void;
    __setPart(name: string | null): void;
  }
}
window.__events = [];
window.__parts = [];

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
          before={before} after={after} answers={answers} part={part}
          onPart={name => { window.__parts.push(name); setPart(name); }}
          onEvent={name => window.__events.push(name)}
        />
      )}
    </main>
  );
}

createRoot(document.getElementById('root')!).render(<Harness />);
