// Copy the built WebAssembly module next to the sources that import it.
//
// `web/src/wasm.ts` loads `./legalpdf.wasm` via `new URL(..., import.meta.url)`, which
// Vite resolves and fingerprints at build time. The file therefore has to exist under
// `web/src/` for the dev server, and Vite copies it into `dist/assets/` for a deploy.
//
// Kept as a script rather than a `vite` plugin so the build has no extra dependency and
// the copy can also be run on its own after a Rust-only change.
//
// Run from the repository root, or via `npm run wasm` in `web/`.

import { copyFileSync, mkdirSync, statSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')

const SOURCE = join(ROOT, 'target/wasm32-unknown-unknown/release/legal_pdf_wasm.wasm')
const DESTINATION = join(ROOT, 'web/src/legalpdf.wasm')

let size
try {
  size = statSync(SOURCE).size
} catch {
  console.error(
    `missing ${SOURCE}\n` +
      'Build it first: cargo build --release --target wasm32-unknown-unknown -p legal-pdf-wasm',
  )
  process.exit(1)
}

mkdirSync(dirname(DESTINATION), { recursive: true })
copyFileSync(SOURCE, DESTINATION)

// A debug build is tens of megabytes larger and roughly an order of magnitude slower,
// which in a browser reads as a hang rather than as a slow parse. Worth being loud about.
if (size > 40 * 1024 * 1024) {
  console.warn(
    `warning: legalpdf.wasm is ${(size / 1e6).toFixed(0)} MB, which is larger than a ` +
      'release build should be. Check that `--release` reached the cargo command.',
  )
}

console.log(`legalpdf.wasm ${(size / 1e6).toFixed(1)} MB -> web/src/legalpdf.wasm`)