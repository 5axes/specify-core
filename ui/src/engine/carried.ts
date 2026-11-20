interface CarriedFile {
  payload(name: string): string | undefined;
  original(): Uint8Array | undefined;
  free(): void;
}

export function readCarried(module: { IntentEngine: {
  new (name: string, bytes: Uint8Array): CarriedFile;
  openPart(name: string, bytes: Uint8Array, part: number): CarriedFile;
} }, name: string, bytes: Uint8Array, payload: string, part?: number) {
  const file = part === undefined ? new module.IntentEngine(name, bytes) : module.IntentEngine.openPart(name, bytes, part);
  try { return { project: file.payload(payload) ?? null, original: file.original() ?? null }; }
  finally { file.free(); }
}
