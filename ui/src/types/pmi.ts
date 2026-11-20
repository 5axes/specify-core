// Preview-only PMI lab (#859): pmi-assist on the server asks for a part's
// manufacturing intent and writes it into the STEP before it is drawn. These
// are the shapes pmi-assist produces, passed through unchanged by the server.

export type PmiVec = readonly [number, number, number];

export interface PmiBinding {
  readonly source_sha256: string;
  readonly face_count: number;
  readonly loader_version: number;
}

export interface PmiFace {
  readonly id: number;
  readonly kind: 'plane' | 'cylinder' | 'cone' | 'torus' | 'other';
  readonly area: number;
  readonly centroid: PmiVec;
  /** Outward normal of a plane; axis of a surface of revolution. */
  readonly direction?: PmiVec;
  readonly origin?: PmiVec;
  readonly radius?: number;
}

export interface PmiFeature {
  readonly id: string;
  readonly family: string;
  readonly record: Readonly<Record<string, unknown>>;
  readonly faces: readonly number[];
  readonly members?: readonly string[];
  readonly notes?: readonly string[];
}

/** PMI the uploaded file already states; a note is a thread or knurl pmi-assist wrote. */
export interface PmiExisting {
  /** Stable ID_ATTRIBUTE read from this requirement in the STEP. */
  readonly number?: number;
  readonly kind: 'dimension' | 'geometric_tolerance' | 'datum' | 'note';
  readonly type: string;
  readonly faces?: readonly number[];
  readonly value?: number;
  readonly upper?: number;
  readonly lower?: number;
  readonly datums?: readonly string[];
}

export interface PmiAnalysis {
  readonly binding: PmiBinding;
  readonly faces: readonly PmiFace[];
  readonly features: readonly PmiFeature[];
  readonly existing: readonly PmiExisting[];
  /** The material and general tolerance the file already states; not asked again. */
  readonly part?: Readonly<Record<string, string>>;
  /**
   * A STEP Specify wrote, opened again: the answers it was written from, to carry
   * on editing. Its PMI is theirs, so it is not shown as the file's own.
   */
  readonly resume?: {
    readonly answers: PmiAnswers;
    /** Saved answers Specify alpha.7+ could not match to this geometry: not applied, and not written again. */
    readonly unresolved?: readonly { readonly id: string; readonly feature?: string | null; readonly answer: unknown; readonly reason: string }[];
    /** Specify alpha.8+: why the answers were matched by feature ID, without checking the faces (a pre-alpha file). */
    readonly unverified?: string;
  };
}

/** Triangles of the part, ranged face by face in the analysis's face numbering. */
export interface PmiMesh {
  readonly format: 'pmi-assist-mesh';
  readonly version: number;
  readonly binding: PmiBinding;
  readonly bbox: readonly [PmiVec, PmiVec];
  readonly positions: readonly number[];
  readonly indices: readonly number[];
  readonly faces: readonly (readonly [number, number])[];
  readonly missing: readonly number[];
  readonly edges: readonly (readonly number[])[];
}

export type PmiAttention = 'required' | 'check' | 'routine' | 'refine';

export interface PmiQuestion {
  readonly title?: string;
  /** A short name for rows and messages ("Other notes", "Datum C"): no question mark or input hint. */
  readonly label?: string;
  readonly characteristic?: 'flatness' | 'perpendicularity' | 'parallelism' | 'position' | 'runout';
  readonly diameter?: number;
  readonly depth?: number;
  readonly id: string;
  readonly kind: 'choice' | 'text' | 'faces';
  readonly prompt: string;
  readonly faces: readonly number[];
  readonly default: unknown;
  readonly why: string;
  readonly options?: readonly string[];
  readonly feature?: string;
  readonly frame?: string;
  readonly attention: PmiAttention;
  readonly reason?: string;
  /** Why the default is what it is: shown as a choice made for the person. */
  readonly basis?: string;
  readonly features?: readonly string[];
  /** A hole's or turned diameter's options by purpose, code and callout. */
  readonly choices?: readonly PmiChoice[];
  /** For a text question: common answers to offer as it is typed; any text is accepted. */
  readonly suggestions?: readonly PmiSuggestion[];
  /** Specify writes its suggestion when it is not answered (an assembly part's parts-list row). */
  readonly optional?: boolean;
}

/** One option of a question: what it is for, its code, and what it puts on the drawing. */
export interface PmiChoice {
  readonly value: string;
  readonly purpose: string;
  readonly label: string;
  readonly code: string;
  readonly callout: string;
}

export interface PmiSuggestion {
  readonly value: string;
  readonly group: string;
  /** Other names it is known by, matched as it is typed. */
  readonly also: readonly string[];
}

/** A typed spec read by pmi-assist as an answer. */
export interface PmiInterpretation {
  readonly value: string;
  readonly code: string;
  readonly callout?: string;
}

export type PmiAnswers = Readonly<Record<string, unknown>>;

export interface PmiQuestionSet {
  readonly questions: readonly PmiQuestion[];
  readonly open: readonly string[];
  /** What is incomplete or does nothing so far, for a person to put right. */
  readonly problems?: readonly PmiProblem[];
}

export interface PmiProblem {
  readonly severity: 'warning' | 'info';
  readonly message: string;
  readonly faces: readonly number[];
  /** The question, or added requirement (``extra.<id>``), it is about. */
  readonly about: string;
}

/**
 * A requirement a person added where no question asked, as its answer. A
 * ``size`` or ``distance`` is a dimension called out of the general tolerance.
 */
export interface PmiExtra {
  readonly faces: readonly number[];
  readonly kind:
    | 'flatness' | 'perpendicularity' | 'parallelism' | 'fit' | 'finish' | 'profile' | 'runout' | 'total_runout'
    | 'size' | 'distance';
  /**
   * A tolerance in mm, a fit's code (H7, g6), a finish's roughness (Ra 0.8),
   * or for a size or distance ±0.05 or limits +0.1/-0 (a size may take a fit).
   */
  readonly value: string;
  readonly datums?: readonly string[];
  /** A distance's flat faces it is measured from; ``faces`` are what it runs to. */
  readonly reference?: readonly number[];
}

/** One datum or requirement of the intent; its fields depend on its kind. */
export interface PmiIntentItem {
  /** The stored or engine-planned characteristic number. Absent when no entity is numbered. */
  readonly number?: number;
  readonly kind?: string;
  readonly faces: readonly number[];
  readonly question?: string;
  /** True when its question was not answered and a default stands in. */
  readonly default?: boolean;
  /** A datum the file already has: cited, never written again. */
  readonly existing?: boolean;
  readonly [field: string]: unknown;
}

export interface PmiIntent {
  /** Numbered part requirements such as surface finish. Material and text notes are unnumbered. */
  readonly partNumbers?: Readonly<Record<string, number>>;
  readonly part: Readonly<Record<string, string>>;
  readonly datums: readonly (PmiIntentItem & { readonly letter: string })[];
  readonly requirements: readonly PmiIntentItem[];
  /**
   * The dimensions the general tolerance covers -- diameters, lengths from the
   * datums, hole locations -- each with its ISO 2768 ±: listed and shown, not
   * written into the STEP.
   */
  readonly general?: readonly PmiIntentItem[];
  /** Where each label lands, by its sorted face ids joined with commas: worked out on the solid. */
  readonly anchors?: Readonly<Record<string, PmiAnchor>>;
}

/** A point on a face and the way a leader leaves it through free space. */
export interface PmiAnchor {
  readonly at: PmiVec;
  readonly out: PmiVec;
  readonly clear: boolean;
}

export interface PmiLabSession extends PmiQuestionSet {
  readonly sessionId: string;
  readonly answers?: PmiAnswers;
  readonly revision?: number;
  readonly filename: string;
  readonly analysis: PmiAnalysis;
  readonly mesh: PmiMesh;
  /** Where the part came from, as the engine's snapshot gives it: its faces' STEP ``ADVANCED_FACE``
   * entities, by face, and Specify's index of the part. A host's view identifies faces by them. */
  readonly provenance?: { readonly face_entities?: readonly number[]; readonly part?: number | null };
}

export interface PmiFinishReport {
  readonly filename: string;
  readonly written: readonly string[];
  readonly notWritten: readonly string[];
  readonly warnings: readonly string[];
}

/** A lab failure: an EngineError, with the questions still unanswered if that was why. */
export interface PmiLabError {
  readonly code: string;
  readonly message: string;
  readonly missing?: readonly string[];
}

export interface PmiLabEngine {
  /**
   * Upload a STEP file; pmi-assist analyses and meshes it in the background and
   * asks its questions. ``onProgress`` hears the seconds spent analysing so far.
   */
  open(file: File, onProgress?: (seconds: number) => void): Promise<PmiLabSession>;
  questions(sessionId: string, answers: PmiAnswers): Promise<PmiQuestionSet>;
  /**
   * What was typed for ``question``, read as the answer it means and checked
   * against the feature; a spec that does not fit is refused with the reason.
   */
  interpret(sessionId: string, answers: PmiAnswers, question: string, text: string): Promise<PmiInterpretation>;
  /** The intent the answers so far amount to, defaults standing in. */
  preview(sessionId: string, answers: PmiAnswers): Promise<PmiIntent>;
  /** Write the answers, untouched defaults accepted, into the STEP the session keeps. */
  finish(sessionId: string, answers: PmiAnswers): Promise<PmiFinishReport>;
  /**
   * The finished STEP, for a registered user to keep; an anonymous user is refused
   * (``register_required``). A drawing is made from it by reference instead
   * (lib/specify-handoff), so its bytes need not reach the browser.
   */
  download(sessionId: string, filename: string): Promise<File>;
  /**
   * A one-time code for an anonymous trial's session. Signing in is a different
   * user; presenting the code with ``claim`` moves the analysed part to them.
   */
  claimCode(sessionId: string): Promise<string>;
  claim(sessionId: string, code: string): Promise<void>;
  close(sessionId: string): Promise<void>;
}

export interface IntentReview {
  questions: PmiQuestion[];
  open: string[];
  intent: PmiIntent;
  problems: PmiProblem[];
}

/** One distinct part of an assembly, as Specify's ``parts`` lists it: where each instance is (a 3×4
 * matrix, rotation then translation, in rows) and how many faces it has. */
export interface AssemblyPart {
  part: number; name: string; product_id: string; faces: number; instances: number;
  same_as?: number | null;
  /** The first part's face index for each face in this copy's own order. */
  faces_of_first?: number[] | null;
  placements: number[][][];
}

export type PartTint = 'required' | 'check' | 'problem';
