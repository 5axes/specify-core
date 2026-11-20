import type { PmiIntent, PmiIntentItem } from '../types/pmi';

/** The IDs in specify-core 0.2.0's pmiScene identify semantic requirements independently of
 * values. Their number metadata comes from the same allocation used by the STEP writer. */
function sceneId(item: PmiIntentItem): string {
  const planner = item['planner'] as { required?: { id?: string } } | undefined;
  if (planner?.required?.id) return `planner:${planner.required.id}`;
  if (typeof item['letter'] === 'string') return `datum:${item['letter']}`;
  const question = item.question ?? '';
  const feature = question.startsWith('extra.') ? question : item['feature'];
  const target = typeof feature === 'string' && feature ? feature : `faces:${JSON.stringify(item.faces)}`;
  const base = `${item.kind ?? 'item'}:${target}`;
  return item.kind === 'location'
    ? `${base}:${JSON.stringify(item['reference'] ?? null)}${question ? `:${question}` : ''}`
    : base;
}

export interface NumberScene {
  specify: Record<string, { number?: number }>;
}

/** Preserve stored numbers and allocate new ones by asking the engine, never by row order. */
export function withCharacteristicNumbers(intent: PmiIntent, scene: NumberScene): PmiIntent {
  const numbered = <T extends PmiIntentItem>(item: T): T => ({
    ...item, number: scene.specify[sceneId(item)]?.number,
  });
  const partNumbers = Object.fromEntries(Object.keys(intent.part).flatMap((key) => {
    const number = scene.specify[`part.${key}`]?.number;
    return number === undefined ? [] : [[key, number]];
  }));
  return {
    ...intent, datums: intent.datums.map(numbered), requirements: intent.requirements.map(numbered),
    general: intent.general?.map(numbered), partNumbers,
  };
}
