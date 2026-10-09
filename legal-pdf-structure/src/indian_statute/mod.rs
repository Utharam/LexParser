//! Specialized Indian Legal Statutory Parser for Bare Acts and Rules (CGST/IGST).
//!
//! Parses Indian statutory hierarchy:
//! - Sections, Subsections, Clauses, Sub-clauses, Provisos, Explanations
//! - Footnotes isolated into amendment metadata
//! - Reverse-order Indian citations normalized to Akoma Ntoso (AKN) style eIds
//! - Provision table resolution and relation detection
//! - AKN-flavored JSON serialization

pub mod amendment;
pub mod ast;
pub mod citation;
pub mod eid;
pub mod normalize;
pub mod parser;
pub mod resolution;
pub mod serializer;

#[cfg(test)]
mod tests;

pub use ast::*;
pub use parser::{
    pages_to_statutory_lines, parse_indian_statute_lines, text_to_statutory_lines, StatutoryLine,
};
pub use serializer::to_akn_json_string;

use legal_pdf_core::model::Page;

/// Parses extracted PDF pages into an `IndianStatuteDocument`.
pub fn parse_indian_statute_from_pages(
    pages: &[Page],
    doc_title: Option<&str>,
) -> IndianStatuteDocument {
    let lines = pages_to_statutory_lines(pages);
    parse_indian_statute_lines(&lines, doc_title)
}

/// Parses statutory plain text into an `IndianStatuteDocument`.
pub fn parse_indian_statute_from_text(
    text: &str,
    doc_title: Option<&str>,
) -> IndianStatuteDocument {
    let lines = text_to_statutory_lines(text);
    parse_indian_statute_lines(&lines, doc_title)
}
