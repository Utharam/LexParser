# Legal PDF parser agent rules

- For ordinary Rust edits, run `cargo quick`. Do not run `cargo build`,
  `cargo run`, `cargo test`, or a corpus replay after each change.
- `cargo quick` now carries `--features pdf`. Every engine module is behind
  `#[cfg(feature = "pdf")]` and the root `default` is empty, so without it the check
  compiles almost nothing. Do not drop that flag.
- `cargo test` compiles `cfg(test)` code, which `cargo quick` never does. A missing
  fixture under `data/` breaks only the test build, so a green `cargo quick` is not
  evidence that tests compile.
- Batch source changes behind metadata checks. Link the executable or test
  harness once, only when a final behavioral gate actually requires it.
- Reuse the most recently linked binary for diagnostics that do not require new
  code. Never start another Cargo command while Cargo or rustc is still active
  for this repository.
- Format only touched Rust files with `rustfmt --edition 2021 --check <files>`;
  `cargo fmt --all` needlessly traverses the entire crate and currently
  overflows rustfmt's stack.
- Keep benchmark output bounded and disposable. Reuse one output directory,
  compare it to frozen hashes, and remove it immediately after recording the
  result.
- The browser module is built with `cargo wasm`, never with the host profile. It
  must stay on the `pdf` feature: `ocr` needs C++ and `fast-allocator` needs C,
  neither of which builds for `wasm32-unknown-unknown`.

## WebAssembly boundary

- `legal-pdf-wasm` exports a hand-written C ABI, not `wasm-bindgen`. Every value
  crossing it is a `u32`; do not introduce a struct, a tuple or a borrowed
  string. A panic is caught inside `legalpdf_parse` and reported as a result,
  never allowed to unwind into WebAssembly.
- Offsets index the module's linear memory. JavaScript must re-read
  `memory.buffer` after any call that can allocate, because growth detaches
  every existing view. Host builds route offsets through a side table so the
  unit tests exercise the offsets a browser would see.
- `wasm-bindgen` imports arrive through `chrono`, which `legal-citations` pulls
  in with its default features. Cargo unions those across the graph, so the host
  must satisfy them. Match them on the stable name prefix, never the hashed
  suffix, and report an unknown symbol by name.
- After changing the bridge, run both `node tools/wasm-smoke.mjs` and
  `node tools/check-loader.mjs <pdf>`. The first proves the module, the second
  proves `web/src/wasm.ts` as shipped. The parser must stay byte-identical to
  `legalpdf --profile indian-statute` for the same input.
- Keep any future repair or external-layout adapter provider-neutral: the
  engine validates bounded structural assignments and source identity, while
  the embedding application owns provider and runtime selection.

## Publishable data

- Use independently invented fixtures or document their public source. Do not copy
  genuine user queries, bug-report identifiers, histories or private documents
  into tests or evals without explicit permission. Synthetic replacements change
  identifying URLs and locators too.
- Automated prompts carry `machine_test` and a run ID; absent legacy origin remains
  `unknown`. Submission origin and fixture provenance are separate facts.
- Use relative paths or runtime input configuration and GitHub noreply attribution.
  Keep private inputs, auth and raw receipts in ignored local storage. Preserve
  third-party licenses and public-source attribution.
- Rust builders should remap source paths with `--remap-path-prefix`.
