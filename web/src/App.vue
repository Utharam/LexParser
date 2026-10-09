<script setup lang="ts">
import { computed, onBeforeUnmount, ref, shallowRef } from 'vue'
import { BRAND, COMPANION, MARK, SUITE, SUITE_URL } from './brand'
import { summarise } from './summarise'
import { MAX_UPLOAD_BYTES, RELATION_LABELS, type ParsePhase, type Summary } from './types'
import { loadParser, type LoadedParser } from './wasm'

/**
 * LexParser — the indian-statute parser, running in this tab as WebAssembly.
 *
 * There is no server: nothing is uploaded, and the PDF never leaves the machine. What
 * that removes is everything the previous server-based version needed — a job queue,
 * polling, an SSE stream, a `DELETE` to cancel — and what it costs is cancellation and
 * determinate progress, both handled explicitly below rather than papered over.
 */

const file = ref<File | null>(null)
const dragOver = ref(false)
const fatal = ref<string | null>(null)

const input = ref<HTMLInputElement | null>(null)

const phase = ref<ParsePhase>('idle')
const startedAt = ref(0)
const elapsed = ref(0)
const summary = ref<Summary | null>(null)
/** The full document, kept only to be offered as a download. */
const documentJson = shallowRef<string | null>(null)

const parser = shallowRef<LoadedParser | null>(null)
const moduleError = ref<string | null>(null)

let ticker: number | null = null

const busy = computed(
  () => phase.value !== 'idle' && phase.value !== 'done' && phase.value !== 'error',
)

const barClass = computed(() => {
  if (phase.value === 'done') return 'bar done'
  if (phase.value === 'error') return 'bar failed'
  return 'bar'
})

const phaseLabel = computed(() => {
  switch (phase.value) {
    case 'loading-module':
      return 'Loading the parser'
    case 'reading-file':
      return 'Reading the file'
    case 'extracting':
      return 'Reading text and layout from the PDF'
    case 'parsing':
      return 'Identifying sections, clauses and citations'
    case 'serialising':
      return 'Writing Akoma Ntoso JSON'
    case 'done':
      return 'Done'
    case 'error':
      return 'Failed'
    default:
      return ''
  }
})

/**
 * The structural breakdown, largest kind first.
 *
 * Shows the whole `kinds` map rather than a hand-picked subset: someone about to pipe
 * this into a downstream system wants to see what the parser thinks it found, including
 * the parts that should be zero.
 */
const kindRows = computed(() => {
  const kinds = summary.value?.kinds ?? {}
  return Object.entries(kinds)
    .map(([kind, count]) => ({ kind, count, label: kind.replace(/_/g, ' ') }))
    .sort((a, b) => b.count - a.count)
})

const kindMax = computed(() => kindRows.value.reduce((peak, row) => Math.max(peak, row.count), 0) || 1)

function human(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`
  const s = ms / 1000
  return s < 60 ? `${s.toFixed(1)} s` : `${Math.floor(s / 60)}m ${Math.round(s % 60)}s`
}

function megabytes(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

function startTimer() {
  startedAt.value = performance.now()
  elapsed.value = 0
  ticker = window.setInterval(() => {
    elapsed.value = performance.now() - startedAt.value
  }, 100)
}

function stopTimer() {
  if (ticker !== null) {
    window.clearInterval(ticker)
    ticker = null
  }
}

function pick(chosen: File | undefined | null) {
  fatal.value = null
  summary.value = null
  documentJson.value = null
  phase.value = 'idle'
  elapsed.value = 0
  if (!chosen) {
    file.value = null
    return
  }
  if (!/\.pdf$/i.test(chosen.name)) {
    fatal.value = 'That file is not a .pdf.'
    file.value = null
    return
  }
  // Checked against the module's own cap rather than a copy of it, so the browser never
  // reads a file the engine was going to reject anyway.
  const cap = parser.value?.maxBytes ?? MAX_UPLOAD_BYTES
  if (chosen.size > cap) {
    fatal.value = `That file is ${megabytes(chosen.size)}. The limit is ${megabytes(cap)}.`
    file.value = null
    return
  }
  file.value = chosen
}

function onDrop(event: DragEvent) {
  dragOver.value = false
  pick(event.dataTransfer?.files?.[0])
}

/**
 * A parse blocks this thread for its whole duration, so a "Stop" button could not do
 * anything except abandon the result. The button is therefore omitted and the cost is
 * stated instead, rather than offering a control that does not work.
 */
async function start() {
  if (!file.value || busy.value) return
  fatal.value = null
  summary.value = null
  documentJson.value = null

  try {
    phase.value = 'loading-module'
    startTimer()

    if (!parser.value) {
      parser.value = await loadParser(new URL('./legalpdf.wasm', import.meta.url))
    }

    phase.value = 'reading-file'
    const bytes = new Uint8Array(await file.value.arrayBuffer())

    const json = await parser.value.parse(bytes, (next) => {
      phase.value = next
    })
    phase.value = 'done'
    documentJson.value = json
    summary.value = summarise(JSON.parse(json))
  } catch (error) {
    phase.value = 'error'
    fatal.value = error instanceof Error ? error.message : String(error)
    // A module that never loaded leaves nothing to retry with, so it is worth saying so
    // separately from a document that would not parse.
    if (!parser.value) {
      moduleError.value = fatal.value
    }
  } finally {
    stopTimer()
    elapsed.value = performance.now() - startedAt.value
  }
}

/**
 * The download is built from the text already in memory. There is no result endpoint to
 * navigate to, and re-reading the PDF to re-parse it would be the only other option.
 */
function download() {
  if (!documentJson.value || !file.value) return
  // The stem is attacker-controlled file metadata, so it is reduced to a safe
  // character set before it reaches a filename.
  const stem =
    file.value.name
      .replace(/\.pdf$/i, '')
      .replace(/[^A-Za-z0-9 ._-]+/g, ' ')
      .trim()
      .slice(0, 80) || 'document'
  const blob = new Blob([documentJson.value], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = `${stem}.akn.json`
  anchor.click()
  URL.revokeObjectURL(url)
}

function reset() {
  stopTimer()
  file.value = null
  summary.value = null
  documentJson.value = null
  fatal.value = null
  moduleError.value = null
  phase.value = 'idle'
  elapsed.value = 0
  if (input.value) input.value.value = ''
}

onBeforeUnmount(stopTimer)
</script>

<template>
  <div class="wrap">
    <!-- The wordmark is inert: it is not a link home, because this is the home of this
         one tool. Only "Utharam" in the tag and in the footer goes to the landing page. -->
    <header class="masthead">
      <div class="brand">
        <span class="brand-mark" aria-hidden="true">{{ MARK }}</span>
        <span class="brand-name">{{ BRAND }}<span>.</span></span>
      </div>
      <a class="masthead-tag" :href="SUITE_URL" rel="noopener">
        Part of {{ SUITE }}
      </a>
    </header>

    <section class="hero">
      <div class="hero-badge">
        <span class="pulse-dot" aria-hidden="true"></span>
        <span>AKN Structure Extractor &middot; Indian Bare Acts &amp; Rules</span>
      </div>
      <h1>Legal PDFs into structured JSON.</h1>
      <p class="lead">
        Drop a text-based PDF of an Indian Act or Rule and get Akoma Ntoso JSON back —
        sections, subsections, clauses, citations and footnotes, each with a stable
        identifier and a resolved cross-reference. The same engine that backs the
        command-line tool, running as WebAssembly.
      </p>

      <div class="claims">
        <span class="claim">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
          </svg>
          Cloud transmit: <strong>0 bytes</strong>
        </span>
        <span class="claim">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <rect width="18" height="11" x="3" y="11" rx="2" ry="2" />
            <path d="M7 11V7a5 5 0 0 1 10 0v4" />
          </svg>
          Closed tab clears <strong>everything</strong>
        </span>
        <span class="claim">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" />
          </svg>
          Engine: <strong>WebAssembly</strong>
        </span>
        <span class="claim">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <path d="M4 7V4h16v3M9 20h6M12 4v16" />
          </svg>
          Output: <strong>akn.statute.v1</strong>
        </span>
      </div>
    </section>

    <div class="card">
      <div
        class="drop"
        :class="{ over: dragOver, busy }"
        @click="input?.click()"
        @dragover.prevent="dragOver = true"
        @dragleave="dragOver = false"
        @drop.prevent="onDrop"
      >
        <strong>{{ file ? file.name : 'Drop a PDF here' }}</strong>
        <span v-if="file">
          {{ megabytes(file.size) }} &middot; click to choose a different file
        </span>
        <span v-else>or click to browse &middot; up to 100 MB &middot; digital-born only</span>
      </div>
      <input
        ref="input"
        type="file"
        accept="application/pdf,.pdf"
        hidden
        @change="pick(($event.target as HTMLInputElement).files?.[0])"
      />

      <div v-if="fatal" class="note bad" style="margin-top: 16px">{{ fatal }}</div>
      <div v-if="moduleError && !parser" class="note bad" style="margin-top: 16px">
        The parser module could not be loaded, so nothing can be parsed. Rebuild it with
        <code>npm run wasm</code> and redeploy.
      </div>

      <div v-if="busy" :class="barClass">
        <i></i>
      </div>
      <div v-if="phase !== 'idle'" class="status">
        <span><b>{{ phaseLabel }}</b></span>
        <span class="elapsed">{{ human(elapsed) }}</span>
      </div>
      <div v-if="busy" class="note info" style="margin-top: 12px">
        Parsing blocks this tab until it finishes, so the page will not respond while the
        bar is moving. A large Act can take a couple of minutes.
      </div>

      <div class="actions">
        <button class="primary" :disabled="!file || busy" @click="start">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true">
            <path d="M5 12h14M13 6l6 6-6 6" />
          </svg>
          Extract structure
        </button>
        <button v-if="file || summary" @click="reset">Clear</button>
      </div>
    </div>

    <div v-if="summary" class="card">
      <div class="result-meta">
        <span class="result-title">{{ summary.title || file?.name }}</span>
        <span class="schema-tag">{{ summary.schemaVersion }}</span>
      </div>

      <div class="stats">
        <div class="stat"><b>{{ summary.sections }}</b><span>sections</span></div>
        <div class="stat"><b>{{ summary.provisions }}</b><span>provisions</span></div>
        <div class="stat"><b>{{ summary.citations }}</b><span>citations</span></div>
        <div class="stat"><b>{{ summary.resolved }}</b><span>resolved</span></div>
        <div class="stat"><b>{{ summary.unresolvedInternal }}</b><span>unresolved</span></div>
        <div class="stat"><b>{{ summary.external }}</b><span>external</span></div>
        <div class="stat"><b>{{ summary.footnotes }}</b><span>footnotes</span></div>
        <div class="stat"><b>{{ summary.diagnostics }}</b><span>diagnostics</span></div>
      </div>

      <div v-if="kindRows.length" class="kinds">
        <div class="kinds-head">Structure found</div>
        <div v-for="row in kindRows" :key="row.kind" class="kind-row">
          <span class="kind-name">{{ row.label }}</span>
          <span class="kind-track">
            <i :style="{ width: `${Math.max(2, (row.count / kindMax) * 100)}%` }"></i>
          </span>
          <span class="kind-count">{{ row.count.toLocaleString() }}</span>
        </div>
      </div>

      <div v-if="Object.keys(summary.byRelation).length" class="relations">
        <span v-for="(count, key) in summary.byRelation" :key="key" class="pill">
          {{ RELATION_LABELS[key] || key }}<strong>{{ count.toLocaleString() }}</strong>
        </span>
      </div>

      <div v-if="summary.warning" class="note warn">{{ summary.warning }}</div>
      <div v-else-if="summary.note" class="note info">{{ summary.note }}</div>

      <div class="actions">
        <button class="primary" @click="download">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true">
            <path d="M12 5v14M7 12l5 5 5-5" />
          </svg>
          Download JSON
        </button>
      </div>
    </div>

    <!--
      Companion tool. Present as a placeholder because the reader is being built: the
      point of the card is that this tool has an obvious second half, and that whoever
      lands here knows what to do with the file they just downloaded.
    -->
    <div class="card companion">
      <div class="companion-head">
        <span class="companion-badge">Companion</span>
        <span class="companion-name">{{ COMPANION.name }}</span>
      </div>
      <p class="companion-summary">{{ COMPANION.summary }}</p>
      <div class="actions" style="margin-top: 14px">
        <a class="btn-secondary" :href="COMPANION.href" rel="noopener">
          {{ COMPANION.href.replace(/^https?:\/\//, '') }}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true">
            <path d="M7 17 17 7" />
            <path d="M7 7h10v10" />
          </svg>
        </a>
      </div>
    </div>

    <footer class="site-footer">
      <span>
        Digital-born PDFs only. Scanned documents have no text layer and will come back
        empty. Nothing is uploaded.
      </span>
      <span class="footer-meta">
        A <a :href="SUITE_URL">{{ SUITE }}</a> tool
        <template v-if="parser"> &middot; engine {{ parser.version }}</template>
      </span>
    </footer>
  </div>
</template>