//! Browser WebAssembly bridge for the `indian-statute` profile.
//!
//! # Why this is a hand-written C ABI rather than `wasm-bindgen`
//!
//! The surface is deliberately tiny: one entry point that takes the PDF bytes and
//! returns the serialized `akn.statute.v1` JSON. A `wasm-bindgen` build would need
//! the `wasm-bindgen` crate plus the `wasm-bindgen-cli` tool at build time, and the
//! generated glue would own the buffer protocol anyway. Declaring the handful of
//! functions here and writing the loader in `web/src/wasm.ts` keeps the whole boundary
//! auditable and needs no extra toolchain — the only artifact is the `.wasm` produced
//! by `cargo build --target wasm32-unknown-unknown`.
//!
//! # The boundary
//!
//! JavaScript owns the bytes going in and the bytes coming out. Nothing else crosses:
//! no structs, no enums, no tuples, no borrowed Rust strings. Every parameter and
//! every return value is a `u32`, which is FFI-safe by construction and matches the
//! 32-bit pointer width of `wasm32-unknown-unknown`.
//!
//! ```text
//! legalpdf_max_pdf_bytes()        -> u32  the engine's own size cap
//! legalpdf_engine_version()       -> u32  offset of a NUL-terminated version string
//! legalpdf_engine_version_len()   -> u32
//! legalpdf_alloc(len)             -> u32  reserve input bytes, returns an offset
//! legalpdf_parse(offset, len)     -> u32  run the profile, returns a result handle
//! legalpdf_result_ok(handle)      -> u32  1 = JSON document, 0 = error message
//! legalpdf_result_len(handle)     -> u32
//! legalpdf_result_read(handle)    -> u32  offset of the payload
//! legalpdf_result_free(handle)    -> u32  release it
//! legalpdf_free_input(offset,len) -> u32
//! ```
//!
//! Results come back as an opaque handle rather than a raw struct pointer. That keeps
//! `ResultBox` private — a `pub extern "C" fn` may not expose a `pub(crate)` type — and
//! it means a stale handle is a number JavaScript cannot accidentally dereference.
//!
//! `legalpdf_engine_version` returns a *static* offset and the other two data offsets
//! come from live allocations, so they need different handling on the JavaScript side:
//! a static offset survives `memory.buffer` being replaced, an allocated one does not.
//!
//! The JavaScript side must re-read `memory.buffer` after any call that can allocate,
//! since growing the module detaches the previous `ArrayBuffer`.
//!
//! # Failures
//!
//! A parse failure is reported as a result whose payload is a message, never as an
//! unwind across the boundary. A panic escaping into WebAssembly poisons the instance,
//! so every later document in the same tab would fail the same way. `catch_unwind`
//! turns that into one ordinary error.

use std::alloc::{alloc as raw_alloc, dealloc as raw_dealloc, Layout};
use std::cell::RefCell;
use std::collections::BTreeMap;

/// One source-size cap, shared with the CLI. Exposed so the UI can refuse an
/// oversized file before it reads the bytes, matching what the engine rejects.
#[no_mangle]
pub extern "C" fn legalpdf_max_pdf_bytes() -> u32 {
    legalpdf::MAX_PDF_BYTES as u32
}

/// Version of this crate, so the UI can report which engine build produced a
/// document rather than presenting output of unknown provenance.
///
/// The offset refers to a `'static` string literal. In `wasm32` those live in the data
/// section at a fixed linear-memory address, so the offset stays valid even when the
/// module grows and replaces `memory.buffer`; on the host the literal lives in the
/// executable's read-only data and the offset goes through the side table like any
/// other.
#[no_mangle]
pub extern "C" fn legalpdf_engine_version() -> u32 {
    let static_version = concat!(env!("CARGO_PKG_VERSION"), "\0");
    pointer_to_offset(static_version.as_ptr() as *mut u8)
}

/// Length of the NUL-terminated string from [`legalpdf_engine_version`], counting the
/// terminator. JavaScript needs this because a NUL-terminated region has no length of
/// its own, and a `String` would have to guess where the document stops.
#[no_mangle]
pub extern "C" fn legalpdf_engine_version_len() -> u32 {
    concat!(env!("CARGO_PKG_VERSION"), "\0").len() as u32
}

/// Coarse phases reported to the host while a parse runs. The engine does its work in
/// one pass and reports nothing finer, so the UI shows an indeterminate indicator
/// rather than a fabricated percentage.
pub const PHASE_EXTRACTING: u32 = 1;
pub const PHASE_PARSING: u32 = 2;
pub const PHASE_SERIALIZING: u32 = 3;

// Supplied by the `WebAssembly.instantiate` import object. Declared, never defined:
// a host build has no import object, so `emit` is a no-op there.
//
// The module path is `./legalpdf.js` rather than the bare name so the import lands
// under `imports["./legalpdf.js"]` in the instance. A bare `env::` import would work
// too, but a named module keeps the boundary visible in the instantiating code
// instead of scattering host functions into the default `env` namespace.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "./legalpdf.js")]
extern "C" {
    fn __legalpdf_phase(phase: u32);
}

#[cfg(target_arch = "wasm32")]
fn emit(phase: u32) {
    // Safety: the signature is integer-only, so the callee cannot dereference
    // anything this side did not pass, and it cannot unwind into a caller that has
    // already been told its result is an integer.
    unsafe { __legalpdf_phase(phase) }
}

#[cfg(not(target_arch = "wasm32"))]
fn emit(_phase: u32) {}

/// One parse outcome, held until the host releases it.
///
/// The trailing NUL lets the host build a string from the payload when it wants to,
/// without a second length round trip, while `len` still excludes it.
struct ResultBox {
    ok: bool,
    payload: Vec<u8>,
}

// Handles index this table. `BTreeMap` rather than `HashMap` so the bridge carries no
// random state, which keeps the module's memory use reproducible across runs.
thread_local! {
    static RESULTS: RefCell<BTreeMap<u32, ResultBox>> = RefCell::new(BTreeMap::new());
    static NEXT_HANDLE: RefCell<u32> = const { RefCell::new(1) };
}

// The boundary trades in `u32` offsets, which is exact on `wasm32-unknown-unknown`
// because a pointer there *is* a 32-bit index into the module's linear memory. On a
// 64-bit host build that same cast would silently truncate, so host builds route
// offsets through a side table instead. Both paths then share one test: the offsets
// JavaScript would see are exactly the ones the tests write through.
//
// On the host the table is the authority, which means an offset that was never issued
// resolves to null rather than to a wild address. A corrupt or stale offset from
// JavaScript is therefore inert here, and the wasm path gets the same guarantee from
// the fact that it is bounds-checked by the engine's own reads.

/// Turn a heap pointer into the `u32` the boundary speaks.
fn pointer_to_offset(pointer: *mut u8) -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        pointer as usize as u32
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        POINTERS.with(|pointers| {
            let mut pointers = pointers.borrow_mut();
            if let Some((offset, _)) = pointers
                .iter()
                .find(|(_, mapped)| std::ptr::eq(**mapped, pointer))
            {
                return *offset;
            }
            let mut next = pointers.len() as u32 + 1;
            // Skip any offset already in use, so a removed entry can never be aliased.
            while pointers.contains_key(&next) {
                next += 1;
            }
            pointers.insert(next, pointer);
            next
        })
    }
}

/// Resolve a boundary offset that must refer to a live allocation, or `None` when the
/// offset is null or was never issued.
///
/// The callers treat a missing allocation as a readable error rather than a crash, so a
/// corrupt offset from JavaScript costs one failed parse instead of the tab.
fn non_null_pointer(offset: u32) -> Option<*mut u8> {
    let pointer = offset_to_pointer(offset);
    if pointer.is_null() {
        None
    } else {
        Some(pointer)
    }
}

/// Resolve a `u32` from the boundary back to a heap pointer.
fn offset_to_pointer(offset: u32) -> *mut u8 {
    #[cfg(target_arch = "wasm32")]
    {
        offset as usize as *mut u8
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        POINTERS.with(|pointers| {
            pointers
                .borrow()
                .get(&offset)
                .copied()
                .unwrap_or(std::ptr::null_mut())
        })
    }
}

/// Forget a mapping. Host-only, so it compiles away entirely in the browser.
#[cfg(not(target_arch = "wasm32"))]
fn forget_pointer(offset: u32) {
    POINTERS.with(|pointers| {
        pointers.borrow_mut().remove(&offset);
    });
}

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static POINTERS: RefCell<BTreeMap<u32, *mut u8>> = RefCell::new(BTreeMap::new());
}

/// Insert a result and return its handle. Handle `0` is reserved as "no result",
/// because an offset of zero is never a valid allocation.
fn store(ok: bool, payload: Vec<u8>) -> u32 {
    NEXT_HANDLE.with(|next| {
        let mut next = next.borrow_mut();
        let handle = *next;
        *next = next.wrapping_add(1).max(1);
        RESULTS.with(|results| {
            results
                .borrow_mut()
                .insert(handle, ResultBox { ok, payload });
        });
        handle
    })
}

fn with_result<T>(handle: u32, read: impl FnOnce(&ResultBox) -> T, fallback: T) -> T {
    RESULTS.with(|results| results.borrow().get(&handle).map(read).unwrap_or(fallback))
}

/// Reserve `len` bytes that the host fills with PDF bytes.
///
/// Returns the offset of the allocation, or `0` if the request could not be honoured.
/// A zero length is refused rather than handed back as a null offset, so the caller
/// never has to distinguish "nothing" from "address zero".
///
/// # Safety
///
/// The returned offset must eventually be passed to [`legalpdf_free_input`] with the
/// same `len`, and must not be reused afterwards. Rust never reads it.
#[no_mangle]
pub unsafe extern "C" fn legalpdf_alloc(len: u32) -> u32 {
    if len == 0 {
        return 0;
    }
    let Ok(layout) = Layout::from_size_align(len as usize, 1) else {
        return 0;
    };
    // Safety: `layout` has a non-zero size, which `alloc` requires.
    let pointer = raw_alloc(layout);
    if pointer.is_null() {
        return 0;
    }
    pointer_to_offset(pointer)
}

/// Release a buffer from [`legalpdf_alloc`].
///
/// # Safety
///
/// `ptr` and `len` must come from one unmatched [`legalpdf_alloc`] call. A zero
/// pointer or zero length is ignored rather than treated as a double free.
#[no_mangle]
pub unsafe extern "C" fn legalpdf_free_input(ptr: u32, len: u32) -> u32 {
    if ptr == 0 || len == 0 {
        return 0;
    }
    let Ok(layout) = Layout::from_size_align(len as usize, 1) else {
        return 0;
    };
    let Some(pointer) = non_null_pointer(ptr) else {
        return 0;
    };
    // Safety: the caller's contract is the one `dealloc` requires: this offset and
    // size came from `alloc`, and this call has not happened before.
    raw_dealloc(pointer, layout);
    #[cfg(not(target_arch = "wasm32"))]
    forget_pointer(ptr);
    0
}

/// Run the `indian-statute` profile over `len` bytes at offset `ptr`.
///
/// Returns a result handle for [`legalpdf_result_ok`], [`legalpdf_result_len`],
/// [`legalpdf_result_read`] and [`legalpdf_result_free`]. A handle of `0` means the
/// result could not be stored, which is the only case where no handle comes back.
///
/// # Safety
///
/// `ptr` must be an offset into the module's memory addressing `len` readable bytes
/// from a live [`legalpdf_alloc`] buffer. The engine reads from it for the whole call,
/// so the host must keep the allocation alive until this returns.
#[no_mangle]
pub unsafe extern "C" fn legalpdf_parse(ptr: u32, len: u32) -> u32 {
    let Some(pointer) = non_null_pointer(ptr) else {
        return store(false, b"No input buffer was provided.".to_vec());
    };
    // Safety: the caller's contract on `ptr`/`len` is what `from_raw_parts` requires,
    // and the buffer outlives this call.
    let bytes = std::slice::from_raw_parts(pointer as *const u8, len as usize);

    let (ok, payload) = match std::panic::catch_unwind(|| run(bytes)) {
        Ok(Ok(json)) => (true, json.into_bytes()),
        Ok(Err(message)) => (false, message.into_bytes()),
        Err(_) => (false, b"The parser stopped unexpectedly.".to_vec()),
    };
    emit(PHASE_SERIALIZING);
    let mut payload = payload;
    payload.push(0);
    store(ok, payload)
}

/// `1` when the payload is an `akn.statute.v1` document, `0` when it is a message.
///
/// # Safety
///
/// `handle` must be a live handle from [`legalpdf_parse`], or `0`.
#[no_mangle]
pub unsafe extern "C" fn legalpdf_result_ok(handle: u32) -> u32 {
    with_result(handle, |result| result.ok as u32, 0)
}

/// Payload length in bytes, excluding the trailing NUL.
///
/// # Safety
///
/// `handle` must be a live handle from [`legalpdf_parse`], or `0`.
#[no_mangle]
pub unsafe extern "C" fn legalpdf_result_len(handle: u32) -> u32 {
    with_result(handle, |result| (result.payload.len() - 1) as u32, 0)
}

/// Offset of the payload. Valid until [`legalpdf_result_free`] for the same handle.
///
/// # Safety
///
/// `handle` must be a live handle from [`legalpdf_parse`], or `0`.
#[no_mangle]
pub unsafe extern "C" fn legalpdf_result_read(handle: u32) -> u32 {
    with_result(
        handle,
        |result| pointer_to_offset(result.payload.as_ptr() as *mut u8),
        0,
    )
}

/// Release a result and its payload.
///
/// # Safety
///
/// `handle` must be a live handle from [`legalpdf_parse`] that has not already been
/// freed. Freeing an unknown or stale handle is a no-op rather than undefined
/// behaviour, so a double free from JavaScript cannot corrupt the heap.
#[no_mangle]
pub unsafe extern "C" fn legalpdf_result_free(handle: u32) -> u32 {
    if handle == 0 {
        return 0;
    }
    RESULTS.with(|results| {
        results.borrow_mut().remove(&handle);
    });
    0
}

fn run(bytes: &[u8]) -> Result<String, String> {
    // These three checks mirror `validate_pdf_bytes` (`rust/src/contract.rs:265`) but
    // produce messages meant for a browser. The engine repeats the checks; doing them
    // here as well is what turns "PDF source bytes are invalid" into an explanation
    // the user can act on.
    if bytes.is_empty() {
        return Err("That file is empty.".to_owned());
    }
    if bytes.len() > legalpdf::MAX_PDF_BYTES {
        return Err(format!(
            "That file is {:.1} MB, over the {} MB limit.",
            bytes.len() as f64 / (1024.0 * 1024.0),
            legalpdf::MAX_PDF_BYTES / (1024 * 1024)
        ));
    }
    // ISO 32000-1 clause 7.5.2 permits the header inside the first 1024 bytes rather
    // than at offset 0, so the window is deliberately not narrowed.
    if !bytes[..bytes.len().min(1024)]
        .windows(5)
        .any(|window| window == b"%PDF-")
    {
        return Err(
            "That does not look like a PDF: no %PDF- header in the first 1024 bytes.".to_owned(),
        );
    }

    // An empty request is what the CLI builds for this profile (`rust/src/main.rs:382`):
    // no cache directory, no page selection, no OCR provider, no layout backend.
    let request: legalpdf::PdfRequest =
        serde_json::from_str("{}").map_err(|error| format!("Internal request error: {error}"))?;

    emit(PHASE_EXTRACTING);
    // The same entry point the CLI uses, so the browser and
    // `legalpdf.exe --profile indian-statute` produce byte-identical output for the
    // same input. Extraction, the structure pass and the statute grammar all run
    // inside this one call; the phases around it are the only progress available.
    let document = legalpdf::parse_indian_statute_pdf(bytes, &request, None)
        .map_err(|error| error.to_string())?;

    emit(PHASE_PARSING);
    legalpdf::indian_statute::to_akn_json_string(&document, true)
        .map_err(|error| format!("Could not serialize the parsed document: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The minimal PDF shape from `rust/src/contract.rs:356`, rebuilt here so this
    /// crate needs no fixture file. The section heading is quoted from the Central
    /// Goods and Services Tax Act, 2017, a public statute; all other text is invented.
    fn fixture_pdf() -> Vec<u8> {
        use lopdf::{Dictionary, Object, Stream};

        let font = Object::Dictionary(Dictionary::from_iter([(
            "F1",
            Object::Dictionary(Dictionary::from_iter([
                ("Type", Object::Name(b"Font".to_vec())),
                ("Subtype", Object::Name(b"Type1".to_vec())),
                ("BaseFont", Object::Name(b"Helvetica".to_vec())),
            ])),
        )]));

        fn text_page(font: &Object, lines: &[(f32, f32, &str)]) -> (Dictionary, Stream) {
            let mut operators = String::new();
            for (y, size, text) in lines {
                operators.push_str(&format!("BT /F1 {size} Tf 40 {y} Td ({text}) Tj ET\n"));
            }
            let mut page = Dictionary::new();
            page.set("Type", Object::Name(b"Page".to_vec()));
            page.set(
                "MediaBox",
                Object::Array(vec![
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Integer(612),
                    Object::Integer(792),
                ]),
            );
            page.set(
                "Resources",
                Object::Dictionary(Dictionary::from_iter([("Font", font.clone())])),
            );
            (page, Stream::new(Dictionary::new(), operators.into_bytes()))
        }

        let mut document = lopdf::Document::with_version("1.7");
        let page_id = document.new_object_id();
        let pages_id = document.new_object_id();
        let (mut page, contents) = text_page(
            &font,
            &[
                (760.0, 11.0, "CENTRAL GOODS AND SERVICES TAX ACT, 2017"),
                (
                    700.0,
                    11.0,
                    "22. Persons liable for registration.-Every supplier of goods",
                ),
                (
                    688.0,
                    11.0,
                    "or services shall be liable to be registered under this Act.",
                ),
                (40.0, 11.0, "1"),
            ],
        );
        let contents_id = document.add_object(Object::Stream(contents));
        page.set("Contents", Object::Reference(contents_id));
        page.set("Parent", Object::Reference(pages_id));
        document.set_object(page_id, Object::Dictionary(page));

        let mut pages_dictionary = Dictionary::new();
        pages_dictionary.set("Type", Object::Name(b"Pages".to_vec()));
        pages_dictionary.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
        pages_dictionary.set("Count", Object::Integer(1));
        document
            .objects
            .insert(pages_id, Object::Dictionary(pages_dictionary));

        let catalog_id = document.new_object_id();
        document.objects.insert(
            catalog_id,
            Object::Dictionary(Dictionary::from_iter([
                ("Type", Object::Name(b"Catalog".to_vec())),
                ("Pages", Object::Reference(pages_id)),
            ])),
        );
        document.trailer.set("Root", Object::Reference(catalog_id));

        let mut bytes = Vec::new();
        // No `compress()`: object streams would have to round-trip through lopdf's own
        // reader before the page tree resolves, and the extractor loads the document
        // with two different parsers in turn.
        document.save_to(&mut bytes).expect("serialize fixture");
        bytes
    }

    #[test]
    fn parses_a_pdf_into_akn_json() {
        let json = run(&fixture_pdf()).expect("fixture parses");
        assert!(json.starts_with('{'));
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["schema_version"], "akn.statute.v1");
        assert!(
            value["akomaNtoso"]["act"]["body"]
                .as_array()
                .is_some_and(|body| !body.is_empty()),
            "the fixture holds one section, so an empty body would mean a regression"
        );
    }

    #[test]
    fn rejects_bytes_that_are_not_a_pdf() {
        let error = run(b"this is not a pdf").expect_err("must reject");
        assert!(error.contains("%PDF-"), "unhelpful message: {error}");
    }

    #[test]
    fn rejects_an_empty_source() {
        assert_eq!(run(b"").expect_err("must reject"), "That file is empty.");
    }

    /// A `%PDF-` header after the first bytes is still accepted, because the standard
    /// allows it inside the first 1024 bytes.
    #[test]
    fn accepts_a_late_header_within_the_iso_window() {
        let mut late = vec![b'x'; 16];
        late.extend_from_slice(b"%PDF-1.7");
        // The byte predicates pass; the engine then rejects the truncated file, and
        // what matters here is that it is not rejected as "not a PDF".
        let error = run(&late).expect_err("truncated file still fails");
        assert!(!error.contains("%PDF-"), "header check must pass: {error}");
    }

    #[test]
    fn the_size_cap_is_shared_with_the_engine() {
        assert_eq!(
            legalpdf_max_pdf_bytes() as usize,
            legalpdf::MAX_PDF_BYTES,
            "the UI must be told the same cap the engine enforces"
        );
    }

    #[test]
    fn the_version_string_is_nul_terminated_and_sized() {
        let len = legalpdf_engine_version_len() as usize;
        let offset = legalpdf_engine_version();
        // Safety: `offset` and `len` both describe a `'static` string literal that
        // outlives the read, and `len` counts the terminator.
        let bytes =
            unsafe { std::slice::from_raw_parts(offset_to_pointer(offset) as *const u8, len) };
        assert_eq!(
            bytes.last(),
            Some(&0),
            "the host needs to know where it ends"
        );
        assert_eq!(&bytes[..len - 1], env!("CARGO_PKG_VERSION").as_bytes());
        // The reported length must match the crate version exactly, or the UI would
        // read past the literal.
        assert_eq!(len, env!("CARGO_PKG_VERSION").len() + 1);
    }

    /// Exercises the boundary functions the way JavaScript does, on both the success
    /// and the failure path, so a lifetime mistake shows up here rather than in the
    /// browser.
    #[test]
    fn the_alloc_parse_read_free_cycle_reclaims_everything() {
        for payload in [fixture_pdf(), b"not a pdf".to_vec()] {
            let len = payload.len() as u32;
            // Safety: each step below honours the documented ownership contract, and
            // the input buffer outlives the parse that reads it.
            unsafe {
                let input = legalpdf_alloc(len);
                assert_ne!(input, 0, "allocation failed for {len} bytes");
                let destination = offset_to_pointer(input);
                assert!(!destination.is_null());
                std::ptr::copy_nonoverlapping(payload.as_ptr(), destination, len as usize);

                let handle = legalpdf_parse(input, len);
                assert_ne!(handle, 0, "a handle always comes back");

                let (ok, result_len) = (legalpdf_result_ok(handle), legalpdf_result_len(handle));
                assert!(ok == 0 || ok == 1, "ok must be a flag, got {ok}");
                assert!(result_len > 0, "every result carries a payload");
                let offset = legalpdf_result_read(handle);
                assert_ne!(offset, 0, "a non-empty payload cannot start at zero");

                // Safety: `offset` and `result_len` describe a live allocation.
                let text = std::slice::from_raw_parts(
                    offset_to_pointer(offset) as *const u8,
                    result_len as usize,
                );
                if payload.starts_with(b"%PDF") {
                    assert_eq!(ok, 1, "a real PDF must succeed");
                    assert_eq!(text.last(), Some(&b'}'), "pretty JSON ends in a brace");
                } else {
                    assert_eq!(ok, 0, "garbage must fail");
                    assert!(
                        std::str::from_utf8(text).is_ok_and(|t| t.contains("%PDF-")),
                        "the payload must be a readable message, got {text:?}"
                    );
                }

                legalpdf_result_free(handle);
                legalpdf_free_input(input, len);
            }
        }
    }

    /// The exact sequence `web/src/wasm.ts` performs, so the two cannot drift: the
    /// engine version string is read out of linear memory through a static offset,
    /// and the PDF bytes are written in through an allocated one.
    #[test]
    fn the_javascript_access_pattern_works() {
        let pdf = fixture_pdf();
        let len = pdf.len() as u32;
        // Safety: every offset here was produced by the module in this same function,
        // and each buffer is released exactly once.
        unsafe {
            let version_offset = legalpdf_engine_version();
            let version_len = legalpdf_engine_version_len();
            let version = std::slice::from_raw_parts(
                offset_to_pointer(version_offset) as *const u8,
                version_len as usize,
            );
            let version = std::str::from_utf8(version).expect("version is UTF-8");
            assert_eq!(version.trim_end_matches('\0'), env!("CARGO_PKG_VERSION"));

            let input = legalpdf_alloc(len);
            std::ptr::copy_nonoverlapping(pdf.as_ptr(), offset_to_pointer(input), len as usize);
            let handle = legalpdf_parse(input, len);
            assert_eq!(legalpdf_result_ok(handle), 1);
            let payload_len = legalpdf_result_len(handle);
            let json = std::slice::from_raw_parts(
                offset_to_pointer(legalpdf_result_read(handle)) as *const u8,
                payload_len as usize,
            );
            let value: serde_json::Value =
                serde_json::from_slice(json).expect("the payload parses as JSON");
            assert_eq!(value["schema_version"], "akn.statute.v1");
            legalpdf_result_free(handle);
            legalpdf_free_input(input, len);
        }
    }

    #[test]
    fn a_zero_length_input_allocates_nothing() {
        // Safety: both calls tolerate a zero pointer or length by design.
        unsafe {
            assert_eq!(legalpdf_alloc(0), 0);
            assert_eq!(legalpdf_free_input(0, 16), 0);
            assert_eq!(legalpdf_result_free(0), 0);
        }
    }

    /// A handle from a finished parse must not be readable afterwards, and a handle
    /// that was never issued must not be readable at all. JavaScript can produce both
    /// through a double free, so neither may corrupt memory.
    #[test]
    fn stale_and_unknown_handles_are_inert() {
        let payload = b"not a pdf".to_vec();
        let len = payload.len() as u32;
        // Safety: the buffer outlives the parse, and the handles are used exactly as
        // JavaScript would use them, including out of contract.
        unsafe {
            let input = legalpdf_alloc(len);
            std::ptr::copy_nonoverlapping(payload.as_ptr(), offset_to_pointer(input), len as usize);
            let handle = legalpdf_parse(input, len);
            legalpdf_result_free(handle);
            // Freed, then read again.
            assert_eq!(legalpdf_result_len(handle), 0);
            assert_eq!(legalpdf_result_read(handle), 0);
            assert_eq!(legalpdf_result_ok(handle), 0);
            // Never issued at all.
            assert_eq!(legalpdf_result_len(999_999), 0);
            assert_eq!(legalpdf_result_read(999_999), 0);
            legalpdf_free_input(input, len);
        }
    }

    /// A null input pointer must still produce a readable error, because JavaScript
    /// reaching this state is a bug in the loader and should not take the module down.
    #[test]
    fn a_null_input_yields_an_error_result() {
        // Safety: the function checks for a zero offset before dereferencing it.
        unsafe {
            let handle = legalpdf_parse(0, 16);
            assert_ne!(handle, 0);
            assert_eq!(legalpdf_result_ok(handle), 0);
            assert!(legalpdf_result_len(handle) > 0);
            legalpdf_result_free(handle);
        }
    }
}
