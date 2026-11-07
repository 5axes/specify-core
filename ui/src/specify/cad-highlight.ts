import type { Viewer } from 'three-cad-viewer';

/** The viewer accepts one hover path; multiple paths use temporary face highlights. */
export function lightGroup(highlight: NonNullable<Viewer['nestedGroup']['highlight']>, paths: readonly string[]) {
  const added: number[] = [];
  for (const component of highlight.registry.entries()) {
    if (component.topo !== 'face' || !paths.some(path => component.path === path || component.path.startsWith(`${path}/`))
        || highlight.isSelected(component.id)) continue;
    highlight.setSelected(component.id, true);
    added.push(component.id);
  }
  return () => { for (const id of added) highlight.setSelected(id, false); };
}
