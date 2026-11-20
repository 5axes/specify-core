// The package's public surface.
export { default as StepDiff, type StepDiffProps } from './components/StepDiff';
export { default as AssemblyDiffView, type AssemblyRow, type StepFile } from './components/AssemblyDiffView';
export { SpecifyReview, type SpecifyEngine, type SpecifySlots } from './specify/SpecifyReview';
export { drawnFromScene, withoutNotesBlock, hasGraphics, type DrawnPmi } from './specify/drawn';
export { readOnlyEngine } from './specify/read-only';
export { createIntentEngine } from './engine/intent-engine';
export { WorkerClient } from './engine/rpc';
export * from './specify/characteristics';
export { as9102, type FaiPart } from './specify/as9102';
export { xlsx, workbookFiles, zip, type Cell, type Sheet } from './model/xlsx';
export * from './model/diff-pmi';
export * from './model/diff';
export type * from './types/pmi';
export type {
  Box, CameraState, FaceIds, FaceRef, FaceStyle, FaceSample, HostPointerEvent, HostView, HostViewInfo, Mat4, MeshOptions, Vec3,
} from './specify/host-view';
