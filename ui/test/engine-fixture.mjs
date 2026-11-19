// The same released engine served to the local app, rather than the package's copy.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import init, { IntentEngine, pmiScene } from '../public/wasm/intent/specify_core_wasm.js';

await init({ module_or_path: readFileSync(new URL('../public/wasm/intent/specify_core_wasm_bg.wasm', import.meta.url)) });
export { IntentEngine };

/** Read the real writer's private history via its semantic reader; never fabricate a schema. */
export function characteristicHistory(bytes) {
  const scene = JSON.parse(pmiScene(bytes));
  const attribute = scene.pmi.pmi.attributes.find(({ value }) => value.name === 'specify characteristic numbers' && value.on === null);
  assert.ok(attribute, "the saved STEP contains the writer's characteristic-number history");
  const item = attribute.value.items.find(([name]) => name === 'history');
  assert.ok(item, 'the private property contains its history item');
  return JSON.parse(item[1].text);
}

/** Validate every WASM operation used while opening a local app part, including the scene. */
export function validateOpening(name, bytes, answers) {
  const engine = new IntentEngine(name, bytes);
  try {
    const snapshot = JSON.parse(engine.snapshot());
    const given = JSON.stringify(answers);
    const accepted = engine.acceptDefaults(given);
    const review = JSON.parse(engine.review(accepted));
    engine.review(given);
    const scene = JSON.parse(engine.pmiScene(JSON.stringify({ answers: JSON.parse(accepted) })));
    return { snapshot, review, scene };
  } finally { engine.free(); }
}
