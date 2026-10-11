// What a question's options mean, in a sentence each, for someone who has not
// got ISO 286 by heart. Pure, no DOM. Fits are described with the mating part
// they are usually paired with; the label on the part shows the actual limits.
import type { PmiQuestion } from '../types/pmi';

const HOLE_FIT: Readonly<Record<string, string>> = {
  H7: 'The usual precise hole: dowel pins, bearings, locating spigots. Reamed or bored. '
    + 'With a g6 shaft it slides, with h6 it locates snugly, with p6 it is a press fit.',
  H8: 'Close running fit (with an f7 shaft): accurate location for parts that turn or slide.',
  H9: 'Free running fit (with a d9 shaft): moves easily, not for accurate location.',
  H11: 'Loose running fit: wide limits, often just drilled. Cheap where location does not matter.',
};

const SHAFT_FIT: Readonly<Record<string, string>> = {
  h6: 'Locational clearance (in an H7 hole): a snug fit assembled by hand that can still come apart.',
  h7: 'Like h6 with wider limits: a sliding fit in an H8 hole.',
  g6: 'Sliding fit (in an H7 hole): turns and slides freely with precise location, e.g. a spigot.',
  f7: 'Close running fit (in an H8 hole): a shaft turning in a plain bearing.',
  k6: 'Transition fit (in an H7 hole): accurate location, may need a light tap to assemble, e.g. a gear or pulley on a keyed shaft.',
  m6: 'Transition fit, tighter than k6: usually needs pressing; parts stay put but can be removed.',
  n6: 'Tight transition fit (in an H7 hole): pressed on, for location that must not shift.',
  p6: 'Press fit (in an H7 hole): pressed in for good, e.g. a bush or dowel. Alignment, not torque.',
  js6: 'Symmetric about the nominal size (±): a transition fit, e.g. hubs and bearing seats that must come apart.',
};

const KNURL: Readonly<Record<string, string>> = {
  straight: 'Ridges along the axis: grip, or to key into a plastic moulding.',
  diamond: 'Crossed ridges: a grip for fingers, on knobs and handles.',
};

const GENERAL_CLASS: Readonly<Record<string, string>> = {
  f: 'Fine: ±0.1 on a 6–30 mm size. Precision machining.',
  m: 'Medium: ±0.2 on a 6–30 mm size. Ordinary machining; the usual choice.',
  c: 'Coarse: ±0.5 on a 6–30 mm size. Rough machining, fabrication.',
  v: 'Very coarse: ±1 on a 6–30 mm size. Rough work, flame cutting.',
};

const ROUGHNESS: Readonly<Record<string, string>> = {
  '0.4': 'Lapped or fine ground: sealing faces and precision bores. Costly.',
  '0.8': 'Ground or fine turned: bearing seats, sliding fits, O-ring grooves.',
  '1.6': 'Fine machining: fitted bores and shafts, faces that mate.',
  '3.2': 'Ordinary good machining: the usual default.',
  '6.3': 'Rough machining: faces that touch nothing.',
  '12.5': 'Rough cut: clearance faces only. Cheapest.',
};

/** One sentence on what choosing ``option`` for ``q`` means, or null if there is nothing to add. */
export function optionHint(q: PmiQuestion, option: string): string | null {
  const [kind, value = ''] = option.split(':');
  if (q.id.startsWith('hole.location:')) {
    return option === 'general'
      ? "Located by dimensions on the drawing, held to the part's general tolerance."
      : `Each hole within ${option} of its distance from the part's edges.`;
  }
  if (option === 'general') {
    return "No fit: the size is held only by the part's general tolerance.";
  }
  if (option === 'none') return 'Nothing beyond the general tolerance.';
  const ra = /^Ra (\d*\.?\d+)$/.exec(option);
  if (ra?.[1]) return ROUGHNESS[ra[1]] ?? null;
  if (option.startsWith('Break sharp edges')) {
    return `Every sharp edge is chamfered or rounded to at most ${option.split(' ').at(-2)} mm, so it does not cut or chip.`;
  }
  if (option === 'Deburr only') return 'Burrs are removed; edges are otherwise left as machined.';
  if (option === 'No edge note') return 'Nothing is said about edges.';
  if (option === 'position tolerances') {
    return 'Each hole is located by a position tolerance from datums: for parts that bolt or pin to others.';
  }
  if (option === '± dimensions') {
    return 'Holes are dimensioned on the drawing and held to the general tolerance. No datums are needed, '
      + 'unless you add a perpendicularity or parallelism, which adds datum A.';
  }
  if (kind === 'fit') return HOLE_FIT[value] ?? `${value} fit (ISO 286).`;
  if (kind === 'clearance') {
    return value
      ? `A ${value} bolt passes through freely (ISO 273); the position tolerance follows from the gap.`
      : 'A bolt passes through freely; no bolt size is stated.';
  }
  if (kind === 'tapped') return `Drilled and tapped for an ${value} thread; the drawing calls up the thread.`;
  if (kind === 'thread') return `An external ${value} thread.`;
  if (kind === 'knurl') return KNURL[value] ?? null;
  if (option in SHAFT_FIT) return SHAFT_FIT[option] ?? null;
  const general = /^ISO 2768-(\w)$/.exec(option);
  if (general?.[1]) return GENERAL_CLASS[general[1]] ?? null;
  if (/^[A-Z](\|[A-Z])*$/.test(option)) {
    const order = ['primary', 'secondary', 'tertiary'];
    return `Measured from ${option.split('|').map((d, i) => `${d} (${order[i] ?? ''})`).join(', ')}.`;
  }
  if (/^\d*\.?\d+$/.test(option)) return zoneHint(q, option);
  return null;
}

function zoneHint(q: PmiQuestion, size: string): string | null {
  if (q.characteristic === 'position' && q.id.startsWith('hole.position:')) {
    return `Each hole's axis must lie in a cylinder Ø${size} around where it should be.`;
  }
  if (q.characteristic === 'runout') {
    return `As the part turns on its axis, the diameter may wobble by at most ${size}.`;
  }
  const kind = q.characteristic;
  if (kind === 'flatness') return `The face must lie between two parallel planes ${size} apart.`;
  if (kind === 'perpendicularity') return `Must lie within a zone ${size} wide, square to the datum it refers to.`;
  if (kind === 'parallelism') return `Must lie within a zone ${size} wide, parallel to the datum it refers to.`;
  if (kind === 'position') return `Must lie within a zone ${size} across around where it should be.`;
  return null;
}
