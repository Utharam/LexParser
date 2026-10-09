//! The single owner of the Indian statutory eId grammar and of the locator token
//! patterns. The parser (which mints eIds) and the citation passes (which build target
//! eIds) both call this module; no other module spells an eId or a locator token.
//!
//! Invariant 1 — a proviso is numbered per sub-section and never carries a clause
//! component. `parser.rs` keys `proviso_counters` by the sub-section-or-section parent
//! and pushes the node into that parent, so `…__proviso_N` is the only shape that exists.
//! A citation that names a clause ("proviso to clause (b) of sub-section (2) of section
//! 16") matches the whole phrase but builds the eId without the clause.
//!
//! Invariant 2 — an unnumbered explanation targets `…__expl`, which the parser mints only
//! for the first one. The citation pass cannot know how many exist, so "Explanation to
//! section 16" always names the first. Irreducible; pinned by a test so it stays visible
//! rather than silent.
//!
//! The two deliberately *different* numeral patterns are not drift and stay local:
//! `SEC_START_RE` caps the unprefixed numeral at three digits so a bare year cannot mint
//! a section, and `EXTERNAL_ACT_RE` requires exactly four digits for an Act's year.

/// Indian section / sub-section numeral: "16", "16A", "1a".
pub const SECTION_NUMERAL: &str = r"[0-9]+[A-Za-z]*";
/// A clause designator: one or two lower-case letters, "(a)", "(aa)".
pub const CLAUSE_TOKEN: &str = r"[a-z]{1,2}";
/// A sub-clause designator: "(i)", "(ii)", "(viii)". `[ivx]+`, never `[ivxlcdm]+`:
/// `c`, `d` and `m` are ordinary clause letters (see `P5`).
pub const SUBCLAUSE_TOKEN: &str = r"[ivx]+";

pub fn section_eid(num: &str) -> String {
    format!("sec_{}", num.to_lowercase())
}

pub fn subsection_eid(parent: &str, num: &str) -> String {
    format!("{parent}__subsec_{}", num.to_lowercase())
}

pub fn clause_eid(parent: &str, num: &str) -> String {
    format!("{parent}__cl_{}", num.to_lowercase())
}

pub fn subclause_eid(parent: &str, num: &str) -> String {
    format!("{parent}__subcl_{}", num.to_lowercase())
}

pub fn proviso_eid(parent: &str, ordinal: usize) -> String {
    format!("{parent}__proviso_{ordinal}")
}

/// A schedule is a division root, a sibling of `chapter_N`, never a child of a section.
pub fn schedule_eid(ordinal: usize) -> String {
    format!("schedule_{ordinal}")
}

/// A numbered paragraph inside an open schedule. The numeral restarts at 1 for every
/// schedule, which is exactly why a schedule cannot reuse `sec_N`.
pub fn schedule_item_eid(parent: &str, num: &str) -> String {
    format!("{parent}__item_{}", num.to_lowercase())
}

/// Reads the designator a schedule is printed under: arabic, roman or an ordinal word.
/// `SCHEDULE I` and `THE FIRST SCHEDULE` both yield 1.
pub fn parse_schedule_ordinal(word: &str) -> Option<usize> {
    let word = word.trim().trim_end_matches('.').to_ascii_uppercase();
    if let Ok(number) = word.parse::<usize>() {
        return (number >= 1).then_some(number);
    }
    const ROMAN: [(&str, usize); 9] = [
        ("I", 1),
        ("II", 2),
        ("III", 3),
        ("IV", 4),
        ("V", 5),
        ("VI", 6),
        ("VII", 7),
        ("VIII", 8),
        ("IX", 9),
    ];
    if let Some((_, number)) = ROMAN.iter().find(|(r, _)| *r == word) {
        return Some(*number);
    }
    const WORDS: [(&str, usize); 9] = [
        ("FIRST", 1),
        ("SECOND", 2),
        ("THIRD", 3),
        ("FOURTH", 4),
        ("FIFTH", 5),
        ("SIXTH", 6),
        ("SEVENTH", 7),
        ("EIGHTH", 8),
        ("NINTH", 9),
    ];
    WORDS
        .iter()
        .find(|(candidate, _)| *candidate == word)
        .map(|(_, number)| *number)
}

pub fn numbered_explanation_eid(parent: &str, num: &str) -> String {
    format!("{parent}__expl_{num}")
}

pub fn unnumbered_explanation_eid(parent: &str, ordinal: usize) -> String {
    if ordinal == 1 {
        format!("{parent}__expl")
    } else {
        format!("{parent}__expl_{ordinal}")
    }
}

pub fn rule_eid(rule: &str) -> String {
    format!("rule_{}", rule.to_lowercase())
}

pub fn subrule_eid(parent: &str, num: &str) -> String {
    format!("{parent}__subrule_{}", num.to_lowercase())
}

/// A statutory locator resolved to its provision path.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LocatorPath {
    pub section: String,
    pub subsection: Option<String>,
    pub clause: Option<String>,
    pub subclause: Option<String>,
}

/// The one place that turns a locator into an eId. Level order is fixed here:
/// sub-section, then clause, then sub-clause. `P5` decides which of clause / sub-clause a
/// three- or four-level locator names; this function is the other half of that decision.
pub fn provision_eid(path: &LocatorPath) -> String {
    let mut eid = section_eid(&path.section);
    if let Some(s) = &path.subsection {
        eid = subsection_eid(&eid, s);
    }
    if let Some(c) = &path.clause {
        eid = clause_eid(&eid, c);
    }
    if let Some(s) = &path.subclause {
        eid = subclause_eid(&eid, s);
    }
    eid
}
