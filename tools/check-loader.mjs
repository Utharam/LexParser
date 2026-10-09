// Verify the shipped loader, not just the ABI.
//
// `tools/wasm-smoke.mjs` exercises the boundary by hand, which proves the module is
// callable. This proves the file the browser actually loads — `web/src/wasm.ts` — does
// the same thing, because that is where the import table, the `memory.buffer`
// re-reads and the offset handling live.
//
// Run from the repository root:
//
//   node tools/check-loader.mjs path/to.pdf
//
// `web/src/wasm.ts` is bundled with esbuild (already present as a Vite dependency) and
// run under Node, which has the same WebAssembly and Web Streams APIs a browser has.

import { readFileSync, readdirSync, existsSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { argv, exit } from 'node:process'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const PDF = argv[2] ?? 'D:/Projects/Legal Parser/_temp/it.pdf'
const DIST = join(ROOT, 'web/dist/assets')

if (!existsSync(PDF)) {
  console.error(`no PDF at ${PDF}`)
  exit(1)
}

const { build } = await import(pathToFileURL(join(ROOT, 'web/node_modules/esbuild/lib/main.js')))

// Bundle the real loader. `external` is left empty so the emitted module imports nothing
// and can run directly under Node.
const bundle = await build({
  entryPoints: [join(ROOT, 'web/src/wasm.ts')],
  bundle: true,
  format: 'esm',
  write: false,
  target: 'es2022',
  logLevel: 'error',
})
const loaderUrl = 'data:text/javascript;base64,' + Buffer.from(bundle.outputFiles[0].text).toString('base64')
const { loadParser } = await import(loaderUrl)

// Find the emitted `.wasm` the way the app does. `wasm.ts` resolves it from
// `import.meta.url`, so it lands in `dist/assets/` under a content hash; read the
// directory rather than hard-coding one.
const emitted = readdirSync(DIST).filter((name) => name.endsWith('.wasm'))
if (emitted.length === 0) {
  console.error(`no .wasm in ${DIST}. Run \`npm run build:web\` in web/ first.`)
  exit(1)
}
const wasm = readFileSync(join(DIST, emitted[0]))

const failures = []
const check = (label, condition, detail = '') => {
  console.log(`${condition ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`)
  if (!condition) failures.push(label)
}

// `loadParser` fetches, and Node's `fetch` needs a real URL, so the emitted asset is
// served through a `data:` URL rather than the filesystem.
const wasmUrl = `data:application/wasm;base64,${wasm.toString('base64')}`
const started = Date.now()
const parser = await loadParser(wasmUrl)
check('loadParser resolves', true, `engine ${parser.version}, cap ${parser.maxBytes}`)
check('reports the engine cap', parser.maxBytes === 100 * 1024 * 1024, `${parser.maxBytes} bytes`)

// Every phase the engine can emit must arrive, or the progress bar would stall on a
// phase the UI has no label for.
const phases = []
const pdf = new Uint8Array(readFileSync(PDF))
const json = await parser.parse(pdf, (phase) => phases.push(phase))
check('parse resolves', typeof json === 'string' && json.length > 0, `${(json.length / 1e6).toFixed(1)} MB`)
check(
  'phase callback fired',
  phases.includes('extracting') && phases.includes('parsing') && phases.includes('serialising'),
  phases.join(' -> '),
)

const document = JSON.parse(json)
check('schema is akn.statute.v1', document.schema_version === 'akn.statute.v1', document.schema_version)
check('provisions were extracted', (document.provisions?.length ?? 0) > 0, `${document.provisions?.length} provisions`)

// A rejection must not poison the instance: the next document has to parse.
// An empty buffer is refused before it reaches the engine — `legalpdf_alloc(0)` returns
// the null offset, which the loader reports. Checked separately from a bad PDF so the
// two failure paths stay distinguishable.
let rejected = null
try {
  await parser.parse(new Uint8Array(0))
} catch (error) {
  rejected = error
}
check(
  'empty input is rejected with a message',
  rejected instanceof Error && rejected.message.length > 0,
  rejected?.message,
)

// And a real PDF that is not one: the engine's own message has to survive the crossing.
let refused = null
try {
  await parser.parse(new TextEncoder().encode('%PDF-1.7\nthis is not a document'))
} catch (error) {
  refused = error
}
check(
  'a malformed PDF is rejected with the engine message',
  refused instanceof Error && /PDF|invalid|structure|header/i.test(refused.message),
  refused?.message,
)

const second = await parser.parse(pdf)
check('the module still works after a failure', second.length > 0, `${(second.length / 1e6).toFixed(1)} MB`)

// Offset discipline: the payload must survive the parse without corruption, and a second
// parse must produce byte-identical output.
check('output is stable across parses', second === json, 'deterministic')

console.log(`\nchecked in ${((Date.now() - started) / 1000).toFixed(1)}s`)
exit(failures.length === 0 ? 0 : 1)