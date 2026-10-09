/** Counts the UI shows for one parsed document. Produced by `summarise.ts`. */
export interface Summary {
  schemaVersion: string | null
  title: string | null
  sections: number
  provisions: number
  citations: number
  footnotes: number
  resolved: number
  unresolvedInternal: number
  external: number
  byRelation: Record<string, number>
  kinds: Record<string, number>
  diagnostics: number
  warning?: string
  note?: string
  likelyScanned?: boolean
}

/**
 * Where a parse has got to.
 *
 * The server build had a queue, a job id and an SSE stream, so it could report upload
 * progress and cancel from outside. There is no server here: the module runs in this
 * tab, so a parse is described by a phase and an elapsed time and nothing else.
 */
export type ParsePhase =
  | 'idle'
  | 'loading-module'
  | 'reading-file'
  | 'extracting'
  | 'parsing'
  | 'serialising'
  | 'done'
  | 'error'

export interface ParseProgress {
  phase: ParsePhase
  elapsedMs: number
}

/**
 * The engine's own size cap, in bytes. Read from the module at load time rather than
 * hard-coded here, so the browser refuses exactly what the parser refuses.
 */
export const MAX_UPLOAD_BYTES = 100 * 1024 * 1024

/** Relation identifiers the engine emits, for the labels in the UI. */
export const RELATION_LABELS: Record<string, string> = {
  notwithstanding: 'notwithstanding',
  subject_to: 'subject to',
  reference: 'plain reference',
}