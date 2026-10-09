# LexParser

**Legal PDFs into structured JSON, in your browser.**

LexParser parses Indian Acts and Rules from PDF into
[Akoma Ntoso](https://www.akomantoso.org/) JSON — sections, subsections, clauses,
provisos, footnotes and cross-references, each with a stable identifier. The parse
runs entirely in the browser as WebAssembly. The PDF is never uploaded; closing the tab
discards everything.

A [Utharam](https://utharam.in/) tool. Free, no signup, no account.

Its companion, **StatuteGraph**, reads the JSON this produces — a provision tree and a
walkable citation graph. See [statutegraph.utharam.in](https://statutegraph.utharam.in/).

---

## What it does

Drop in a text-based PDF of a Bare Act or Rules and get back an `akn.statute.v1`
document containing:

| Output | What it is |
| --- | --- |
| `akomaNtoso.act.body` | The hierarchy: chapters, sections, subsections, clauses, sub-clauses, provisos, explanations |
| `provisions` | Every provision, flattened, for table lookup |
| `citations` | Every cross-reference, with its raw text, character span, target id and whether it resolved |
| `footnotes` | Footnotes and the amendments they carry |
| `diagnostics` | Non-fatal observations, in document order |

Each provision carries a stable `eid` such as `sec_16__subsec_2__cl_a`, so a citation
found in the prose can be joined to the provision it points at without string matching.

The output is **byte-identical** to the command-line tool:

```sh
legalpdf <file.pdf> --profile indian-statute -o out.json
```

Both produce SHA-256 `305d9580af0cfafdc8e3009c05cf7b74c38e0942848b88c82392b665d3e79845`
for the Income-tax Act 2025. That is the guarantee that matters if you are piping this
into anything downstream.

## Running it

```sh
rustup target add wasm32-unknown-unknown     # once
cd web && npm install
npm run dev                                    # http://localhost:5173
```

For a deployable build:

```sh
cd web && npm run build                        # emits web/dist
```

`web/dist` is a static site. No server, no Pages Functions, no build command needed on
Cloudflare's side if you build before pushing.

| Script | What it does |
| --- | --- |
| `npm run dev` | Build the module, then serve with HMR |
| `npm run wasm` | Build `legal-pdf-wasm` and copy it to `web/src/` |
| `npm run build` | `wasm`, then `vue-tsc`, then `vite build` |
| `npm run build:web` | Skip the Rust build when only the UI changed |
| `npm run typecheck` | `vue-tsc --noEmit` |

The first `npm run dev` spends a few minutes in Cargo. After that it is cached and
starts in about a second.

## Using the command line instead

The same engine runs as a normal binary, which is the better choice for batch work,
CI pipelines, or anything that needs to process many files.

```sh
cargo build --release --features pdf --bin legalpdf

./legalpdf income-tax-act-2025.pdf --profile indian-statute -o act.json
```

Other profiles and features exist behind flags — `language`, `ocr`, `kraken`,
`ppdoc-full` — but they are not part of the browser build. `ocr` and `fast-allocator`
need C and C++ and cannot compile for `wasm32-unknown-unknown`; that constraint is why
this build stays on the `pdf` feature alone.

## Requirements and limits

**Digital-born PDFs only.** A scanned document has no text layer, so the parser returns
an empty tree rather than an error. LexParser detects this and says so explicitly,
because a "successful" parse of a scan is the one failure that would otherwise pass
unnoticed. OCR is not available in this build.

**Size limit: 100 MB.** The page reads the cap from the module rather than hard-coding
it, so the browser refuses exactly what the engine would refuse.

**A parse blocks the tab.** It is one synchronous call into WebAssembly, with no worker
and no threads, so there is nothing to cancel and the page will not respond until it
finishes. The Income-tax Act 2025 — 10 MB, 8,699 provisions — takes about two minutes.

**21 MB download.** The `.wasm` is a full engine: the PDF layout model, the statutory
grammar and the citation tables. About 5.2 MB over the wire, gzipped.

## What the page is telling you

After a parse, the tiles are the things worth checking before trusting the output:

- **sections / provisions** — how much structure was found at all
- **citations, resolved / unresolved / external** — an internal citation that did not
  resolve points at a provision the document does not contain. That is often the first
  sign of a dropped page.
- **diagnostics** — extraction and structure-pass observations, forwarded from the
  pipeline rather than swallowed
- **Structure found** — the full breakdown by provision kind, so you can see the parts
  that *should* be zero and are not

## Known parser defects

These are live bugs in the upstream engine, not regressions from the browser port:

- Text order inside a clause can interleave with its sub-clauses, producing garbled
  parent text such as `the fund is–– agreement referred to in section 159(1)`.
- Running headers are still glued into body text (`... any entity; CERTAIN ACTIVITIES`).
- 81 orphan roots holding 20,774 characters remain outside any schedule.
- Duplicate `eid`s: 1,148 on the Income-tax Act 2025, 127 on the CGST Act.
- A `(b)` marker following `(a) ...; and` can be misread as a sub-clause of `(a)`.

## Verified on

- **Income-tax Act 2025** — 8,699 provisions, 539 sections, 28 chapters, 16 schedules
- **CGST Act updated to 16-08-2024** — 1,913 provisions, 187 sections, 22 chapters,
  3 schedules, 17 schedule items

## How it works

The engine is `legal-pdf-parser`, retargeted at `wasm32-unknown-unknown` rather than
reimplemented. The full architecture is in the [repository README](../README.md); the
short version is that `legal-pdf-wasm` is a `cdylib` exporting a hand-written C ABI of
nine `u32`-only functions, and `web/src/wasm.ts` is the whole host side of that boundary.

To verify a change to the bridge:

```sh
npm run smoke                                  # the module, against a real statute PDF
node tools/check-loader.mjs path/to/file.pdf   # the shipped loader, against built assets
cargo test -p legal-pdf-wasm                   # 11 tests, including the browser's access pattern
```

## Licence

MIT. See [LICENSE](../LICENSE).

Third-party bundled data in `data/` keeps its own provenance — see
[data/README.md](../data/README.md).