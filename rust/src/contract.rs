use crate::engine::{derive_extracted, parse_pdf, ParseOptions};
#[cfg(feature = "kraken")]
use crate::KrakenOptions;
#[cfg(any(feature = "ppdoc-full", feature = "ppdoc-openvino"))]
use crate::PPDocOptions;
use crate::{Error, PdfLookupRequest, PdfStructureLookup, Result};
#[cfg(feature = "ocr")]
use crate::{OcrOptions, TesseractOptions};
pub use legal_pdf_support::{PdfDocument, PdfSummary};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const MAX_SELECTED_PAGES: usize = 1_000;

fn present<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
pub struct PdfRequest {
    cache_dir: Option<PathBuf>,
    #[serde(default, deserialize_with = "present")]
    cache_key: Option<String>,
    #[serde(default, deserialize_with = "present")]
    expected_source_sha256: Option<String>,
    #[serde(default, deserialize_with = "present")]
    max_output_bytes: Option<usize>,
    #[serde(default, deserialize_with = "present")]
    pages: Option<Vec<usize>>,
    supplied_ocr: Option<crate::supplied_ocr::SuppliedOcr>,
    id: Option<String>,
    url: Option<String>,
    #[cfg(feature = "ocr")]
    #[serde(default, deserialize_with = "present")]
    ocr: Option<OcrRequest>,
    #[cfg(not(feature = "ocr"))]
    #[serde(default, deserialize_with = "present")]
    ocr: Option<serde::de::IgnoredAny>,
    #[cfg(any(feature = "ppdoc-full", feature = "ppdoc-openvino"))]
    #[serde(default, deserialize_with = "present")]
    layout: Option<LayoutRequest>,
    #[cfg(not(any(feature = "ppdoc-full", feature = "ppdoc-openvino")))]
    #[serde(default, deserialize_with = "present")]
    layout: Option<serde::de::IgnoredAny>,
}

#[cfg(feature = "ocr")]
#[derive(Deserialize)]
#[serde(tag = "provider", deny_unknown_fields)]
enum OcrRequest {
    #[serde(rename = "tesseract")]
    Tesseract {
        #[serde(default)]
        settings: TesseractOptions,
    },
    #[cfg(feature = "kraken")]
    #[serde(rename = "kraken-lite")]
    Kraken {
        #[serde(default)]
        settings: KrakenOptions,
    },
    #[cfg(not(feature = "kraken"))]
    #[serde(rename = "kraken-lite")]
    Kraken {
        #[serde(rename = "settings", default = "ignored_any")]
        _settings: serde::de::IgnoredAny,
    },
}

#[cfg(any(feature = "ppdoc-full", feature = "ppdoc-openvino"))]
#[derive(Deserialize)]
#[serde(tag = "provider", deny_unknown_fields)]
enum LayoutRequest {
    #[serde(rename = "ppdoc")]
    Ppdoc {
        #[serde(default)]
        settings: PPDocOptions,
    },
}

#[cfg(all(feature = "ocr", not(feature = "kraken")))]
fn ignored_any() -> serde::de::IgnoredAny {
    serde::de::IgnoredAny
}

fn selected_pages(pages: Option<&[usize]>) -> Result<Option<Vec<usize>>> {
    let Some(pages) = pages else {
        return Ok(None);
    };
    if pages.is_empty() || pages.len() > MAX_SELECTED_PAGES {
        return Err(Error::Message(format!(
            "document request pages requires 1 to {MAX_SELECTED_PAGES} pages"
        )));
    }
    if pages.contains(&0) {
        return Err(Error::Message(
            "document request pages must be positive integers".to_owned(),
        ));
    }
    let mut selected = pages.iter().map(|page| page - 1).collect::<Vec<_>>();
    selected.sort_unstable();
    selected.dedup();
    Ok(Some(selected))
}

fn sha256_field(value: &Option<String>, key: &str) -> Result<Option<String>> {
    value
        .as_ref()
        .map(|value| {
            if value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                Ok(value.clone())
            } else {
                Err(Error::Message(format!(
                    "document request {key} must be lowercase SHA-256"
                )))
            }
        })
        .transpose()
}

fn parse_options(request: &PdfRequest) -> Result<ParseOptions> {
    // `options.ocr` (:151) and `options.ppdoc` (:173) are assigned only under their
    // features, so the binding needs `mut` in exactly those configurations and is otherwise
    // unused-mut. Scoping the allow to the configurations that have no gated writer keeps
    // the lint live everywhere it can still fire.
    #[cfg_attr(
        not(any(feature = "ocr", feature = "ppdoc-full", feature = "ppdoc-openvino")),
        allow(unused_mut)
    )]
    let mut options = ParseOptions {
        cache_dir: request.cache_dir.clone(),
        supplied_ocr: request.supplied_ocr.clone(),
        ocr_pages: selected_pages(request.pages.as_deref())?,
        cache_key: sha256_field(&request.cache_key, "cache_key")?,
        max_output_bytes: request.max_output_bytes,
        use_cache: true,
        expected_source_sha256: sha256_field(
            &request.expected_source_sha256,
            "expected_source_sha256",
        )?,
        ..ParseOptions::default()
    };

    if request.supplied_ocr.is_some() && (request.ocr.is_some() || request.cache_key.is_some()) {
        return Err(Error::Message(
            "Supplied OCR cannot be combined with a provider or cache key".into(),
        ));
    }
    #[cfg(feature = "ocr")]
    if let Some(request) = &request.ocr {
        options.ocr = match request {
            OcrRequest::Tesseract { settings } => Some(OcrOptions::Tesseract(settings.clone())),
            #[cfg(feature = "kraken")]
            OcrRequest::Kraken { settings } => Some(OcrOptions::Kraken(settings.clone())),
            #[cfg(not(feature = "kraken"))]
            OcrRequest::Kraken { .. } => {
                return Err(Error::Message(
                    "kraken-lite requires a legalpdf binary built with the kraken feature"
                        .to_owned(),
                ));
            }
        };
    }
    #[cfg(not(feature = "ocr"))]
    if request.ocr.is_some() {
        return Err(Error::Message(
            "this legalpdf binary was built without the `ocr` feature".to_owned(),
        ));
    }

    #[cfg(any(feature = "ppdoc-full", feature = "ppdoc-openvino"))]
    if let Some(LayoutRequest::Ppdoc { settings }) = &request.layout {
        options.ppdoc = Some(settings.clone());
    }
    #[cfg(not(any(feature = "ppdoc-full", feature = "ppdoc-openvino")))]
    if request.layout.is_some() {
        return Err(Error::Message(
            "layout requires a legalpdf binary built with a layout feature".to_owned(),
        ));
    }
    Ok(options)
}

fn validate_selected_pages(selected: Option<&[usize]>, count: usize) -> Result<()> {
    let Some(selected) = selected else {
        return Ok(());
    };
    if selected.iter().any(|page| *page >= count) {
        return Err(Error::Message(
            "document request pages contains a page beyond the source PDF".to_owned(),
        ));
    }
    Ok(())
}

pub fn derive_pdf_document(bytes: &[u8], request: &PdfRequest) -> Result<PdfDocument> {
    let options = parse_options(request)?;
    let document = parse_pdf(Some(bytes), &options, None)?
        .ok_or_else(|| Error::Message("PDF cache miss after parsing source bytes".to_owned()))?;
    finish_pdf_document(document, request, &options)
}

pub fn prepare_pdf_document(bytes: &[u8], request: &PdfRequest) -> Result<PdfSummary> {
    prepare_pdf_document_reporting(bytes, request, None)
}

/// Prepares the PDF, reporting recognized pages of the pages to recognize as they finish.
pub fn prepare_pdf_document_reporting(
    bytes: &[u8],
    request: &PdfRequest,
    progress: crate::engine::RecognitionProgress<'_>,
) -> Result<PdfSummary> {
    let mut options = parse_options(request)?;
    options.require_cache_write = true;
    let document = parse_pdf(Some(bytes), &options, progress)?
        .ok_or_else(|| Error::Message("PDF cache miss after parsing source bytes".to_owned()))?;
    validate_selected_pages(options.ocr_pages.as_deref(), document.page_count())?;
    Ok(document.summary().clone())
}

pub fn restore_pdf_document(request: &PdfRequest) -> Result<Option<PdfDocument>> {
    let options = parse_options(request)?;
    parse_pdf(None, &options, None)?
        .map(|document| finish_pdf_document(document, request, &options))
        .transpose()
}

fn finish_pdf_document(
    mut document: PdfDocument,
    request: &PdfRequest,
    options: &ParseOptions,
) -> Result<PdfDocument> {
    validate_selected_pages(options.ocr_pages.as_deref(), document.page_count())?;
    let structure = document.structure_mut();
    if let Some(id) = &request.id {
        structure.document_id = id.to_owned();
    }
    structure.url = request.url.clone();
    Ok(document)
}

pub fn query_pdf_document(document: &PdfDocument, query: &PdfLookupRequest) -> PdfStructureLookup {
    document.lookup(query)
}

pub fn pdf_document_summary(document: &PdfDocument) -> &PdfSummary {
    document.summary()
}

/// The one source-size cap, shared by the CLI and every entry point that accepts bytes.
pub const MAX_PDF_BYTES: usize = 100 * 1024 * 1024;

/// The one source-acceptance predicate: non-empty, within the size cap, and a `%PDF-`
/// header inside the first 1024 bytes (ISO 32000-1 clause 7.5.2, so the window is
/// conformant and is **not** narrowed to offset 0 — see `R6`).
fn validate_pdf_bytes(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty()
        || bytes.len() > MAX_PDF_BYTES
        || !bytes[..bytes.len().min(1024)]
            .windows(5)
            .any(|window| window == b"%PDF-")
    {
        return Err(Error::Message("PDF source bytes are invalid".to_owned()));
    }
    Ok(())
}

/// Map an extraction error without discarding its kind. `Core` already *is* a
/// `legal_pdf_core::Error` and must pass through unchanged; the other three variants have
/// no counterpart in `legal_pdf_core::Error` and can only be rendered. The blanket `From`
/// impl (`legal-pdf-extraction-processor/src/error.rs:13-17`) would stringify `Core` too,
/// which is the defect `A24` records.
fn map_extraction_error(error: legal_pdf_extraction::Error) -> Error {
    match error {
        legal_pdf_extraction::Error::Core(inner) => inner,
        other => Error::Message(other.to_string()),
    }
}

pub fn parse_indian_statute_pdf(
    bytes: &[u8],
    request: &PdfRequest,
    doc_title: Option<&str>,
) -> Result<legal_pdf_structure::indian_statute::IndianStatuteDocument> {
    validate_pdf_bytes(bytes)?;

    let options = parse_options(request)?;
    let source_sha256 = format!("{:x}", Sha256::digest(bytes));
    if options
        .expected_source_sha256
        .as_deref()
        .is_some_and(|expected| expected != source_sha256)
    {
        return Err(Error::Message(
            "PDF source changed after preparation began".to_owned(),
        ));
    }

    // `None, None`, exactly as `engine.rs:586`: this profile has no OCR provider
    // (`README.md:63`), so the OCR page selection is meaningless at extraction time. The
    // pipeline itself forwards `options.ocr_pages` to `recognize_pdf` (`engine.rs:609`),
    // not to `extract_pdf`, and this path runs neither.
    let mut extracted =
        legal_pdf_extraction::extract_pdf(bytes, None, None).map_err(map_extraction_error)?;

    // `PdfRequest.pages` is honoured exactly as every other entry point honours it:
    // validated against the real page count, and nothing more
    // (`prepare_pdf_document_reporting:217` and `finish_pdf_document:233` do the same).
    // It does **not** restrict the returned document — see `D2`.
    validate_selected_pages(options.ocr_pages.as_deref(), extracted.pages.len())?;

    // The statute grammar consumes `Page::exclude_from_body` and `Line::region_type`, and
    // both are set only by the structure pass (`structure.rs:889, 991`). Without it the
    // furniture filter at `parser.rs:78` rejects nothing, and page headers, footers and
    // printed folios enter the body stream to be re-classified by seven regexes.
    // `derive_extracted` is the existing helper the rest of the pipeline uses
    // (`engine.rs:264-279`); make it `pub(crate)` and call it rather than re-deriving the
    // same four lines. Its diagnostics land in `extracted.diagnostics`, which the parser
    // forwards onto the document.
    let document_id = request
        .id
        .clone()
        .unwrap_or_else(|| format!("doc-{}", &source_sha256[..20]));
    let _structure = derive_extracted(&mut extracted, &document_id, &source_sha256)?;

    let mut document = legal_pdf_structure::indian_statute::parse_indian_statute_from_pages(
        &extracted.pages,
        doc_title,
    );
    // The parser can only report what it observes in the page stream, so the extraction and
    // structure-pass diagnostics are forwarded ahead of its own recoveries. Running the
    // structure pass must not discard them silently.
    let mut forwarded = extracted.diagnostics;
    forwarded.append(&mut document.diagnostics);
    document.diagnostics = forwarded;
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a two-page PDF in memory. No checked-in binary and no corpus file. The
    /// section headings are taken from the Central Goods and Services Tax Act, 2017, a
    /// public statute published by the Government of India; everything else is invented.
    /// `lopdf` is already a dev-dependency (`Cargo.toml:59`).
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

        // A page is a plain dictionary whose `/Contents` names a separate stream: a page
        // stored as a stream is invisible to `lopdf::Document::get_pages`, which is what
        // every reader here uses to find the page tree.
        fn text_page(font: &Object, lines: &[(f32, f32, &str)]) -> (Dictionary, Stream) {
            let mut operators = String::new();
            for (y, size, text) in lines {
                operators.push_str(&format!("BT /F1 {size} Tf 40 {y} Td ({text}) Tj ET\n"));
            }
            let mut page = Dictionary::new();
            page.set("Type", Object::Name(b"Page".to_vec()));
            // The extractor needs real page geometry before it will report a readable page.
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

        // A page with no embedded text at all: the extractor reports it as OCR_REQUIRED,
        // which is what makes the diagnostic forwarding observable.
        fn shape_page() -> (Dictionary, Stream) {
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
            page.set("Resources", Object::Dictionary(Dictionary::new()));
            (
                page,
                Stream::new(
                    Dictionary::new(),
                    b"0.2 0.2 0.2 rg 100 600 200 100 re f\n".to_vec(),
                ),
            )
        }

        let mut document = lopdf::Document::with_version("1.7");
        let page_ids: Vec<lopdf::ObjectId> = (0..3).map(|_| document.new_object_id()).collect();
        let pages_id = document.new_object_id();
        // Lines are kept short enough not to wrap: a wrapped line is re-joined by the
        // extractor and would corrupt the furniture geometry this fixture exists to test.
        let page_one = text_page(
            &font,
            &[
                (760.0, 11.0, "CENTRAL GOODS AND SERVICES TAX ACT, 2017"),
                (
                    700.0,
                    11.0,
                    "16. Eligibility and conditions for taking input tax credit.-",
                ),
                (
                    688.0,
                    11.0,
                    "(1) Every registered person shall be entitled to take credit",
                ),
                (
                    676.0,
                    11.0,
                    "of input tax charged on any supply of goods or services.",
                ),
                (40.0, 11.0, "1"),
            ],
        );
        let page_two = text_page(
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
                (40.0, 11.0, "2"),
            ],
        );
        for (id, (page, contents)) in page_ids.iter().zip([page_one, page_two, shape_page()]) {
            let contents_id = document.add_object(Object::Stream(contents));
            let mut page = page;
            page.set("Contents", Object::Reference(contents_id));
            page.set("Parent", Object::Reference(pages_id));
            document.set_object(*id, Object::Dictionary(page));
        }

        let mut pages_dictionary = Dictionary::new();
        pages_dictionary.set("Type", Object::Name(b"Pages".to_vec()));
        pages_dictionary.set(
            "Kids",
            Object::Array(page_ids.iter().map(|id| Object::Reference(*id)).collect()),
        );
        pages_dictionary.set("Count", Object::Integer(page_ids.len() as i64));
        document
            .objects
            .insert(pages_id, Object::Dictionary(pages_dictionary));

        let catalog_id = document.new_object_id();
        document.objects.insert(
            catalog_id,
            Object::Dictionary(Dictionary::from_iter([
                ("Type", Object::Name(b"Catalog".to_vec())),
                // Without this the catalog has no page tree and every caller sees zero pages.
                ("Pages", Object::Reference(pages_id)),
            ])),
        );
        document.trailer.set("Root", Object::Reference(catalog_id));

        let mut bytes = Vec::new();
        // No `compress()`: object streams would have to round-trip through lopdf's own
        // reader before the page tree resolves, and the extractor loads with two different
        // parsers in turn.
        document.save_to(&mut bytes).expect("serialize fixture");
        bytes
    }

    fn empty_request() -> PdfRequest {
        serde_json::from_value(serde_json::json!({})).expect("empty request")
    }

    fn tree_text(nodes: &[legal_pdf_structure::indian_statute::ProvisionNode]) -> String {
        nodes
            .iter()
            .map(|node| {
                let own = [
                    node.heading.clone().unwrap_or_default(),
                    node.num.clone(),
                    node.text.clone(),
                ]
                .join(" ");
                format!("{own} {}", tree_text(&node.children))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn assert_message(error: &Error, expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[test]
    fn indian_statute_pdf_excludes_furniture_from_the_tree() {
        let doc = parse_indian_statute_pdf(&fixture_pdf(), &empty_request(), None)
            .expect("fixture parses");
        let haystack = tree_text(&doc.akoma_ntoso.act.body);
        assert!(
            !haystack.contains("CENTRAL GOODS AND SERVICES TAX ACT, 2017"),
            "the running head reached the statute tree: {haystack}"
        );
        for folio in [" 1 ", " 2 "] {
            assert!(
                !haystack.contains(folio),
                "printed folio {folio:?} reached the statute tree: {haystack}"
            );
        }
        // The section lines themselves survived, so the assertions above are about furniture
        // and not about an empty document.
        assert!(haystack.contains("Eligibility and conditions"));
        assert!(haystack.contains("Persons liable for registration"));
    }

    #[test]
    fn indian_statute_pdf_forwards_structure_diagnostics() {
        let doc = parse_indian_statute_pdf(&fixture_pdf(), &empty_request(), None)
            .expect("fixture parses");
        assert!(
            doc.diagnostics.iter().any(|d| d.code == "OCR_REQUIRED"),
            "the pipeline's own diagnostic must reach the document, got {:?}",
            doc.diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
    }

    #[test]
    fn indian_statute_pdf_rejects_expected_sha256_mismatch() {
        let request: PdfRequest = serde_json::from_value(serde_json::json!({
            "expected_source_sha256":
                "0000000000000000000000000000000000000000000000000000000000000000",
        }))
        .expect("request");
        let error = parse_indian_statute_pdf(&fixture_pdf(), &request, None)
            .expect_err("digest mismatch must be rejected");
        assert_message(&error, "PDF source changed after preparation began");
    }

    #[test]
    fn indian_statute_pdf_preserves_extraction_error_kinds() {
        // A `Core` error must arrive as its original variant rather than being stringified
        // into `Error::Message`.
        let mapped = map_extraction_error(legal_pdf_extraction::Error::Core(
            legal_pdf_core::Error::Message("core detail".to_owned()),
        ));
        assert!(
            matches!(&mapped, Error::Message(text) if text == "core detail"),
            "a Core error must be the Core variant, got {mapped:?}"
        );

        let rendered =
            map_extraction_error(legal_pdf_extraction::Error::Message("no pages".to_owned()));
        assert!(matches!(&rendered, Error::Message(text) if text == "no pages"));
    }

    #[test]
    fn indian_statute_pdf_rejects_pages_beyond_the_source() {
        let request: PdfRequest =
            serde_json::from_value(serde_json::json!({ "pages": [99] })).expect("request");
        let error = parse_indian_statute_pdf(&fixture_pdf(), &request, None)
            .expect_err("page beyond the source must be rejected");
        assert_message(
            &error,
            "document request pages contains a page beyond the source PDF",
        );
    }

    #[test]
    fn indian_statute_pdf_validates_source_bytes() {
        assert_message(
            &validate_pdf_bytes(b"").expect_err("empty"),
            "PDF source bytes are invalid",
        );
        assert_message(
            &validate_pdf_bytes(&vec![b'x'; MAX_PDF_BYTES + 1]).expect_err("oversize"),
            "PDF source bytes are invalid",
        );
        // A `%PDF-` header inside the ISO 32000-1 1024-byte window is accepted, not
        // required at offset 0 (`R6`).
        let mut late_header = vec![b'x'; 16];
        late_header.extend_from_slice(b"%PDF-1.7");
        assert!(validate_pdf_bytes(&late_header).is_ok());
    }
}
