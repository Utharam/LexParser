// Smoke test for the browser WebAssembly bridge, run outside the browser.
//
// The point is to exercise the exact ABI that `web/src/wasm.ts` uses against a real
// statute PDF, so a boundary mistake shows up here instead of in a tab. Run from the
// repository root:
//
//   node tools/wasm-smoke.mjs [path/to.pdf]
//
// This deliberately does *not* go through Vite. It loads the `.wasm` from disk and
// builds the import object by hand, so it tests the module rather than the bundler.

import { readFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { argv, exit } from 'node:process'
import { performance } from 'node:perf_hooks'

const WASM = 'target/wasm32-unknown-unknown/release/legal_pdf_wasm.wasm'
const PDF = argv[2] ?? 'D:/Projects/Legal Parser/_temp/it.pdf'

const bytes = readFileSync(WASM)
const module = await WebAssembly.compile(bytes)

console.log(`module: ${(bytes.length / 1e6).toFixed(1)} MB`)

/**
 * Handles for the `js_sys::Date` values that `chrono` creates through
 * `__wbg_new_0`. `wasm-bindgen` normally maintains a table for this; here a `Map` does
 * the same job, because `chrono` only ever passes a `Date` straight back to
 * `getTime`.
 */
const dates = new Map()
let nextDateHandle = 1

/**
 * Implementations for the imports `wasm-bindgen` and `js-sys` require.
 *
 * `legal-citations` depends on `diff-match-patch-rs`, which depends on `chrono` with
 * its default features, and `chrono`'s `wasmbind` feature is what drags in
 * `wasm-bindgen` for `wasm32`. Those features are unioned across the graph, so no
 * crate that does not own the dependency can turn them off — which means the browser
 * loader has to satisfy them too, and this file is where that contract is exercised.
 *
 * None of these run during a statutory parse. `chrono` is reached only if a citation
 * string is date-formatted, and `getrandom` only if something needs entropy. They are
 * implemented properly rather than stubbed so that this test would catch a real call.
 */
const runtime = {
  throwWasm: (pointer, length) => {
    const message = new TextDecoder().decode(
      new Uint8Array(exports.memory.buffer, pointer, length),
    )
    throw new Error(`wasm-bindgen throw: ${message}`)
  },
  now: () => Date.now(),
  getRandomValues: (pointer) => {
    globalThis.crypto.getRandomValues(
      new Uint8Array(exports.memory.buffer, pointer, 1),
    )
    return pointer
  },
  newDate: (millis) => {
    const handle = nextDateHandle++
    dates.set(handle, new Date(Number(millis)))
    return handle
  },
  noop: () => {},
}

/**
 * Wire each imported symbol to an implementation.
 *
 * `wasm-bindgen` appends a hash to every generated name, and the hash changes whenever
 * the dependency is rebuilt, so the mapping keys on the stable prefix and an unknown
 * symbol fails loudly instead of being missed. `web/src/wasm.ts` carries the same
 * table for the same reason.
 */
const implementations = {
  __legalpdf_phase: null, // filled in below, it needs `lastPhase`
  __wbindgen_object_drop_ref: runtime.noop,
  __wbindgen_describe: runtime.noop,
  __wbg___wbindgen_throw: runtime.throwWasm,
  __wbg_getTime: runtime.now,
  __wbg_getRandomValues: runtime.getRandomValues,
  __wbg_new_0: runtime.newDate,
  __wbindgen_externref_table_set_null: runtime.noop,
  __wbindgen_externref_table_grow: runtime.noop,
}

/** Strip the trailing `_<hash>` that `wasm-bindgen` adds to a generated name. */
const stableName = (name) => name.replace(/_[0-9a-f]{16}$/, '')

let lastPhase = 0
const imports = {}
const missing = []
for (const { module: moduleName, name } of WebAssembly.Module.imports(module)) {
  imports[moduleName] ??= {}
  if (name === '__legalpdf_phase') {
    imports[moduleName][name] = (phase) => {
      if (phase !== 0 && phase !== lastPhase) {
        lastPhase = phase
        console.log(`phase ${phase}`)
      }
    }
    continue
  }
  const implementation = implementations[stableName(name)]
  if (!implementation) {
    missing.push(`${moduleName}.${name}`)
    continue
  }
  imports[moduleName][name] = implementation
}
if (missing.length > 0) {
  console.error(`no implementation for: ${missing.join(', ')}`)
  exit(1)
}

const { exports } = new WebAssembly.Instance(module, imports)

/** Linear memory, re-read every time: growing the module detaches the old buffer. */
const memory = () => new Uint8Array(exports.memory.buffer)

function readString(offset, length) {
  return new TextDecoder().decode(
    new Uint8Array(exports.memory.buffer, offset, length),
  ).replace(/\0+$/, '')
}

console.log(`engine: ${readString(exports.legalpdf_engine_version(), exports.legalpdf_engine_version_len())}`)
console.log(`cap:    ${(exports.legalpdf_max_pdf_bytes() / 1048576).toFixed(0)} MB`)

const pdf = readFileSync(PDF)
console.log(`source: ${PDF} (${(pdf.length / 1e6).toFixed(1)} MB)`)

const started = performance.now()
const input = exports.legalpdf_alloc(pdf.length)
if (input === 0) throw new Error('legalpdf_alloc refused the input')
memory().set(pdf, input)

const handle = exports.legalpdf_parse(input, pdf.length)
exports.legalpdf_free_input(input, pdf.length)

const ok = exports.legalpdf_result_ok(handle)
const length = exports.legalpdf_result_len(handle)
const payload = readString(exports.legalpdf_result_read(handle), length)
exports.legalpdf_result_free(handle)

console.log(
  `parsed in ${((performance.now() - started) / 1000).toFixed(1)}s, ${ok ? 'ok' : 'FAILED'}`,
)

if (!ok) {
  console.error(payload)
  exit(1)
}

const document = JSON.parse(payload)
const kinds = {}
const walk = (node) => {
  kinds[node.kind] = (kinds[node.kind] ?? 0) + 1
  for (const child of node.children ?? []) walk(child)
}
for (const provision of document.akomaNtoso?.act?.body ?? []) walk(provision)

console.log(`schema:     ${document.schema_version}`)
console.log(`title:      ${document.akomaNtoso?.act?.meta?.title}`)
console.log(`provisions: ${document.provisions?.length ?? 0}`)
console.log(`citations:  ${document.citations?.length ?? 0}`)
console.log(`footnotes:  ${document.footnotes?.length ?? 0}`)
console.log(`diagnostics: ${document.diagnostics?.length ?? 0}`)
console.log(`kinds:      ${JSON.stringify(kinds)}`)
console.log(`sha256:     ${createHash('sha256').update(payload).digest('hex')}`)