import type { Summary } from './types'

/**
 * Reduces an `akn.statute.v1` document to the handful of numbers the UI shows.
 *
 * Ported from the server's `summarise.mjs`, which is the behaviour this replaces, and
 * it keeps the one thing that would otherwise be silent: a PDF with no text layer. The
 * `indian-statute` profile has no OCR, so a scanned document yields an empty tree rather
 * than an error, and a parse that "succeeded" with nothing in it is worth saying out loud.
 */

interface Node {
  kind?: string
  children?: Node[]
}

interface Citation {
  resolved?: boolean
  external?: boolean
  relation?: string
}

interface Document {
  schema_version?: string
  provisions?: unknown[]
  citations?: Citation[]
  footnotes?: unknown[]
  diagnostics?: unknown[]
  akomaNtoso?: {
    act?: {
      meta?: { title?: string }
      body?: Node[]
    }
  }
}

function countKinds(nodes: Node[]): Record<string, number> {
  const counts: Record<string, number> = {}
  const walk = (node: Node) => {
    const kind = node.kind
    if (kind !== undefined) {
      counts[kind] = (counts[kind] ?? 0) + 1
    }
    for (const child of node.children ?? []) {
      walk(child)
    }
  }
  for (const node of nodes) {
    walk(node)
  }
  return counts
}

export function summarise(value: unknown): Summary {
  const document = (value ?? {}) as Document
  const provisions = Array.isArray(document.provisions) ? document.provisions : []
  const citations = Array.isArray(document.citations) ? document.citations : []
  const footnotes = Array.isArray(document.footnotes) ? document.footnotes : []
  const diagnostics = Array.isArray(document.diagnostics) ? document.diagnostics : []

  // Counted over the whole tree rather than the roots: `body.length` would count only
  // top-level nodes, so an Act that nests a section under a chapter would report fewer
  // sections than it contains, under a label that reads as a total.
  const kinds = countKinds(document.akomaNtoso?.act?.body ?? [])

  const resolved = citations.filter((citation) => citation.resolved).length
  const external = citations.filter((citation) => citation.external).length
  // Kept as an explicit predicate rather than `total - resolved - external`: a citation
  // that is both resolved and external would make that subtraction wrong, and this
  // mirrors what the engine actually marks.
  const unresolvedInternal = citations.filter(
    (citation) => !citation.resolved && !citation.external,
  ).length

  const byRelation: Record<string, number> = {}
  for (const citation of citations) {
    const relation = citation.relation ?? 'unknown'
    byRelation[relation] = (byRelation[relation] ?? 0) + 1
  }

  const summary: Summary = {
    schemaVersion: document.schema_version ?? null,
    title: document.akomaNtoso?.act?.meta?.title ?? null,
    sections: kinds.section ?? 0,
    provisions: provisions.length,
    citations: citations.length,
    footnotes: footnotes.length,
    resolved,
    unresolvedInternal,
    external,
    byRelation,
    kinds,
    diagnostics: diagnostics.length,
  }

  if (provisions.length === 0) {
    summary.warning =
      'No provisions were extracted. The most likely cause is that this PDF has no text ' +
      'layer, which is the case for scanned documents. This build has no OCR.'
    summary.likelyScanned = true
  } else if (unresolvedInternal > 0) {
    summary.note =
      `${unresolvedInternal} internal cross-reference` +
      `${unresolvedInternal === 1 ? '' : 's'} pointed at a provision that does not exist ` +
      'in this document.'
  }

  return summary
}