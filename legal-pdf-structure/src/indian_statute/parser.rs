//! Hierarchical parser for an Indian Bare Act or Rules.
//!
//! `sec_unknown` is the `parent_eid` minted when a provision arrives with no open section
//! ancestor. It is **not** a bucket: every push site re-attaches such a provision as a root
//! (see `reparent_to_root`), so the prefix survives only as the record that no section
//! ancestor was open.

use super::amendment::parse_amendment_footnote;
use super::ast::{
    AknAct, AkomaNtoso, CitationReference, DocumentMeta, FootnoteItem, IndianStatuteDocument,
    ProvisionKind, ProvisionNode,
};
use super::citation::extract_citations;
use super::eid;
use super::resolution::{
    build_provision_table, resolve_citations, resolve_provision_tree_citations,
};
use legal_pdf_core::model::{Diagnostic, Line, Page};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

static SEC_START_RE: OnceLock<Regex> = OnceLock::new();
static SUBSEC_RE: OnceLock<Regex> = OnceLock::new();
static CLAUSE_RE: OnceLock<Regex> = OnceLock::new();
static SUBCLAUSE_RE: OnceLock<Regex> = OnceLock::new();
static PROVISO_RE: OnceLock<Regex> = OnceLock::new();
static EXPL_RE: OnceLock<Regex> = OnceLock::new();
static FOOTNOTE_LINE_RE: OnceLock<Regex> = OnceLock::new();
static NOTE_TAIL_RE: OnceLock<Regex> = OnceLock::new();
static CHAPTER_MARKER_RE: OnceLock<Regex> = OnceLock::new();
static CHAPTER_TITLE_RE: OnceLock<Regex> = OnceLock::new();

fn get_sec_start_re() -> &'static Regex {
    SEC_START_RE.get_or_init(|| {
        // The prefixed form accepts any numeral: "Section 2019 …" is a citation-like
        // phrase, not a bare year. The bare form caps at three digits so a bare year
        // ("2019. The Government may …") cannot mint a section.
        //
        // A gazette marks an inserted or substituted heading with the amendment ordinal and
        // an opening bracket, so the heading is printed as `26[Section 11A. Power not to
        // recover …`. By the time a line reaches here the superscript ordinal has usually been
        // canonicalised into a note marker, so the same line arrives as `[26][Section 11A. …`;
        // both forms are accepted, and the ordinal itself is discarded rather than captured as
        // a numeral because it belongs to the amending footnote. Without this 17 sections of
        // the CGST Act were never minted, and because a missed section start also fails to
        // close the previous one, their sub-sections were credited to whatever section happened
        // to be open — which is where most duplicate eIds came from.
        //
        // The bare branch stays capped at three digits so a bare year cannot mint a section;
        // the `Section`-prefixed branch and the bracketed branch are unambiguous and may be
        // longer.
        //
        // The whole tail after the dot is captured rather than `\s*(?P<rest>.*)$`, because
        // the decimal/date guard has to be applied by `starts_a_section`: the `regex` crate
        // has no look-around, so the plan's `(?!\d)` cannot be written in the pattern.
        Regex::new(concat!(
            r"^(?:\[\d+\])?(?:\d+\[)?\[?\s*",
            r"(?:Section\s+(?P<sec_sp>[0-9]+[A-Za-z]*)",
            r"|(?P<sec>[0-9]{1,3}[A-Za-z]*))",
            r"\.(?P<after>.*)$"
        ))
        .unwrap()
    })
}

/// A section start whose `.` is immediately followed by a digit is a decimal or a date
/// ("1.5 per cent", "1.4.2019.", "3.14. Value of …"), not a section. This is the guard the
/// plan writes as `(?!\d)`; `regex` has no look-around, so it lives here.
fn sec_start_dot_is_guarded(after: &str) -> bool {
    !after.starts_with(|c: char| c.is_ascii_digit())
}

/// The section number and the remainder of `text`, or `None` when `text` does not open a
/// section. One predicate for both the main loop and `starts_a_provision`, so the two can
/// never disagree about which lines are section starts.
fn section_start(text: &str) -> Option<(String, &str)> {
    let caps = get_sec_start_re().captures(text)?;
    let after = caps.name("after").map_or("", |m| m.as_str());
    if !sec_start_dot_is_guarded(after) {
        return None;
    }
    let sec_num = caps
        .name("sec")
        .or_else(|| caps.name("sec_sp"))?
        .as_str()
        .to_owned();
    Some((sec_num, after.trim()))
}

fn get_subsec_re() -> &'static Regex {
    SUBSEC_RE.get_or_init(|| {
        Regex::new(
            &[
                r"^\((?P<subsec>",
                eid::SECTION_NUMERAL,
                r")\)\s*(?P<rest>.*)$",
            ]
            .concat(),
        )
        .unwrap()
    })
}

fn get_clause_re() -> &'static Regex {
    CLAUSE_RE.get_or_init(|| {
        // `(?i:…)` is scoped to the bracket and the token: an amending Act prints a newly
        // inserted letter upper-case, `(A)` and `(B)` beside the surviving `(a)` and `(b)`,
        // and it is a clause designator like any other. `rest` and the anchor stay
        // case-sensitive, and `eid::CLAUSE_TOKEN` keeps its lower-case class so a
        // three-letter token still cannot match.
        Regex::new(
            &[
                r"^(?i:\((?P<cl>",
                eid::CLAUSE_TOKEN,
                r"))\)\s*(?P<rest>.*)$",
            ]
            .concat(),
        )
        .unwrap()
    })
}

fn get_subclause_re() -> &'static Regex {
    SUBCLAUSE_RE.get_or_init(|| {
        Regex::new(
            &[
                r"^\((?P<subcl>",
                eid::SUBCLAUSE_TOKEN,
                r")\)\s*(?P<rest>.*)$",
            ]
            .concat(),
        )
        .unwrap()
    })
}

fn get_proviso_re() -> &'static Regex {
    PROVISO_RE.get_or_init(|| {
        // The adverb group may repeat and may be followed by a comma, and `that` is
        // optional: "Provided further also that", "Provided further, that",
        // "Provided always that", and the continuous-prose form "Provided the Court
        // thinks fit" are all real. `num` carries `lead` verbatim, so the group stops at
        // the optional `that` and is trimmed where it is read.
        Regex::new(concat!(
            r"^(?P<lead>Provided(?:\s+(?:also|always|even\s+further|further|hereinbefore|therein))*\s*,?\s*(?:that\b)?)",
            r"(?P<rest>.*)$"
        ))
        .unwrap()
    })
}

fn get_expl_re() -> &'static Regex {
    EXPL_RE.get_or_init(|| {
        Regex::new(r"^Explanation(?:\s+(?P<num>[0-9]+))?(?:\.—|\.-|—|-|:|\.)\s*(?P<rest>.*)$")
            .unwrap()
    })
}

fn get_footnote_line_re() -> &'static Regex {
    FOOTNOTE_LINE_RE.get_or_init(|| {
        Regex::new(
            &[
                // `(?i)`: the action detector lowercases while the lead vocabulary is
                // printed mixed-case, so a lower-case `subs.` must still separate.
                r"(?i)^",
                super::amendment::LEAD_NUMBER,
                r"(?P<lead>",
                super::amendment::LEAD_ALTERNATION,
                r").*$",
            ]
            .concat(),
        )
        .unwrap()
    })
}

fn get_chapter_marker_re() -> &'static Regex {
    // A division heading is the word, an optional designator, and then the end of the line:
    // "CHAPTER", "CHAPTER IV", "PART A", "SCHEDULE", "SCHEDULES", "CHAPTER II—RATES". The
    // plural is the back-matter form of a consolidated Act. The looser form that only
    // required the leading word also matched ordinary prose, and a boundary invented on a
    // prose line closes the open section and re-parents everything printed under it:
    // "Part of the value of supply shall be credited", "Chapter XXVII of the Code of
    // Criminal Procedure shall apply". What excludes prose is the `\s*$` anchor rather than
    // the word form, so accepting the plural re-admits none of them. Case-insensitivity is
    // scoped to the word and the roman designator so the letter designator stays upper-case.
    //
    // A hyphen may only ever form a designator suffix. It once shared the separator class
    // with the colon and the dashes, which let that single character introduce a title or a
    // sentence: in `computed as per Chapter IV-D as increased by the aggregate amount of the`
    // the tail after `-D` was taken as a title, so a wrapped definition became a boundary.
    // That is the whole shape of the trap. `IV-D` is a designator and the line must stop
    // after it; a title arrives after a colon or a dash, and a title is prose like any
    // other, so it cannot be told from a continuation by looking at this line alone. Hence
    // the split: a hyphen extends the designator, a colon or a dash introduces the title.
    CHAPTER_MARKER_RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?i:^(?:THE\s+)?(?:FIRST|SECOND|THIRD|FOURTH|FIFTH|SIXTH|SEVENTH|EIGHTH|NINTH|TENT)?\s*",
            r"(?:CHAPTER|PART|SCHEDULE)S?(?:\s+[ivxlcdm]+)?)",
            r"(?:\s+[A-Z]{1,3}|\s+[0-9]{1,3})?",
            r"(?:-\s?[A-Z]{1,3})?",
            r"(?:\s*[:\u{2014}\u{2013}]+\s*\S.*)?\s*$",
        ))
        .unwrap()
    })
}

// A gazette breaks a line wherever the column runs out, so "as per" can leave a division
// designator alone on the next line, and nothing about that line distinguishes it from a
// heading. The unfinished sentence in front of it is the only remaining evidence, so the
// paragraph has to be read, not just the line. These are the words that cannot close a
// heading and routinely carry a line of running prose into the next.
fn sentence_carries_over(previous: &str) -> bool {
    const CONNECTORS: &[&str] = &[
        "a", "an", "and", "as", "at", "by", "for", "from", "in", "into", "of", "on", "or", "per",
        "than", "that", "the", "this", "to", "under", "with", "which", "whose",
    ];
    let last = previous
        .split_whitespace()
        .next_back()
        .unwrap_or_default()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    CONNECTORS.contains(&last.as_str())
}

fn get_chapter_title_re() -> &'static Regex {
    // A bare part title: upper case, no lower-case letter, no terminal full stop.
    // "PRELIMINARY", "RATES AND CHARGES". Matched ONLY on the line immediately after a
    // chapter marker, so an isolated all-caps provision line is never taken for a title.
    CHAPTER_TITLE_RE.get_or_init(|| Regex::new(r"^[A-Z][A-Z0-9 ,.'()/&-]*[A-Z0-9)]$").unwrap())
}

/// Append a continuation line to an open node's text, space-separated, and record the
/// page it came from.
fn push_continuation(node: &mut ProvisionNode, text: &str, page_number: u32) {
    if !node.text.is_empty() {
        node.text.push(' ');
    }
    node.text.push_str(text);
    if !node.page_numbers.contains(&page_number) {
        node.page_numbers.push(page_number);
    }
}

/// A provision recovered from an orphan position becomes a root. Its eId keeps the
/// `sec_unknown` prefix it was minted with — that prefix is the record that no section
/// ancestor was open — but it must not claim a parent that does not exist.
fn reparent_to_root(mut node: ProvisionNode) -> ProvisionNode {
    node.parent_eid = None;
    node
}

/// Mints an eId that is unique within the document, and reports every collision.
///
/// A sub-section, clause or sub-clause eId is a pure function of its parent and designator,
/// so two blocks that share a parent and a designator mint byte-identical eIds. The first use
/// keeps the base verbatim, which leaves every currently-correct eId unchanged; a repeat gains
/// `__dup2`, `__dup3` and so on. Deduplicating the flat list instead would make the count
/// agree while leaving two tree nodes sharing one identity, which `ast.rs` forbids and which
/// `build_provision_table` would still collapse.
///
/// The suffix keeps the tree internally consistent but reports nothing. A collision means a
/// container boundary was missed upstream, and the suffix is the only trace of that, in a field
/// no consumer inspects. So the collision is also counted in `diagnostics`, which is part of
/// the document contract. The eId must keep the suffix either way: it is the join key that
/// `resolve_citations`, footnote attachment and the citation passes all compare as a bare
/// string, so moving it to a sibling field would make the underlying collision undetectable
/// rather than merely less visible.
fn dedup_eid(
    counts: &mut HashMap<String, usize>,
    diagnostics: &mut Vec<Diagnostic>,
    kind: &str,
    page: u32,
    base: String,
) -> String {
    let count = counts.entry(base.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        base
    } else {
        diagnostics.push(Diagnostic::warning(
            "statute.duplicate_provision_eid",
            format!("collision {count} on {kind} base {base} on page {page}"),
            None,
        ));
        format!("{base}__dup{count}")
    }
}

/// The one continuation path. Front matter becomes `preamble`; a stray line inside an open
/// chapter joins that chapter; anything else is reported rather than dropped silently.
fn append_continuation_line(
    clause: &mut Option<ProvisionNode>,
    subsec: &mut Option<ProvisionNode>,
    section: &mut Option<ProvisionNode>,
    roots: &mut Vec<ProvisionNode>,
    preamble: &mut Vec<StatutoryLine>,
    diagnostics: &mut Vec<Diagnostic>,
    text: &str,
    page_number: u32,
) {
    if let Some(c) = clause {
        push_continuation(c, text, page_number);
    } else if let Some(s) = subsec {
        push_continuation(s, text, page_number);
    } else if let Some(sec) = section {
        push_continuation(sec, text, page_number);
    } else if roots.is_empty() {
        // Nothing is open and nothing has been emitted yet: this is the long title,
        // the enacting formula, the chapter list — front matter, not a provision.
        preamble.push(StatutoryLine {
            text: text.to_owned(),
            page_number,
            note_class: NoteClass::Unknown,
        });
    } else if let Some(chapter) = roots
        .last_mut()
        .filter(|n| n.kind == ProvisionKind::Chapter)
    {
        // A stray line between a chapter heading and its first section belongs to the
        // chapter (schedule preambles do this).
        push_continuation(chapter, text, page_number);
    } else {
        diagnostics.push(Diagnostic::warning(
            "statute.orphan_line",
            format!("line matches no statutory construct and no provision is open: {text}"),
            None,
        ));
    }
}

/// Whether `rest` is a marginal note rather than body text. A marginal note is short, is
/// one sentence with no internal terminator, carries no statutory locator, and does not
/// open like a sentence. This is a labelled heuristic; the conditions below are the whole
/// rule and none may be added to without re-reading `P21`.
fn looks_like_marginal_note(rest: &str) -> bool {
    /// Words that open a sentence. A marginal note is a noun phrase.
    const SENTENCE_OPENERS: [&str; 14] = [
        "the ", "a ", "an ", "it ", "this ", "that ", "these ", "those ", "no ", "nothing ",
        "every ", "any ", "where ", "when ",
    ];
    let lower = rest.to_ascii_lowercase();
    !rest.is_empty()
        && rest.len() <= 96
        && !rest.contains(". ")
        && !rest.contains(';')
        && !rest.contains("section ")
        && !rest.contains("Section ")
        && !rest.contains("Article")
        && !rest.contains(" rule")
        && !rest.contains(" Act")
        && !SENTENCE_OPENERS
            .iter()
            .any(|opener| lower.starts_with(opener))
}

/// The clause label this container expects next: a..z, then the Indian doubled form
/// aa, ab, … which is exactly what `[a-z]{1,2}` accepts.
fn expected_clause_label(ordinal: usize) -> String {
    const LETTERS: usize = 26;
    if ordinal < LETTERS {
        ((b'a' + ordinal as u8) as char).to_string()
    } else {
        let rest = ordinal - LETTERS;
        let tens = (b'a' + (rest / LETTERS) as u8) as char;
        let ones = (b'a' + (rest % LETTERS) as u8) as char;
        format!("{tens}{ones}")
    }
}

/// The token inside a leading `( … )`, if the line starts with one.
fn leading_paren_token(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('(')?;
    let end = rest.find(')')?;
    Some(&rest[..end])
}

/// True when `token` is a pure roman designator.
fn is_roman_token(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|b| matches!(b, b'i' | b'v' | b'x'))
}

/// A footnote marker line plus the non-marker lines that continue it, held until the note
/// is closed.
struct PendingFootnote {
    number: String,
    lines: Vec<StatutoryLine>,
}

fn flush_pending_footnote(
    pending: &mut Option<PendingFootnote>,
    footnotes: &mut Vec<FootnoteItem>,
) {
    let Some(note) = pending.take() else {
        return;
    };
    let raw = note
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    footnotes.push(FootnoteItem {
        number: note.number,
        amendment: parse_amendment_footnote(&raw),
        raw_text: raw,
        associated_eid: None,
        page_number: Some(note.lines[0].page_number),
    });
}

/// Reads the footnote number from the front of a note line, tolerating the
/// period-less form. Gazette footnotes print `1.`, `1` and `1 ` interchangeably.
use super::amendment::leading_integer as leading_note_number;

/// Rewrites a body line's note references into a canonical bracketed form.
///
/// A gazette footnote reference is set as a superscript digit welded to the end of
/// the preceding word or bracket, as in `India [*****]1`. Plain-text matching cannot
/// tell that trailing `1` from any other digit, and matching on the surrounding
/// characters is guesswork. The structure pass already reports which spans are
/// superscript, so the marker is bracketed here and matched exactly downstream.
///
/// Offsets are byte offsets into `line.text`, and inserting right to left keeps the
/// not-yet-edited offsets valid.
/// Converts a character range into the byte range covering the same characters.
///
/// `Span::start` and `Span::end` are character offsets into `Line::text` — the extraction
/// pass clamps them against `raw_text.chars().count()`. Slicing a `str` by those values
/// directly is a byte operation, so on any line containing a multi-byte character before a
/// marker the range lands mid-character and silently drops or corrupts the text around it.
fn char_range_to_byte_range(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    if start >= end || end > text.chars().count() {
        return None;
    }
    let byte_start = text.char_indices().nth(start).map(|(index, _)| index)?;
    let byte_end = text
        .char_indices()
        .nth(end)
        .map_or(text.len(), |(index, _)| index);
    Some((byte_start, byte_end))
}

fn mark_note_references(line: &Line) -> String {
    // Operates on the untrimmed line: the span offsets are relative to exactly this string.
    let mut text = line.text.clone();
    let mut inserts: Vec<(usize, usize, String)> = line
        .spans
        .iter()
        .filter(|span| span.superscript)
        .filter(|span| !span.text.is_empty() && span.text.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|span| {
            char_range_to_byte_range(&text, span.start, span.end)
                .map(|(start, end)| (start, end, format!("[{}]", span.text)))
        })
        .collect();

    inserts.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    for (start, end, marker) in inserts {
        // A reference already bracketed needs no second pair.
        let already = text[..start].ends_with('[') && text[end..].starts_with(']');
        if !already {
            text.replace_range(start..end, &marker);
        }
    }
    text
}

/// The enforcement tail a gazette note ends with, and the one shape of note continuation
/// that carries no label of its own: "– Brought into force w.e.f. 1 February, 2019.",
/// "— Commenced on …", "– With effect from …", optionally preceded by the Act's gazette
/// number. Operative statutory prose does not open this way; it says "shall come into force
/// on the date appointed". The dash-anchored opening is the discriminator, so a sentence
/// that merely contains the phrase cannot match.
fn get_note_tail_re() -> &'static Regex {
    NOTE_TAIL_RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?i)^\s*(?:\(\s*no\.\s*[0-9]{1,3}\s+of\s+(?:19|20)[0-9]{2}\s*\)\s*)?[—–-]{1,3}\s*",
            r"(?:brought\s+into\s+force|come\s+into\s+force|force\s+came\s+into\s+force",
            r"|commenced\s+on|with\s+effect\s+from|effective\s+from)\b",
        ))
        .unwrap()
    })
}
fn note_class_for(line: &Line) -> NoteClass {
    if line.region_type == "footnote" || !line.note_region_mode.is_empty() {
        NoteClass::Note
    } else if line.region_type == "body" {
        NoteClass::Body
    } else {
        // Header, footer, heading, or a document that never ran a structure pass.
        // The first three are excluded upstream; the last must stay open to the
        // text patterns rather than be guessed at.
        NoteClass::Unknown
    }
}

fn starts_a_provision(text: &str) -> bool {
    section_start(text).is_some()
        || get_subsec_re().is_match(text)
        || get_clause_re().is_match(text)
        || get_proviso_re().is_match(text)
        || get_expl_re().is_match(text)
}

/// What the structure pass concluded about a line's role, when one ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteClass {
    /// No structure pass contributed to this document, so nothing is known and
    /// text patterns are the only available signal.
    Unknown,
    /// The structure pass classified the line as ordinary body material.
    Body,
    /// The structure pass classified the line as note material.
    Note,
}

/// Raw line with page number context.
#[derive(Debug, Clone)]
pub struct StatutoryLine {
    pub text: String,
    pub page_number: u32,
    /// The structure pass's verdict on this line. A `Note` is authoritative; the
    /// other two leave the decision to the text patterns.
    pub note_class: NoteClass,
}

/// The band, as a fraction of page height, inside which a repeated line is page furniture.
///
/// The structure pass marks furniture at 0.12 (top) and 0.90 (bottom). These fractions are
/// wider because a consolidated gazette prints its running head and folio just inside those,
/// but the top band must stay below a division heading: measured across the 155-page CGST Act
/// the running head ends at `bbox[3] = 0.1435h` while the `SCHEDULE II` heading on the next
/// page ends at `0.1730h`, and at 0.16 both were classified as furniture and the schedule was
/// lost. 0.155 sits between them; the folio band has the same margin, the folio starting at
/// `bbox[1] = 0.8519h` against the nearest body line above it at `0.8276h`.
const RUNNING_HEAD_BAND_TOP_FRAC: f64 = 0.155;
const RUNNING_HEAD_BAND_BOTTOM_FRAC: f64 = 0.84;

/// Pages a line must appear on before position makes it furniture. A gazette prints its
/// running head on every page and its folio on every page, so a low bar is safe here where
/// it would not be in the shared pass, which must also clear unrelated corpora.
const RUNNING_HEAD_MIN_PAGES: usize = 4;

/// A line in the top or bottom band whose text repeats across enough pages is a running head
/// or folio rather than enacted text. `pages` is the whole document so repetition is counted
/// over the document rather than the page.
fn is_repeated_edge_furniture(line: &Line, page: &Page, pages: &[Page]) -> bool {
    if page.width <= 0.0 || page.height <= 0.0 {
        return false;
    }
    let top = line.bbox[3] < page.height * RUNNING_HEAD_BAND_TOP_FRAC;
    let bottom = line.bbox[1] > page.height * RUNNING_HEAD_BAND_BOTTOM_FRAC;
    if !top && !bottom {
        return false;
    }
    let key = furniture_key(&line.text);
    if key.is_empty() {
        return false;
    }
    // The page itself always matches, so a line unique to this page counts once and is
    // dropped only if the document is long enough for repetition to mean anything.
    let seen = pages
        .iter()
        .filter(|candidate| {
            candidate
                .lines
                .iter()
                .any(|other| furniture_key(&other.text) == key)
        })
        .count();
    seen >= RUNNING_HEAD_MIN_PAGES
}

/// The comparison key for a running head: case-folded and stripped of spacing and the
/// separator glyphs a printer varies between pages, so `Sec 49-53A` and `Sec. 49-53A` are
/// one string.
fn furniture_key(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Helper to convert PDF pages into a sequence of statutory lines.
pub fn pages_to_statutory_lines(pages: &[Page]) -> Vec<StatutoryLine> {
    // The character map is chosen once per document, so the quotation-glyph decision has to
    // be made once over the whole document rather than per line.
    let broken_font = {
        let document: String = pages
            .iter()
            .flat_map(|page| page.lines.iter())
            .map(|line| line.text.as_str())
            .collect();
        super::normalize::has_broken_quotation_font(&document)
    };

    let mut lines = Vec::new();
    for page in pages {
        // Vector order is not reading order; the crate's established key is
        // `(reading_order, source_index)`.
        let mut ordered: Vec<&Line> = page.lines.iter().collect();
        ordered.sort_by_key(|line| (line.reading_order, line.source_index));
        for line in ordered {
            // `exclude_from_body` carries what the structure pass suppresses structurally;
            // `region_type` is how it marks page furniture. Both are set only by that pass,
            // and a document that never ran one carries `"unknown"` here, so this filter
            // rejects nothing without it.
            if line.exclude_from_body || matches!(line.region_type.as_str(), "header" | "footer") {
                continue;
            }
            // A running head that the structure pass scored as body still has to leave the
            // statute. The shared geometry bands are 0.12/0.90 of the page, and a
            // consolidated gazette sets its running head and folio just inside them — the
            // CGST Act prints `Ch-I : Preliminary` at `bbox[3] ≈ 0.1435h` and
            // `GST & Indirect Taxes Committee` at `bbox[1] ≈ 0.8519h` — so no amount of
            // repetition marks them furniture and they were appended to the last provision
            // of each page. Widen the shared bands to reach them and every other profile's
            // furniture changes with them, so the line is recognised here instead, from its
            // position on the page, and only when it repeats: a running head or folio does,
            // and a first-line-of-body that happens to sit high on its page does not.
            if is_repeated_edge_furniture(line, page, pages) {
                continue;
            }
            let trimmed = line.text.trim();
            if !trimmed.is_empty() {
                let note_class = note_class_for(line);
                // A superscript digit inside a note block is that note's own label, not a
                // reference to another one, so bracketing it there would hide the number the
                // separator has to read.
                let text = if note_class == NoteClass::Note {
                    trimmed.to_owned()
                } else {
                    mark_note_references(line)
                };
                lines.push(StatutoryLine {
                    text: super::normalize::rewrite_quotation_glyphs(&text, broken_font)
                        .into_owned(),
                    page_number: page.number,
                    note_class,
                });
            }
        }
    }
    lines
}

/// Helper to convert plain string into statutory lines.
pub fn text_to_statutory_lines(text: &str) -> Vec<StatutoryLine> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| StatutoryLine {
            text: line.to_owned(),
            page_number: 1,
            note_class: NoteClass::Unknown,
        })
        .collect()
}

/// Main parser that transforms lines into an `IndianStatuteDocument`.
pub fn parse_indian_statute_lines(
    lines: &[StatutoryLine],
    doc_title: Option<&str>,
) -> IndianStatuteDocument {
    let mut body_lines: Vec<StatutoryLine> = Vec::new();
    let mut footnote_items: Vec<FootnoteItem> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // 1. Separate footnote lines from body lines, grouping a wrapped note before parsing it.
    //
    // Where a structure pass ran it has already decided which lines are notes, using
    // separator geometry and label geometry that no text pattern can recover. Its verdict
    // wins. The pattern is kept for documents that never had one, where it is the only
    // signal available.
    let fn_re = get_footnote_line_re();
    let mut pending: Option<PendingFootnote> = None;
    for line in lines {
        let labelled = fn_re
            .captures(&line.text)
            .and_then(|caps| caps.name("num").map(|m| m.as_str().to_owned()));
        let is_tail = get_note_tail_re().is_match(&line.text);
        let is_note = line.note_class == NoteClass::Note;
        let is_body = line.note_class == NoteClass::Body;

        // Whether this line opens a note rather than continuing one. Gazette note blocks
        // wrap, and only the first line carries the label, so an unlabelled note line while
        // a note is open is the wrapped remainder of that note, not a new one.
        let opens_note = if is_note {
            pending.is_none() || labelled.is_some()
        } else {
            // With no structural verdict, a label is the only signal available.
            labelled.is_some()
        };

        if opens_note {
            flush_pending_footnote(&mut pending, &mut footnote_items);
            // A structurally detected note need not carry its own label, so fall back to a
            // bare leading integer and finally to an anonymous note rather than inventing
            // a number.
            let number = labelled.unwrap_or_else(|| leading_note_number(&line.text).to_owned());
            pending = Some(PendingFootnote {
                number,
                lines: vec![line.clone()],
            });
            continue;
        }

        if let Some(note) = pending.as_mut() {
            // A structural verdict outranks the continuation heuristic: a line the structure
            // pass called body ends the note even if it opens no provision. Without a verdict,
            // absorb until something that clearly starts a provision. An enforcement tail is
            // note material whatever the verdict was — it is the tail of the note that is
            // open, not the body of a provision, and a `body` verdict for it is a geometry
            // miss on a note that straddles a page or a column.
            // A division heading is enacted structure, so it can never be note material however the
            // structure pass classified its region. The CGST Act prints `SCHEDULE II` where a
            // small-type centred line reads like a note tail, and it was being absorbed into
            // the footnote above rather than reaching the loop that mints schedules — which
            // left one schedule holding every schedule's paragraphs.
            let opens_a_division =
                section_start(&line.text).is_none() && get_chapter_marker_re().is_match(&line.text);
            if !opens_a_division
                && (is_note || is_tail || !(is_body || starts_a_provision(&line.text)))
            {
                note.lines.push(line.clone());
                continue;
            }
            flush_pending_footnote(&mut pending, &mut footnote_items);
        }

        // A tail that arrives with no note open belongs to the note whose label line was
        // separated and then closed, which is the common shape when the label sits on one
        // page and the tail on the next. It must not be spliced into whichever provision
        // happens to be open. With no such note the text is still kept, but reported rather
        // than presented as statutory text.
        if is_tail {
            if let Some(last) = footnote_items.last_mut() {
                if last.page_number == Some(line.page_number) {
                    last.raw_text.push(' ');
                    last.raw_text.push_str(&line.text);
                    continue;
                }
            }
            diagnostics.push(Diagnostic::warning(
                "statute.note_tail_unattached",
                format!("note tail matched no open note: {}", line.text),
                None,
            ));
        }

        body_lines.push(line.clone());
    }
    flush_pending_footnote(&mut pending, &mut footnote_items);

    // 2. Build hierarchical provision tree
    let mut root_provisions: Vec<ProvisionNode> = Vec::new();
    let mut preamble: Vec<StatutoryLine> = Vec::new();
    let mut current_section: Option<ProvisionNode> = None;
    let mut current_subsec: Option<ProvisionNode> = None;
    let mut current_clause: Option<ProvisionNode> = None;
    let mut proviso_counters: HashMap<String, usize> = HashMap::new();
    let mut expl_counters: HashMap<String, usize> = HashMap::new();
    let mut eid_counts: HashMap<String, usize> = HashMap::new();
    let mut clause_ordinal: usize = 0;
    let mut chapter_counter: usize = 1;
    let mut schedule_counter: usize = 1;
    let mut inside_schedule: bool = false;
    let mut last_schedule_item: usize = 0;
    let mut awaiting_chapter_title = false;

    // Helper to flush current clause into its nearest container
    fn flush_clause(
        clause: &mut Option<ProvisionNode>,
        subsec: &mut Option<ProvisionNode>,
        sec: &mut Option<ProvisionNode>,
        roots: &mut Vec<ProvisionNode>,
    ) {
        if let Some(c) = clause.take() {
            if let Some(s) = subsec {
                s.children.push(c);
            } else if let Some(s) = sec {
                s.children.push(c);
            } else {
                roots.push(reparent_to_root(c));
            }
        }
    }

    // Helper to flush current subsection into section
    fn flush_subsec(
        subsec: &mut Option<ProvisionNode>,
        clause: &mut Option<ProvisionNode>,
        sec: &mut Option<ProvisionNode>,
        roots: &mut Vec<ProvisionNode>,
    ) {
        flush_clause(clause, subsec, sec, roots);
        if let Some(s) = subsec.take() {
            if let Some(sc) = sec {
                sc.children.push(s);
            } else {
                roots.push(reparent_to_root(s));
            }
        }
    }

    // Helper to flush section into root
    fn flush_section(
        sec: &mut Option<ProvisionNode>,
        subsec: &mut Option<ProvisionNode>,
        clause: &mut Option<ProvisionNode>,
        roots: &mut Vec<ProvisionNode>,
    ) {
        flush_subsec(subsec, clause, sec, roots);
        if let Some(s) = sec.take() {
            // A schedule item is held open in the section slot so that the whole
            // sub-section / clause / sub-clause machinery parents to it unchanged. It is not
            // a root, though: it belongs to its schedule, and every provision it collected on
            // the way has to arrive with it. Routing it to `roots` is what left 410 orphan
            // clauses holding 99,374 characters of schedule text outside the schedule that
            // owns them.
            if s.kind == ProvisionKind::ScheduleItem {
                if let Some(parent) = s.parent_eid.clone() {
                    if let Some(node) = roots.iter_mut().find(|n| n.eid == parent) {
                        node.children.push(s);
                        return;
                    }
                }
            }
            roots.push(s);
        }
    }

    for (index, line) in body_lines.iter().enumerate() {
        let text = &line.text;

        // Check if new Section begins
        if let Some((sec_num, rest)) = section_start(text) {
            let rest = rest.to_owned();
            let rest = rest.as_str();

            // Inside a schedule the same printed shape is a numbered paragraph, not a
            // section: the schedule restarts at 1, so the duplicate guard below would
            // otherwise reject every one of them and keep it as continuation text.
            //
            // A schedule is the terminal division of an Act, but a document may still
            // resume its sections after one. A schedule's own paragraphs ascend from 1, so a
            // numeral outside that run is the body resuming and closes the schedule.
            let schedule_ordinal = sec_num.parse::<usize>().ok();
            let continues_schedule = schedule_ordinal.is_some_and(|n| n <= last_schedule_item + 1);
            // A schedule heading is not one of the schedule's own paragraphs. Inside a
            // schedule the numbered-paragraph branch below would otherwise claim it first —
            // the section arm runs ahead of the chapter arm — and `SCHEDULE II` was absorbed
            // as paragraph 1, which is why the CGST Act yielded one schedule holding every
            // schedule's paragraphs instead of three schedules. A heading is identified by
            // the same rule that mints one: the division-marker pattern.
            let opens_a_new_division =
                schedule_ordinal.is_none() && get_chapter_marker_re().is_match(text);
            if inside_schedule && continues_schedule && !opens_a_new_division {
                let parent = eid::schedule_eid(schedule_counter.saturating_sub(1));
                let item_eid = dedup_eid(
                    &mut eid_counts,
                    &mut diagnostics,
                    "schedule_item",
                    line.page_number,
                    eid::schedule_item_eid(&parent, &sec_num),
                );
                // Close the item that was open before opening this one. Nothing else flushes
                // it, because it does not live in the section slot between paragraphs.
                flush_section(
                    &mut current_section,
                    &mut current_subsec,
                    &mut current_clause,
                    &mut root_provisions,
                );
                // Held open in the section slot, not pushed to `root_provisions`. The item is
                // a container, and a container that is pushed and forgotten cannot hold
                // anything: the next line had nowhere to go, so its continuation text was
                // dropped and its `(a)`, `(b)`, `(i)` markers escaped to the document root as
                // `sec_unknown` clauses. Holding it here means `flush_clause` parents to it
                // for free, because it already falls back to `sec.children`.
                current_section = Some(ProvisionNode {
                    eid: item_eid,
                    kind: ProvisionKind::ScheduleItem,
                    num: sec_num,
                    heading: None,
                    text: rest.to_owned(),
                    parent_eid: Some(parent),
                    children: Vec::new(),
                    citations: Vec::new(),
                    amendments: Vec::new(),
                    page_numbers: vec![line.page_number],
                });
                if let Some(ordinal) = schedule_ordinal {
                    last_schedule_item = ordinal;
                }
                clause_ordinal = 0;
                continue;
            }
            inside_schedule = false;

            let eid = eid::section_eid(&sec_num);

            // `ast.rs` promises a unique eId. A repeated section number cannot mint a second
            // one, so the line is kept as continuation text and reported.
            // The stub case: a line that minted this eId but carries neither text nor
            // children is not a real provision — it is a fragment the boundary rules
            // split off (an orphaned `143.` above the first `Section 143.` line, a
            // marginal note with no body). Replacing it lets the genuine section keep
            // the eId instead of being demoted to continuation text.
            let existing_stub = root_provisions
                .iter()
                .position(|n| n.eid == eid && n.text.trim().is_empty() && n.children.is_empty())
                .or_else(|| {
                    current_section.as_ref().and_then(|n| {
                        (n.eid == eid && n.text.trim().is_empty() && n.children.is_empty())
                            .then_some(usize::MAX)
                    })
                });
            let duplicate = existing_stub.is_none()
                && (root_provisions.iter().any(|n| n.eid == eid)
                    || current_section.as_ref().is_some_and(|n| n.eid == eid));
            if let Some(index) = existing_stub {
                if index != usize::MAX {
                    root_provisions.remove(index);
                    diagnostics.push(Diagnostic::warning(
                        "statute.empty_section_stub_replaced",
                        format!(
                            "section {sec_num} replaces an empty stub minted from an orphan line"
                        ),
                        None,
                    ));
                }
            }
            if duplicate {
                diagnostics.push(Diagnostic::warning(
                    "statute.duplicate_section_number",
                    format!("section {sec_num} repeats an existing eId; kept as continuation text"),
                    None,
                ));
                append_continuation_line(
                    &mut current_clause,
                    &mut current_subsec,
                    &mut current_section,
                    &mut root_provisions,
                    &mut preamble,
                    &mut diagnostics,
                    text,
                    line.page_number,
                );
                continue;
            }

            flush_section(
                &mut current_section,
                &mut current_subsec,
                &mut current_clause,
                &mut root_provisions,
            );
            clause_ordinal = 0;
            awaiting_chapter_title = false;

            // Choose the separator by position, not by candidate order: `:` occurs in
            // ordinary prose long before any marginal-note colon. Ties resolve to the earlier
            // candidate in this list, i.e. to the more specific separator — `.—` and `—`
            // share an index in "…tax credit.—(1)…" and `.—` must win.
            let sep_candidates: [(&str, bool); 4] =
                [(".—", false), (".-", false), ("—", false), (":", true)];
            let mut chosen: Option<(usize, usize)> = None;
            for (sep, needs_uppercase) in sep_candidates {
                let mut from = 0;
                while let Some(rel) = rest[from..].find(sep) {
                    let idx = from + rel;
                    let after = idx + sep.len();
                    // The colon rule looks past the space a printer puts after it, so
                    // "Power to make rules: The Government" is a separator and
                    // "Power to make rules: the Government" is prose.
                    let usable = !needs_uppercase
                        || rest[after..]
                            .trim_start()
                            .chars()
                            .next()
                            .map_or(true, char::is_uppercase);
                    if usable && chosen.map_or(true, |(best, _)| idx < best) {
                        chosen = Some((idx, sep.len()));
                    }
                    from = after.max(idx + 1);
                }
            }
            let (heading, body_text) = if let Some((idx, sep_len)) = chosen {
                (
                    Some(rest[..idx].trim().to_owned()),
                    rest[idx + sep_len..].trim().to_owned(),
                )
            } else if rest.starts_with('(') {
                (None, rest.to_owned())
            } else if looks_like_marginal_note(rest) {
                (Some(rest.to_owned()), String::new())
            } else if !rest.is_empty() {
                (None, rest.to_owned())
            } else {
                (None, String::new())
            };

            let mut sec_node = ProvisionNode {
                eid: eid.clone(),
                kind: ProvisionKind::Section,
                num: sec_num,
                heading,
                text: body_text.clone(),
                parent_eid: None,
                children: Vec::new(),
                citations: Vec::new(),
                amendments: Vec::new(),
                page_numbers: vec![line.page_number],
            };

            // If the section line also carried subsection text (e.g. "(1) Every registered person...")
            if let Some(sub_caps) = get_subsec_re().captures(&body_text) {
                let sub_num = sub_caps.name("subsec").unwrap().as_str().to_owned();
                let sub_rest = sub_caps
                    .name("rest")
                    .map(|m| m.as_str().trim())
                    .unwrap_or("");
                let sub_eid = dedup_eid(
                    &mut eid_counts,
                    &mut diagnostics,
                    "subsection",
                    line.page_number,
                    eid::subsection_eid(&eid, &sub_num),
                );
                current_subsec = Some(ProvisionNode {
                    eid: sub_eid,
                    kind: ProvisionKind::Subsection,
                    num: format!("({sub_num})"),
                    heading: None,
                    text: sub_rest.to_owned(),
                    parent_eid: Some(eid.clone()),
                    children: Vec::new(),
                    citations: Vec::new(),
                    amendments: Vec::new(),
                    page_numbers: vec![line.page_number],
                });

                // `body_text` is the whole remainder, but `(1) …` on the section line is
                // minted as a child sub-section. Leaving it on the section as well prints
                // the same sentence twice and extracts its citations twice. The heading was
                // never scanned for citations either way, so nothing is lost: a
                // `CitationReference.span` must index `parent.text`.
                sec_node.text = String::new();
            }

            current_section = Some(sec_node);
            continue;
        }

        // Check if Subsection begins: e.g. "(1) ..."
        if let Some(caps) = get_subsec_re().captures(text) {
            flush_subsec(
                &mut current_subsec,
                &mut current_clause,
                &mut current_section,
                &mut root_provisions,
            );
            clause_ordinal = 0;

            let sub_num = caps.name("subsec").unwrap().as_str().to_owned();
            let sub_rest = caps.name("rest").map(|m| m.as_str().trim()).unwrap_or("");
            let parent_eid = current_section
                .as_ref()
                .map(|s| s.eid.clone())
                .unwrap_or_else(|| "sec_unknown".to_owned());
            let eid = dedup_eid(
                &mut eid_counts,
                &mut diagnostics,
                "subsection",
                line.page_number,
                eid::subsection_eid(&parent_eid, &sub_num),
            );

            current_subsec = Some(ProvisionNode {
                eid,
                kind: ProvisionKind::Subsection,
                num: format!("({sub_num})"),
                heading: None,
                text: sub_rest.to_owned(),
                parent_eid: Some(parent_eid),
                children: Vec::new(),
                citations: Vec::new(),
                amendments: Vec::new(),
                page_numbers: vec![line.page_number],
            });
            continue;
        }

        // INVARIANT: a token of three or more letters that is pure roman is a sub-clause,
        // because no clause label in this grammar has three letters. A token of one or two
        // letters that is pure roman is genuinely ambiguous — `(i)` is both the ninth clause
        // and the first sub-clause — and is resolved by the clause-alphabet sequence: it is a
        // sub-clause exactly when a clause is open and the sequence does not expect it.
        // `current_clause` alone cannot decide this: it is still `Some` when the next `(x)`
        // arrives, because nothing closes it until the arm that flushes it runs, so an "is a
        // clause open" guard misreads the ninth clause as a sub-clause of the eighth.
        if let Some(token) = leading_paren_token(text) {
            let roman = is_roman_token(token);
            let long_roman = token.len() >= 3;
            let as_sub_clause = if !roman {
                false
            } else if long_roman {
                true
            } else {
                current_clause.is_some() && token != expected_clause_label(clause_ordinal)
            };

            if as_sub_clause {
                let caps = get_subclause_re().captures(text).unwrap_or_else(|| {
                    unreachable!("a roman token of the accepted shape always matches")
                });
                let subcl_str = caps.name("subcl").unwrap().as_str().to_owned();
                let subcl_rest = caps.name("rest").map(|m| m.as_str().trim()).unwrap_or("");
                let parent_eid = current_clause
                    .as_ref()
                    .map(|c| c.eid.clone())
                    .or_else(|| current_subsec.as_ref().map(|s| s.eid.clone()))
                    .or_else(|| current_section.as_ref().map(|s| s.eid.clone()))
                    .unwrap_or_else(|| "sec_unknown".to_owned());
                let eid = dedup_eid(
                    &mut eid_counts,
                    &mut diagnostics,
                    "subclause",
                    line.page_number,
                    eid::subclause_eid(&parent_eid, &subcl_str),
                );

                let subcl_node = ProvisionNode {
                    eid,
                    kind: ProvisionKind::Subclause,
                    num: format!("({subcl_str})"),
                    heading: None,
                    text: subcl_rest.to_owned(),
                    parent_eid: Some(parent_eid),
                    children: Vec::new(),
                    citations: Vec::new(),
                    amendments: Vec::new(),
                    page_numbers: vec![line.page_number],
                };

                if let Some(c) = &mut current_clause {
                    c.children.push(subcl_node);
                } else if let Some(s) = &mut current_subsec {
                    s.children.push(subcl_node);
                } else if let Some(sec) = &mut current_section {
                    sec.children.push(subcl_node);
                } else {
                    root_provisions.push(reparent_to_root(subcl_node));
                }
            } else if let Some(caps) = get_clause_re().captures(text) {
                flush_clause(
                    &mut current_clause,
                    &mut current_subsec,
                    &mut current_section,
                    &mut root_provisions,
                );

                let cl_str = caps.name("cl").unwrap().as_str().to_owned();
                let cl_rest = caps.name("rest").map(|m| m.as_str().trim()).unwrap_or("");
                let parent_eid = current_subsec
                    .as_ref()
                    .map(|s| s.eid.clone())
                    .or_else(|| current_section.as_ref().map(|s| s.eid.clone()))
                    .unwrap_or_else(|| "sec_unknown".to_owned());
                let eid = dedup_eid(
                    &mut eid_counts,
                    &mut diagnostics,
                    "clause",
                    line.page_number,
                    eid::clause_eid(&parent_eid, &cl_str),
                );

                current_clause = Some(ProvisionNode {
                    eid,
                    kind: ProvisionKind::Clause,
                    num: format!("({cl_str})"),
                    heading: None,
                    text: cl_rest.to_owned(),
                    parent_eid: Some(parent_eid),
                    children: Vec::new(),
                    citations: Vec::new(),
                    amendments: Vec::new(),
                    page_numbers: vec![line.page_number],
                });
                // `expected_clause_label` walks the lower-case alphabet, so an upper-case
                // designator must not consume a slot in it: after `(a)`, `(A)`, `(i)` must
                // still read `(i)` as the first sub-clause rather than the ninth clause.
                // `num` keeps the printed form, `clause_eid` lower-cases it, and `dedup_eid`
                // keeps `(a)` and `(A)` distinct when both occur.
                if !cl_str.chars().any(char::is_uppercase) {
                    clause_ordinal += 1;
                }
            } else {
                // A leading `( … )` whose contents are not a designator — "(the words "ten
                // per cent" in section 5 thereof)", "(No. 31 of 2018)", "(i.e. the declared
                // rate)" — is prose, not a lost provision. This path used to reach the
                // unconditional `continue` below and drop the line with no node, no text and
                // no diagnostic.
                append_continuation_line(
                    &mut current_clause,
                    &mut current_subsec,
                    &mut current_section,
                    &mut root_provisions,
                    &mut preamble,
                    &mut diagnostics,
                    text,
                    line.page_number,
                );
            }
            // A classified `(…)` line must stay away from the proviso, explanation, chapter
            // and title arms below, so the `continue` remains.
            continue;
        }

        // Check if Proviso begins: e.g. "Provided that..."
        if let Some(caps) = get_proviso_re().captures(text) {
            flush_clause(
                &mut current_clause,
                &mut current_subsec,
                &mut current_section,
                &mut root_provisions,
            );

            let lead = caps.name("lead").unwrap().as_str().trim().to_owned();
            let rest = caps.name("rest").map(|m| m.as_str().trim()).unwrap_or("");
            let parent_eid = current_subsec
                .as_ref()
                .map(|s| s.eid.clone())
                .or_else(|| current_section.as_ref().map(|s| s.eid.clone()))
                .unwrap_or_else(|| "sec_unknown".to_owned());

            let count = proviso_counters.entry(parent_eid.clone()).or_insert(0);
            *count += 1;
            let eid = eid::proviso_eid(&parent_eid, *count);

            let prov_node = ProvisionNode {
                eid,
                kind: ProvisionKind::Proviso,
                num: lead,
                heading: None,
                text: rest.to_owned(),
                parent_eid: Some(parent_eid),
                children: Vec::new(),
                citations: Vec::new(),
                amendments: Vec::new(),
                page_numbers: vec![line.page_number],
            };

            if let Some(s) = &mut current_subsec {
                s.children.push(prov_node);
            } else if let Some(sec) = &mut current_section {
                sec.children.push(prov_node);
            } else {
                root_provisions.push(reparent_to_root(prov_node));
            }
            continue;
        }

        // Check if Explanation begins: e.g. "Explanation.—..."
        if let Some(caps) = get_expl_re().captures(text) {
            let num_opt = caps.name("num").map(|m| m.as_str().to_owned());
            let rest = caps.name("rest").map(|m| m.as_str().trim()).unwrap_or("");

            // The parent is resolved BEFORE the flush: the flush takes the open clause out
            // of `current_clause`, so a parentage computed afterwards could never name it.
            let parent_eid = current_clause
                .as_ref()
                .map(|c| c.eid.clone())
                .or_else(|| current_subsec.as_ref().map(|s| s.eid.clone()))
                .or_else(|| current_section.as_ref().map(|s| s.eid.clone()))
                .unwrap_or_else(|| "sec_unknown".to_owned());

            flush_clause(
                &mut current_clause,
                &mut current_subsec,
                &mut current_section,
                &mut root_provisions,
            );

            let eid = if let Some(n) = &num_opt {
                eid::numbered_explanation_eid(&parent_eid, n)
            } else {
                let count = expl_counters.entry(parent_eid.clone()).or_insert(0);
                *count += 1;
                if *count == 1 {
                    eid::unnumbered_explanation_eid(&parent_eid, 1)
                } else {
                    eid::unnumbered_explanation_eid(&parent_eid, *count)
                }
            };

            let expl_node = ProvisionNode {
                eid,
                kind: ProvisionKind::Explanation,
                num: num_opt
                    .map(|n| format!("Explanation {n}"))
                    .unwrap_or_else(|| "Explanation".to_owned()),
                heading: None,
                text: rest.to_owned(),
                parent_eid: Some(parent_eid),
                children: Vec::new(),
                citations: Vec::new(),
                amendments: Vec::new(),
                page_numbers: vec![line.page_number],
            };

            if let Some(c) = &mut current_clause {
                c.children.push(expl_node);
            } else if let Some(s) = &mut current_subsec {
                s.children.push(expl_node);
            } else if let Some(sec) = &mut current_section {
                sec.children.push(expl_node);
            } else {
                root_provisions.push(reparent_to_root(expl_node));
            }
            continue;
        }

        // A chapter or part heading is a boundary. Appending it corrupts the preceding
        // provision's `text` and feeds chapter prose to extract_citations and to
        // detect_relation_type. The heading is emitted as its own root rather than dropped,
        // and sections are NOT nested under it, so no existing eId changes.
        //
        // A line that opens a section outranks a division heading. A consolidated gazette
        // prints the chapter designation and the section heading on consecutive lines —
        // `CHAPTER XXI : MISCELLANEOUS` then `Section 143. Job work procedure.-` — and the
        // chapter arm runs before the section arm, so without this guard the section line
        // never reaches `section_start`: it is taken as the chapter title or appended to
        // the chapter, and every sub-section of that section is then credited to
        // `sec_unknown`.
        // A division heading stands alone. A gazette wraps body text, and a wrapped
        // continuation can begin with the word without being one: "as per Chapter XIX-C;"
        // is the tail of a sentence in section 95. Such a line carries sentence
        // punctuation at its end, which a real heading never does — `CHAPTER I` and
        // `CHAPTER II : RATES` both stop at the designator or the title. So the
        // terminator, not the leading word, is the discriminator, and without it the
        // match closes the open provision and orphans everything after it.
        //
        // The terminator is not enough on its own. When the column runs out immediately
        // after the connector, the designator lands alone on its own line — `computed as
        // per` then `Chapter IV-D` then `as increased by the aggregate amount` — and no test
        // on that line alone can tell it from a heading, because it is character for
        // character a heading. Only the unfinished sentence behind it gives it away, which
        // is why the paragraph is consulted rather than just the line.
        let carries_over = index
            .checked_sub(1)
            .and_then(|previous| body_lines.get(previous))
            .is_some_and(|previous| sentence_carries_over(&previous.text));
        if get_chapter_marker_re().is_match(text)
            && !text.trim_end().ends_with(['.', ';', ',', ':'])
            && !carries_over
        {
            flush_section(
                &mut current_section,
                &mut current_subsec,
                &mut current_clause,
                &mut root_provisions,
            );
            let designator = text
                .split_once(char::is_whitespace)
                .map(|(_, rest)| rest.trim().to_owned())
                .filter(|rest| !rest.is_empty())
                .unwrap_or_else(|| text.trim().to_owned());
            // A schedule is a division in its own right, not a chapter: its paragraphs
            // restart at 1, so they collide with every section numeral and the duplicate
            // guard was rejecting each one and keeping it as continuation text. Opening a
            // real container is what lets a numbered paragraph become a node.
            //
            // A designator is required. `SCHEDULES` with nothing after it is the running
            // head this Act prints on every schedule page, and minting a schedule from it
            // would invent one per page.
            let upper = text.trim_start().to_ascii_uppercase();
            let after_schedule = upper
                .strip_prefix("SCHEDULES")
                .or_else(|| upper.strip_prefix("SCHEDULE"))
                .map(str::trim)
                .unwrap_or("")
                .to_owned();
            let is_schedule = !after_schedule.is_empty();
            let node = ProvisionNode {
                eid: if is_schedule {
                    eid::schedule_eid(schedule_counter)
                } else {
                    format!("chapter_{chapter_counter}")
                },
                kind: if is_schedule {
                    ProvisionKind::Schedule
                } else {
                    ProvisionKind::Chapter
                },
                num: designator,
                heading: None,
                text: String::new(),
                parent_eid: None,
                children: Vec::new(),
                citations: Vec::new(),
                amendments: Vec::new(),
                page_numbers: vec![line.page_number],
            };
            if is_schedule {
                // A previous schedule, if any, is already in `root_provisions` through the
                // flush chain, so nothing is held open here and `current_schedule` stays a
                // marker for "inside a schedule" rather than a node to be flushed.
                root_provisions.push(node);
                schedule_counter += 1;
                inside_schedule = true;
                last_schedule_item = 0;
            } else {
                inside_schedule = false;
                root_provisions.push(node);
                chapter_counter += 1;
                awaiting_chapter_title = true;
            }
            continue;
        }

        if awaiting_chapter_title {
            awaiting_chapter_title = false;
            if let Some(chapter) = root_provisions.last_mut() {
                // A section heading is not a chapter title, however title-shaped it
                // looks. `Sec 49-53A` matches the title pattern but belongs to the
                // running head, and a bare `Section 143. Job work procedure.-` opens a
                // section. Only take the line as a title when it starts neither.
                if section_start(text).is_none() && get_chapter_title_re().is_match(text) {
                    chapter.heading = Some(text.to_owned());
                    if !chapter.page_numbers.contains(&line.page_number) {
                        chapter.page_numbers.push(line.page_number);
                    }
                    continue;
                }
            }
            // otherwise fall through and process the line normally
        }

        // Continuation line: one append path for every "no structural construct matched" case.
        append_continuation_line(
            &mut current_clause,
            &mut current_subsec,
            &mut current_section,
            &mut root_provisions,
            &mut preamble,
            &mut diagnostics,
            text,
            line.page_number,
        );
    }

    // Flush any remaining active nodes
    flush_section(
        &mut current_section,
        &mut current_subsec,
        &mut current_clause,
        &mut root_provisions,
    );

    // Front matter recovered from the preamble is emitted as one synthetic root at index 0
    // rather than being lost.
    if !preamble.is_empty() {
        let mut pages: Vec<u32> = preamble.iter().map(|line| line.page_number).collect();
        pages.sort_unstable();
        pages.dedup();
        diagnostics.push(Diagnostic::warning(
            "statute.preamble_recovered",
            format!(
                "{} pre-section line(s) recovered into act_preamble",
                preamble.len()
            ),
            None,
        ));
        root_provisions.insert(
            0,
            ProvisionNode {
                eid: "act_preamble".to_owned(),
                // A front-matter grouping node. `Chapter` is the existing non-section
                // container kind; a new variant would add a wire value to a schema this
                // branch is introducing, for no parsing gain.
                kind: ProvisionKind::Chapter,
                num: "PREAMBLE".to_owned(),
                heading: None,
                text: preamble
                    .iter()
                    .map(|line| line.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                parent_eid: None,
                children: Vec::new(),
                citations: Vec::new(),
                amendments: Vec::new(),
                page_numbers: pages,
            },
        );
    }

    // 3. Choose one owning node per footnote, then attach only there, so `associated_eid`
    // and the per-node `amendments` agree by construction. This runs BEFORE the flat
    // snapshot below: the snapshot clones the tree, so an attachment made after it would
    // reach the live tree and never `provisions`.
    let hosts = choose_footnote_hosts(&root_provisions, &footnote_items);
    for (item, host) in footnote_items.iter_mut().zip(hosts.iter()) {
        item.associated_eid = host.clone();
    }
    attach_footnotes(&mut root_provisions, &footnote_items, &hosts);

    let mut all_citations: Vec<CitationReference> = Vec::new();
    let mut flat_provisions: Vec<ProvisionNode> = Vec::new();

    fn process_node(
        node: &mut ProvisionNode,
        flat_list: &mut Vec<ProvisionNode>,
        all_citations: &mut Vec<CitationReference>,
    ) {
        // Extract citations from node text
        node.citations = extract_citations(&node.eid, &node.text);
        all_citations.extend(node.citations.clone());

        for child in &mut node.children {
            process_node(child, flat_list, all_citations);
        }

        // Snapshot after the recursion, so the flat entry reflects its children's final
        // annotations, and strip `children` so `provisions` is a flat index with one entry
        // per node rather than a second copy of the tree.
        flat_list.push(ProvisionNode {
            children: Vec::new(),
            ..node.clone()
        });
    }

    for root in &mut root_provisions {
        process_node(root, &mut flat_provisions, &mut all_citations);
    }

    // 4. Resolve citations against the provision table
    let table = build_provision_table(&root_provisions);
    resolve_citations(&mut all_citations, &table);
    resolve_provision_tree_citations(&mut root_provisions, &table);
    for p in &mut flat_provisions {
        resolve_citations(&mut p.citations, &table);
    }

    let (recovered_title, act_number, act_year) = root_provisions
        .iter()
        .find(|node| node.eid == "act_preamble")
        .map_or((None, None, None), |preamble| {
            long_title_meta(&preamble.text)
        });

    IndianStatuteDocument {
        schema_version: "akn.statute.v1".to_owned(),
        akoma_ntoso: AkomaNtoso {
            act: AknAct {
                meta: DocumentMeta {
                    // The document's own long title wins; the caller's title is the
                    // fallback; with neither, the neutral fallback inside the parser is
                    // reachable. It was dead while `main.rs` always supplied a file stem.
                    title: recovered_title
                        .clone()
                        .or_else(|| doc_title.map(str::to_owned))
                        .unwrap_or_else(|| "Indian Bare Act".to_owned()),
                    act_number,
                    act_year,
                    date: None,
                    // `source` is an assertion about provenance. Nothing observable in the
                    // parsed text identifies a publisher, so it is omitted rather than
                    // stamped with a hardcoded attribution. `AGENTS.md`: source identity is
                    // a validated fact.
                    source: None,
                },
                body: root_provisions,
            },
        },
        provisions: flat_provisions,
        citations: all_citations,
        footnotes: footnote_items,
        diagnostics,
    }
}

/// Recover the Act title, number and year from the long title of an Indian Bare Act,
/// e.g. "THE CENTRAL GOODS AND SERVICES TAX ACT, 2017". Returns `(None, None, None)`
/// for anything else; nothing is invented.
fn long_title_meta(preamble_text: &str) -> (Option<String>, Option<String>, Option<String>) {
    static LONG_TITLE_RE: OnceLock<Regex> = OnceLock::new();
    static ACT_NO_RE: OnceLock<Regex> = OnceLock::new();
    let title_re = LONG_TITLE_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\bTHE\s+(?<title>[A-Z][A-Za-z0-9 ,.'()\-/&]{6,}?)\s+ACT,?\s*(?<year>(?:19|20)[0-9]{2})\b",
        )
        .unwrap()
    });
    let act_no_re =
        ACT_NO_RE.get_or_init(|| Regex::new(r"\bAct\s+([0-9]+)\s+of\s+([0-9]{4})\b").unwrap());

    // The pattern captures the Act's *name* only; the wire title is the whole long title, and
    // a printed long title is set in capitals, so it is title-cased here and re-joined with
    // "The " and " Act, <year>".
    let matched = title_re.captures(preamble_text).and_then(|caps| {
        Some((
            caps.name("title")?.as_str().to_owned(),
            caps.name("year")?.as_str().to_owned(),
        ))
    });
    let title = matched.as_ref().map(|(name, year)| {
        /// Connectives an Act's name keeps lower-case: "Services and Supply".
        const LOWER_WORDS: [&str; 10] = [
            "and", "of", "the", "in", "on", "for", "to", "at", "by", "with",
        ];
        let mut title_case = String::with_capacity(name.len());
        for (index, word) in name.split_inclusive(char::is_whitespace).enumerate() {
            let bare = word.trim().to_lowercase();
            let trailing = &word[word.trim_end().len()..];
            if index > 0 && LOWER_WORDS.contains(&bare.as_str()) {
                title_case.push_str(&bare);
                title_case.push_str(trailing);
            } else if let Some(first) = word.chars().next() {
                let rest = &word[first.len_utf8()..];
                title_case.push_str(&first.to_uppercase().to_string());
                title_case.push_str(&rest.to_lowercase());
            }
        }
        format!("The {title_case} Act, {year}")
    });
    let act_year = matched.map(|(_, year)| year);
    let act_number = act_no_re.captures(preamble_text).and_then(|caps| {
        Some(format!(
            "Act {} of {}",
            caps.get(1)?.as_str(),
            caps.get(2)?.as_str()
        ))
    });
    (title, act_number, act_year)
}

/// The byte offset of an anchored footnote marker for note `num` in `text`, if any.
///
/// Only the two marker forms a printer uses for a superscript note reference are
/// accepted: `n[` and `[n]`. Both must be anchored on the left, so `1[` cannot sit
/// inside `11[` and `[1]` cannot sit inside `[11]`. A bracketed number with whitespace on
/// BOTH sides is body orthography — "See item [1] of the Schedule." — not a marker,
/// because a superscript marker is set against the preceding word.
fn footnote_marker(text: &str, num: &str) -> Option<usize> {
    let delimited = format!("[{num}]");
    let mut from = 0;
    while let Some(rel) = text[from..].find(&delimited) {
        let at = from + rel;
        let end = at + delimited.len();
        let left_ok = text[..at]
            .chars()
            .next_back()
            .map_or(true, |c| !c.is_ascii_digit() && !c.is_whitespace());
        let right_ok = text[end..]
            .chars()
            .next()
            .map_or(true, |c| !c.is_ascii_digit() && !c.is_whitespace());
        if left_ok && right_ok {
            return Some(at);
        }
        from = at + 1;
    }
    let attached = format!("{num}[");
    let mut from = 0;
    while let Some(rel) = text[from..].find(&attached) {
        let at = from + rel;
        let end = at + attached.len();
        let left_ok = text[..at]
            .chars()
            .next_back()
            .map_or(true, |c| !c.is_ascii_digit());
        let right_ok = text[end..]
            .chars()
            .next()
            .map_or(true, |c| !c.is_ascii_digit());
        if left_ok && right_ok {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// Phase 1: for each footnote, the first node in document order whose text carries an
/// anchored marker. Choosing one owner is what stops the fan-out and makes
/// `associated_eid` and the per-node `amendments` agree by construction.
fn choose_footnote_hosts(
    roots: &[ProvisionNode],
    footnotes: &[FootnoteItem],
) -> Vec<Option<String>> {
    fn walk(nodes: &[ProvisionNode], footnotes: &[FootnoteItem], out: &mut Vec<Option<String>>) {
        for node in nodes {
            for (index, item) in footnotes.iter().enumerate() {
                if out[index].is_none() && footnote_marker(&node.text, &item.number).is_some() {
                    out[index] = Some(node.eid.clone());
                }
            }
            walk(&node.children, footnotes, out);
        }
    }
    let mut out = vec![None; footnotes.len()];
    walk(roots, footnotes, &mut out);
    out
}

/// Phase 2: attach the amendment only to the chosen node.
fn attach_footnotes(
    nodes: &mut [ProvisionNode],
    footnotes: &[FootnoteItem],
    hosts: &[Option<String>],
) {
    for node in nodes {
        let owned = hosts
            .iter()
            .position(|host| host.as_deref() == Some(node.eid.as_str()))
            .and_then(|index| footnotes[index].amendment.clone());
        if let Some(amendment) = owned {
            if !node.amendments.contains(&amendment) {
                node.amendments.push(amendment);
            }
        }
        attach_footnotes(&mut node.children, footnotes, hosts);
    }
}
