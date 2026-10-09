import type { ParsePhase } from './types'

/**
 * The loader for `legal-pdf-wasm`.
 *
 * The module is a plain `cdylib` with a hand-written C ABI (see
 * `legal-pdf-wasm/src/lib.rs`), so this file is the entire host side of the boundary.
 * Every value crossing it is a `u32`, and every offset is an index into the module's
 * linear memory — which gives one rule that governs the whole file:
 *
 *   **Re-read `memory.buffer` after every call that can allocate.**
 *
 * Growing a WebAssembly memory replaces its `ArrayBuffer`, detaching every typed-array
 * view onto the old one. A view cached across an allocating call silently reads memory
 * the module no longer owns. `decode` and `parseWith` below re-read it every time for
 * that reason rather than keeping a cached view.
 */

/** The engine's phase codes, matching `PHASE_*` in `legal-pdf-wasm/src/lib.rs`. */
const PHASE_EXTRACTING = 1
const PHASE_PARSING = 2
const PHASE_SERIALIZING = 3

const PHASE_BY_CODE: Record<number, ParsePhase> = {
  [PHASE_EXTRACTING]: 'extracting',
  [PHASE_PARSING]: 'parsing',
  [PHASE_SERIALIZING]: 'serialising',
}

/** The import module the bridge declares its phase callback under. */
const PHASE_MODULE = './legalpdf.js'

/**
 * The subset of the module this file uses. Declared rather than cast straight from
 * `WebAssembly.Exports`, so a rename on the Rust side is a type error here rather than
 * `undefined` at runtime.
 */
interface LegalPdfModule {
  readonly memory: WebAssembly.Memory
  legalpdf_max_pdf_bytes(): number
  legalpdf_engine_version(): number
  legalpdf_engine_version_len(): number
  legalpdf_alloc(length: number): number
  legalpdf_free_input(offset: number, length: number): number
  legalpdf_parse(offset: number, length: number): number
  legalpdf_result_ok(handle: number): number
  legalpdf_result_len(handle: number): number
  legalpdf_result_read(handle: number): number
  legalpdf_result_free(handle: number): number
}

type ImportImplementation = (module: LegalPdfModule, ...args: number[]) => unknown

/**
 * Implementations for the symbols `wasm-bindgen` and `js-sys` require.
 *
 * `legal-citations` depends on `diff-match-patch-rs`, which pulls in `chrono` with its
 * default features; on `wasm32` that enables `chrono`'s `wasmbind` feature, which is
 * what drags `wasm-bindgen` into the module. Cargo unions features across the whole
 * graph, so no crate that does not own the dependency can switch it off — the host has
 * to satisfy these imports, and this table is where that happens.
 *
 * None of them run during a statutory parse: `chrono` is reached only if a citation
 * string is date-formatted, and `getrandom` only if something wants entropy. They are
 * implemented properly rather than stubbed so that a real call would work instead of
 * failing as a missing function.
 */
const runtime: Record<string, ImportImplementation> = {
  // This module holds no JavaScript references, so reference counting is a no-op.
  __wbindgen_object_drop_ref: () => undefined,
  __wbindgen_describe: () => 0,
  // `Date.now()`.
  __wbg_getTime: () => Date.now(),
  // `new Date(millis)`. `chrono` only reads it back through `getTime`, so the value
  // itself never has to be representable — the millisecond count is enough.
  __wbg_new_0: (millis) => Number(millis),
  // `crypto.getRandomValues(view)` over a one-byte view at `offset`.
  __wbg_getRandomValues: (module, offset) => {
    globalThis.crypto.getRandomValues(new Uint8Array(module.memory.buffer, offset, 1))
    return offset
  },
  // wasm-bindgen's panic path. It cannot be caught on this side, so it is turned into a
  // real throw to keep the failure legible.
  __wbg___wbindgen_throw: (module, pointer, length) => {
    throw new Error(`The parser module failed: ${decode(module, pointer, length)}`)
  },
  __wbindgen_externref_table_set_null: () => undefined,
  __wbindgen_externref_table_grow: () => 0,
}

/**
 * `wasm-bindgen` appends a hash to every generated name, and that hash changes when the
 * dependency is rebuilt. Matching on the stable prefix means a rebuild does not break
 * the loader, and an unrecognised symbol is named in the error rather than left
 * undefined — a missing import otherwise surfaces as an opaque `LinkError`.
 */
function stableName(name: string): string {
  return name.replace(/_[0-9a-f]{16}$/, '')
}

function decode(module: LegalPdfModule, offset: number, length: number): string {
  const text = new TextDecoder().decode(
    new Uint8Array(module.memory.buffer, offset, length),
  )
  // Payloads carry a trailing NUL that `length` excludes. Stripping any that is present
  // means an off-by-one cannot reach the UI as an invisible character.
  return text.replace(/\0+$/, '')
}

export interface LoadedParser {
  /** The engine's own size cap. The UI refuses anything larger before reading it. */
  readonly maxBytes: number
  /** Version of the engine that produced the output, for display. */
  readonly version: string
  /**
   * Parse a PDF and return the serialized `akn.statute.v1` JSON.
   *
   * Rejects with a human-readable message when the document cannot be parsed. The module
   * stays usable either way: the bridge catches panics internally, so one bad document
   * cannot poison the instance for the next one.
   */
  parse(bytes: Uint8Array, onPhase?: (phase: ParsePhase) => void): Promise<string>
}

/**
 * Compile and instantiate the module.
 *
 * `wasmUrl` must resolve to a `.wasm` served with the correct MIME type.
 * `new URL('./legalpdf.wasm', import.meta.url)` makes Vite emit it as a hashed asset,
 * which keeps `base: './'` working from a subpath.
 */
export async function loadParser(wasmUrl: string | URL): Promise<LoadedParser> {
  // `compileStreaming` avoids holding the whole module in JavaScript memory as well as
  // in the engine's, which matters for a module this size.
  const source = WebAssembly.compileStreaming
    ? await WebAssembly.compileStreaming(fetch(wasmUrl))
    : await WebAssembly.compile(await (await fetch(wasmUrl)).arrayBuffer())

  const module = source as unknown as LegalPdfModule

  // The phase callback is wired through the import table, so it is installed once here
  // rather than per parse. `listener` is read when the callback fires, which happens
  // inside `parseWith` and nowhere else, so one module reference is enough.
  let listener: ((phase: ParsePhase) => void) | null = null
  const imports = buildImports(module, (phase) => {
    const name = PHASE_BY_CODE[phase]
    if (name !== undefined) {
      listener?.(name)
    }
  })

  const instance = await WebAssembly.instantiate(source, imports)
  const instanceExports = instance.exports as unknown as LegalPdfModule

  return {
    maxBytes: instanceExports.legalpdf_max_pdf_bytes(),
    version: decode(
      instanceExports,
      instanceExports.legalpdf_engine_version(),
      instanceExports.legalpdf_engine_version_len(),
    ),
    // `async` rather than a bare arrow returning `parseWith`'s string: the caller
    // already awaits this, and a plain return would make the function's declared
    // `Promise<string>` a lie that TypeScript would rightly reject.
    parse: async (bytes, onPhase) => {
      listener = onPhase ?? null
      try {
        return parseWith(instanceExports, bytes)
      } finally {
        // Dropped synchronously in the same turn as the parse, so a phase callback can
        // never fire after the caller has moved on.
        listener = null
      }
    },
  }
}

/**
 * Build the import object from the module's own import list, so an unexpected symbol is
 * named in the error instead of surfacing as a `LinkError`.
 */
function buildImports(
  module: LegalPdfModule,
  onPhase: (phase: number) => void,
): WebAssembly.Imports {
  const imports: WebAssembly.Imports = {}
  const unknown: string[] = []
  for (const entry of WebAssembly.Module.imports(module as unknown as WebAssembly.Module)) {
    imports[entry.module] ??= {}
    if (entry.module === PHASE_MODULE && entry.name === '__legalpdf_phase') {
      imports[entry.module][entry.name] = onPhase
      continue
    }
    const implementation = runtime[stableName(entry.name)]
    if (!implementation) {
      unknown.push(`${entry.module}.${entry.name}`)
      continue
    }
    imports[entry.module][entry.name] = implementation
  }
  if (unknown.length > 0) {
    throw new Error(
      `The parser module needs an import this page does not provide: ${unknown.join(', ')}`,
    )
  }
  return imports
}

/**
 * The whole parse, expressed the way `tools/wasm-smoke.mjs` expresses it so the two
 * cannot drift.
 *
 * Synchronous by necessity: WebAssembly has no threads here, so the call blocks this
 * thread for as long as the parse takes. The promise exists to give the caller a uniform
 * interface with `File.arrayBuffer()`, not to make the work asynchronous.
 */
function parseWith(module: LegalPdfModule, bytes: Uint8Array): string {
  const offset = module.legalpdf_alloc(bytes.length)
  if (offset === 0) {
    throw new Error('The browser could not reserve memory for that file.')
  }
  try {
    // Re-read after `alloc`: reserving the buffer may have grown the module.
    new Uint8Array(module.memory.buffer, offset, bytes.length).set(bytes)

    const handle = module.legalpdf_parse(offset, bytes.length)
    const ok = module.legalpdf_result_ok(handle)
    const length = module.legalpdf_result_len(handle)
    const payload = decode(module, module.legalpdf_result_read(handle), length)
    module.legalpdf_result_free(handle)

    if (!ok) {
      throw new Error(payload)
    }
    return payload
  } finally {
    // The engine has finished reading by here, so the input goes back either way.
    module.legalpdf_free_input(offset, bytes.length)
  }
}