use serde::{Deserialize, Serialize};

/// High-level statutory element kind in an Indian Bare Act or Rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvisionKind {
    Chapter,
    Section,
    Subsection,
    Clause,
    Subclause,
    Proviso,
    Explanation,
    Schedule,
    ScheduleItem,
}

/// Nature of legal relation in a statutory cross-reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    /// Non-obstante clause ("Notwithstanding anything contained in...")
    Notwithstanding,
    /// Subordination / conditional clause ("Subject to the provisions of...")
    SubjectTo,
    /// Standard reference ("referred to in...", "specified in...")
    Reference,
}

/// Target type when referencing an instrument outside the current Act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalTargetType {
    Rule,
    ExternalAct,
}

/// Cross-reference / citation extracted from statutory prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitationReference {
    /// eId of the provision enclosing this citation reference.
    pub parent_eid: String,
    /// Exact text matched from the provision source.
    pub raw_text: String,
    /// Exact Unicode character offsets [start, end] inside `parent.text`.
    pub span: [usize; 2],
    /// Normalized Akoma Ntoso style identifier (e.g., "sec_22__subsec_1").
    pub target_eid: String,
    /// Detected relation type ("notwithstanding", "subject_to", or "reference").
    pub relation: RelationType,
    /// Whether the target provision was found in this document's provision table.
    pub resolved: bool,
    /// Whether this reference points outside the current parsed Act (e.g. to Rules or another Act).
    pub external: bool,
    /// When `external` is true, indicates whether the target is a Rule or an external Act.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_type: Option<ExternalTargetType>,
}

/// Amendment details parsed from statutory footnotes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmendmentMetadata {
    /// Numeric marker corresponding to footnote (e.g. "1").
    pub footnote_number: String,
    /// Action taken: "substituted", "inserted", "omitted", or "amended".
    pub action: String,
    /// Amending legislation, if identified (e.g. "Act 12 of 2020").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amending_act: Option<String>,
    /// Amending section, if identified (e.g. "s. 118").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amending_section: Option<String>,
    /// Date of coming into force, e.g. "1-1-2021".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_date: Option<String>,
    /// Text substituted for, if indicated in footnote.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substituted_for: Option<String>,
    /// Raw footnote text.
    pub raw_text: String,
}

/// A node in the statutory hierarchy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvisionNode {
    /// Unique Akoma Ntoso eId, e.g. "sec_16", "sec_16__subsec_2", "sec_16__subsec_2__cl_a".
    pub eid: String,
    /// Provision kind.
    pub kind: ProvisionKind,
    /// Provision designator / numeral (e.g. "16", "(1)", "(a)", "(i)", "Provided that", "Explanation 1").
    pub num: String,
    /// Provision title or marginal note heading (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    /// Clean body text for this specific node.
    pub text: String,
    /// Parent eId in the AST, if not a root section or chapter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_eid: Option<String>,
    /// Nested child provisions (e.g., subsections under section, clauses under subsection).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<ProvisionNode>,
    /// Citations detected directly within this node's text.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub citations: Vec<CitationReference>,
    /// Amendments linked directly to this node via footnote markers.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub amendments: Vec<AmendmentMetadata>,
    /// Source page numbers where this provision appears, ascending and deduplicated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub page_numbers: Vec<u32>,
}

/// Document-level footnote entry retained for verification and provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FootnoteItem {
    pub number: String,
    pub raw_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amendment: Option<AmendmentMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub associated_eid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_number: Option<u32>,
}

/// Document metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DocumentMeta {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub act_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub act_year: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// AKN Act wrapper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AknAct {
    pub meta: DocumentMeta,
    pub body: Vec<ProvisionNode>,
}

/// Top-level Akoma Ntoso container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AkomaNtoso {
    pub act: AknAct,
}

/// Complete parsed Indian Bare Act document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndianStatuteDocument {
    pub schema_version: String,
    #[serde(rename = "akomaNtoso")]
    pub akoma_ntoso: AkomaNtoso,
    /// Flat list of all provisions for table lookup.
    pub provisions: Vec<ProvisionNode>,
    /// Consolidated flat list of all citation references across the document.
    pub citations: Vec<CitationReference>,
    /// Document-level list of all footnotes.
    pub footnotes: Vec<FootnoteItem>,
    /// Non-fatal parse observations, in document order: extraction and structure-pass
    /// diagnostics forwarded from the pipeline, plus this parser's own recoveries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<legal_pdf_core::model::Diagnostic>,
}
