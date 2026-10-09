# LexParser — engine and browser build

The `indian-statute` PDF parser, compiled to WebAssembly and served as a static site.
Drop a text-based PDF in the page and get `akn.statute.v1` JSON back. The parse happens
in the browser tab; nothing is uploaded and there is no server.

Derived from `D:\Projects\Legal Parser` at commit `7911bb1`, which contained a Node
server that spawned `legalpdf.exe`. That design cannot be deployed to static hosting —
Pages Functions have no `child_process` and cannot load a native executable — so the
engine was retargeted at `wasm32-unknown-unknown` instead.

**This README is the engineering document.** For what the product is and how to use it,
see [`web/README.md`](web/README.md).

## Building

```sh
rustup target add wasm32-unknown-unknown     # once
cd web && npm install
npm run build                                # builds the .wasm, typechecks, then bundles
```

| Script | What it does |
| --- | --- |
| `npm run dev` | Builds the module, then serves with HMR |
| `npm run wasm` | Builds `legal-pdf-wasm` and copies it to `web/src/` |
| `npm run build` | `wasm`, then `vue-tsc`, then `vite build` |
| `npm run build:web` | Skips the Rust build; what Cloudflare runs |
| `npm run deploy` | `build:web`, then `wrangler pages deploy` |
| `npm run typecheck` | `vue-tsc --noEmit` |
| `npm run smoke` | Runs the module against a real PDF outside the browser |

The `.wasm` is 21 MB, about 5.2 MB over the wire. It is a full engine with the PDF
layout model, the statutory grammar and the citation tables, not a trimmed build; see
*Layout* below for what would have to go to shrink it.

## Deploying

`web/dist` is the whole site — no server, no Pages Functions.

### Cloudflare Pages, Git-connected

Set these under **Settings → Builds & deployments**:

| Field | Value |
| --- | --- |
| Build command | `npm run build` |
| Deploy command | *leave empty* |
| Root directory | `/` |

Leave **Deploy command empty**. Pages then deploys the build output itself, using
`pages_build_output_dir` from `wrangler.toml` to locate it, and no API token is
involved. Filling that field in hands deployment to `wrangler pages deploy`, which
needs a token carrying *Cloudflare Pages: Edit* — see *Two deployment mistakes that look
like this one* below.

The root `package.json` declares an npm workspace covering `web/`. That is load-bearing:
Pages runs `npm clean-install` at the root, and a root manifest with no dependencies
installs nothing. `web/node_modules` is never created and the build fails with
`sh: 1: vue-tsc: not found`. There is one lockfile, at the root.

`npm run build` does **not** rebuild the wasm. Cloudflare's Pages image has no Rust
toolchain and no `wasm32-unknown-unknown` target, and a from-scratch cargo release build
of this engine does not fit the Pages build-time limit. That is why
`web/src/legalpdf.wasm` is committed rather than gitignored — the deploy compiles only
the TypeScript and ships the module the Rust source currently produces.

Rebuild it with `npm run build:wasm` and commit the result whenever the engine changes.
`cargo quick` will not notice a stale one.

### Two deployment mistakes that look like this one

Both produce a build that looks fine and a deploy that fails, and the error text points
somewhere unhelpful.

**A wrong output directory looks like a MIME error.** Pages falls back to the repository
root when it has no build to serve, which serves `web/index.html` verbatim. That file's
script tag points at `./src/main.ts`, and Pages returns `.ts` with `video/mp2t` — the
MPEG transport-stream mapping, since the extension is ambiguous between TypeScript and
video:

```
main.ts:1 Failed to load module script: Expected a JavaScript-or-Wasm module script
but the server responded with a MIME type of "video/mp2t".
```

**A wrong deploy command looks like an authentication error.** `npx wrangler deploy` is
the *Workers* command and fails with `Missing entry-point to Worker script or to assets
directory`. The Pages equivalent is `npx wrangler pages deploy web/dist`, and if that
fails with `Authentication error [code: 10000]`, the API token in the Pages environment
lacks *Cloudflare Pages: Edit*. Note that a Super Administrator *account role* does not
help: the API checks the token's own scopes, not the account membership. Fixing the token
works, but leaving the field empty is simpler — then Pages deploys without a token at all.

### From the command line

```sh
npm run deploy
```

Builds, then runs `wrangler pages deploy dist`. Needs `wrangler login` on that machine,
and bypasses the Git integration.

## How it is wired

```
legal-pdf-wasm/          cdylib, the whole host boundary
  src/lib.rs             9 exported functions, all-u32, plus 11 unit tests
web/src/wasm.ts          the JavaScript side of that boundary
web/src/summarise.ts     ported from the server's summarise.mjs
web/src/App.vue          the UI
tools/copy-wasm.mjs      cargo output -> web/src/legalpdf.wasm
tools/wasm-smoke.mjs     runs the module under Node against a real PDF
tools/check-loader.mjs   bundles web/src/wasm.ts and exercises it as shipped
```

### The boundary is a hand-written C ABI, not wasm-bindgen

`legal-pdf-wasm` exports plain `extern "C"` functions and trades only in `u32`. JavaScript
owns the bytes in and the bytes out; nothing else crosses. The alternative —
`wasm-bindgen` plus `wasm-bindgen-cli` — would have added a build-time tool and a
generated glue file that owns the same buffer protocol anyway. The whole boundary is then
about 80 lines of readable TypeScript in one file.

`legal-pdf-wasm` calls `parse_indian_statute_pdf`, which is the same entry point the CLI
uses, so the browser and `legalpdf.exe --profile indian-statute` produce byte-identical
output. Verified: both produce SHA-256
`305d9580af0cfafdc8e3009c05cf7b74c38e0942848b88c82392b665d3e79845` for the Income-tax
Act 2025.

### The import table is not entirely ours

`legal-citations` depends on `diff-match-patch-rs`, which depends on `chrono` with its
default features. On `wasm32` that enables `chrono`'s `wasmbind` feature, which pulls
`wasm-bindgen` and `js-sys` in. Cargo unions features across the whole graph, so no crate
that does not own the dependency can switch it off — the host has to satisfy the imports
even though no statutory parse calls them.

Both `web/src/wasm.ts` and `tools/wasm-smoke.mjs` resolve those imports by matching the
stable name prefix, because `wasm-bindgen` appends a hash to every generated name and that
hash changes when the dependency is rebuilt. An unrecognised symbol is reported by name
rather than left undefined.

## What the old server did that this does not

| Server | Here |
| --- | --- |
| `XMLHttpRequest` POST to `/api/jobs` | `File.arrayBuffer()` |
| `EventSource` progress stream | In-process phase callback |
| `GET /api/jobs/:id/result` | The value returned from the module |
| `DELETE /api/jobs/:id` | Not possible; see below |
| 100 MB server-side cap | The module's own cap, checked in the page |
| Upload progress percentage | None; there is no upload |

**Cancellation is genuinely gone.** A parse is one synchronous call into WebAssembly, so
the tab is blocked for its duration and there is nothing to interrupt. The Stop button was
removed rather than left in as a control that cannot work, and the UI says so while the
bar is moving.

**Progress is a phase, not a percentage.** The engine reports extraction, parsing and
serialisation and nothing finer. The bar is indeterminate instead of showing a fabricated
percentage.

**A large Act blocks the tab.** The Income-tax Act 2025 (10 MB, 8,699 provisions) takes
about 124 s in the module against 113 s for the native binary. That is the browser
runtime overhead on a single-threaded module; the page will not respond until it finishes.

## Verifying

`cargo test -p legal-pdf-wasm` runs 11 tests on the host, including the exact
alloc/parse/read/free sequence `web/src/wasm.ts` performs. That host build routes memory
offsets through a side table, because a 64-bit pointer does not fit in the `u32` the
boundary speaks — so the tests exercise the same offsets the browser sees.

```sh
npm run smoke                      # the module, against a real statute PDF
node tools/check-loader.mjs f.pdf  # the shipped loader, against the built assets
```

`check-loader.mjs` bundles `web/src/wasm.ts` with esbuild and runs it under Node, which
checks the parts `wasm-smoke.mjs` cannot: the import table, the `memory.buffer` re-reads
after every allocating call, and the determinism of the output.

## Layout

```
Cargo.toml, Cargo.lock, build.rs   workspace (members = legal-pdf-*)
legal-pdf-core/                    page, line, span, bbox model
legal-pdf-extraction/              text extraction
legal-pdf-extraction-processor/    reading order, page assembly
legal-pdf-structure/               sectioning, citations, footnotes
  src/indian_statute/              the profile this UI uses
legal-pdf-language/, -pairing/, -support/
legal-pdf-ocr/                     C++ — excluded from the browser build
legal-pdf-wasm/                    the WebAssembly bridge
rust/                              CLI entry points (not used by the browser build)
web/                               Vue UI, builds to web/dist
tools/                             build copy and the two verification scripts
```

`legal-pdf-ocr` is present only because the workspace glob `legal-pdf-*` requires it to
resolve. It is not part of the browser build.

## Known parser defects carried over

Not regressions from the port; these are live bugs in `7911bb1`.

- Text order inside a clause can interleave with its sub-clauses, producing garbled
  parent text such as `the fund is–– agreement referred to in section 159(1)`.
- Running headers are still glued into body text (`... any entity; CERTAIN ACTIVITIES`).
- 81 orphan roots holding 20,774 characters remain outside any schedule.
- Duplicate eIds: 1,148 on the Income-tax Act 2025, 127 on the CGST Act.
- A `(b)` marker following `(a) ...; and` can be misread as a sub-clause of `(a)`.
- OCR is unavailable, so scanned PDFs come back empty. The UI says so, and reports a
  likely-scanned document rather than showing a successful empty parse.

## Verified on

- Income-tax Act 2025: 8,699 provisions, 539 sections, 28 chapters, 16 schedules.
- CGST Act updated to 16-08-2024: 1,913 provisions, 187 sections, 22 chapters,
  3 schedules, 17 schedule items.