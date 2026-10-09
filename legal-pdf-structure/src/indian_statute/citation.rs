use super::ast::{CitationReference, ExternalTargetType, RelationType};
use super::eid::{self, LocatorPath};
use regex::Regex;
use std::sync::OnceLock;

static REVERSE_CITE_RE: OnceLock<Regex> = OnceLock::new();
static PROVISO_CITE_RE: OnceLock<Regex> = OnceLock::new();
static EXPL_CITE_RE: OnceLock<Regex> = OnceLock::new();
static FORWARD_CITE_RE: OnceLock<Regex> = OnceLock::new();
static RULE_CITE_RE: OnceLock<Regex> = OnceLock::new();
static EXTERNAL_ACT_RE: OnceLock<Regex> = OnceLock::new();

fn get_reverse_cite_re() -> &'static Regex {
    REVERSE_CITE_RE.get_or_init(|| {
        // Matches reverse citations like:
        // "sub-clause (i) of clause (c) of sub-section (2) of section 17"
        // "clause (b) of sub-section (2) of section 16"
        // "sub-section (1) of section 22"
        // "clause (a) of section 10"
        //
        // `(?!\s*\()` keeps the reverse pass off the `section N` prefix of a *forward*
        // citation ("section 16(2)"), whose locator this pass would otherwise record first
        // and at the narrower width, suppressing the wider forward match.
        // A bare `section N` that is really the prefix of a *forward* citation
        // ("section 16(2)") belongs to pass 5, which records the whole locator. This is the
        // guard the plan writes as `(?!\s*\()`; `regex` has no look-around, so it is applied
        // to the text after the match instead.
        Regex::new(
            &[
                r"(?i)\b(?:sub-clause\s*\((?P<subcl>",
                eid::SUBCLAUSE_TOKEN,
                r")\)\s+of\s+)?",
                r"(?:clause\s*\((?P<cl>",
                eid::CLAUSE_TOKEN,
                r")\)\s+of\s+)?",
                r"(?:sub-section\s*\((?P<subsec>",
                eid::SECTION_NUMERAL,
                r")\)\s+of\s+)?",
                r"section\s+(?P<sec>",
                eid::SECTION_NUMERAL,
                r")\b",
            ]
            .concat(),
        )
        .unwrap()
    })
}

fn get_proviso_cite_re() -> &'static Regex {
    PROVISO_CITE_RE.get_or_init(|| {
        // Matches:
        // "second proviso to sub-section (2) of section 16"
        // "sixth proviso to sub-section (2) of section 16"
        // "twenty-first proviso to section 16"
        // "the proviso to section 16"   -> ord = "the", a determiner: see NON_ORDINAL
        //
        // The ordinal is one word, with hyphen-joined compounds only — no space, so
        // "the first proviso to …" captures "the", not "the first". The separator class
        // includes U+2010 (non-breaking hyphen) and U+2019 so `twenty‑first` and
        // `twenty’first` also parse.
        Regex::new(
            &[
                r"(?i)\b(?:(?P<ord>[a-z]+(?:[-'‑][a-z]+)?)\s+)?proviso\s+to\s+",
                r"(?:clause\s*\((?P<cl>",
                eid::CLAUSE_TOKEN,
                r")\)\s+of\s+)?",
                r"(?:sub-section\s*\((?P<subsec>",
                eid::SECTION_NUMERAL,
                r")\)\s+of\s+)?",
                r"section\s+(?P<sec>",
                eid::SECTION_NUMERAL,
                r")\b",
            ]
            .concat(),
        )
        .unwrap()
    })
}

fn get_expl_cite_re() -> &'static Regex {
    EXPL_CITE_RE.get_or_init(|| {
        // Matches:
        // "Explanation 1 to clause (b) of sub-section (2) of section 17"
        // "Explanation to section 16"
        Regex::new(
            &[
                r"(?i)\bExplanation\s*(?P<expl_num>[0-9]+)?\s+to\s+",
                r"(?:clause\s*\((?P<cl>",
                eid::CLAUSE_TOKEN,
                r")\)\s+of\s+)?",
                r"(?:sub-section\s*\((?P<subsec>",
                eid::SECTION_NUMERAL,
                r")\)\s+of\s+)?",
                r"section\s+(?P<sec>",
                eid::SECTION_NUMERAL,
                r")\b",
            ]
            .concat(),
        )
        .unwrap()
    })
}

fn get_forward_cite_re() -> &'static Regex {
    FORWARD_CITE_RE.get_or_init(|| {
        // Matches:
        // "section 22"
        // "sections 16 and 17"
        // "section 16(2)"
        // "section 16(2)(b)"
        Regex::new(
            &[
                r"(?i)\bsections?\s+(?P<sec>",
                eid::SECTION_NUMERAL,
                r")\s*(?:\((?P<subsec>",
                eid::SECTION_NUMERAL,
                r")\)(?:\s*\((?P<cl>",
                eid::CLAUSE_TOKEN,
                r")\)(?:\s*\((?P<subcl>",
                eid::SUBCLAUSE_TOKEN,
                r")\))?)?|\b)",
            ]
            .concat(),
        )
        .unwrap()
    })
}

fn get_rule_cite_re() -> &'static Regex {
    RULE_CITE_RE.get_or_init(|| {
        // Matches:
        // "sub-rule (2) of rule 37"
        // "rule 36(4)"
        // "rule 42"
        Regex::new(
            &[
                r"(?i)\b(?:sub-rule\s*\((?P<subrule_rev>",
                eid::SECTION_NUMERAL,
                r")\)\s+of\s+)?",
                r"rule\s+(?P<rule>",
                eid::SECTION_NUMERAL,
                r")(?:\s*\((?P<subrule_fwd>",
                eid::SECTION_NUMERAL,
                r")\)|\b)",
            ]
            .concat(),
        )
        .unwrap()
    })
}

fn get_external_act_re() -> &'static Regex {
    EXTERNAL_ACT_RE.get_or_init(|| {
        // Four digits, not `eid::SECTION_NUMERAL`: this is an Act's *year*, and a bare-year
        // guard would be wrong here. `eid.rs` records the two as deliberately different.
        Regex::new(r"(?i)^\s*of\s+the\s+([A-Z][a-zA-Z\s]+Act(?:,\s*[0-9]{4})?)").unwrap()
    })
}

/// The numerals that continue a plural enumeration after the first, each with its byte
/// range in the source text. Bounded to three further items; the range form
/// ("sections 16 to 18") is deliberately out of scope.
fn enumerated_sections(text: &str, after: usize) -> Vec<(String, usize, usize)> {
    static CONT: OnceLock<Regex> = OnceLock::new();
    let re = CONT.get_or_init(|| Regex::new(r"^(?:\s*(?:,|and)\s*)([0-9]+[A-Za-z]*)").unwrap());
    let mut out = Vec::new();
    let mut cursor = after;
    while out.len() < 3 {
        let Some(caps) = re.captures(&text[cursor..]) else {
            break;
        };
        let numeral = caps.get(1).unwrap();
        out.push((
            numeral.as_str().to_owned(),
            cursor + numeral.start(),
            cursor + numeral.end(),
        ));
        cursor += caps.get(0).unwrap().end();
    }
    out
}

/// English ordinals as used in Indian statutes, including hyphenated compounds:
/// "first".."nineteenth", "twentieth", "twenty-first". `None` for anything else, so the
/// caller can abstain rather than guess.
fn parse_ordinal(word: &str) -> Option<usize> {
    const UNITS: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 8] = [
        "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];

    // The ordinals whose stem is not the cardinal plus a suffix.
    const IRREGULAR: [(&str, usize); 7] = [
        ("first", 1),
        ("second", 2),
        ("third", 3),
        ("fifth", 5),
        ("eighth", 8),
        ("ninth", 9),
        ("twelfth", 12),
    ];

    // The whole word is looked up before any suffix is stripped: "first", "second",
    // "third", "fifth", "eighth" and "twelfth" each end in a two-letter string that looks
    // like an ordinal suffix but is part of the stem.
    fn unit_of(word: &str) -> Option<usize> {
        UNITS.iter().position(|unit| *unit == word)
    }
    fn tens_of(word: &str) -> Option<usize> {
        TENS.iter()
            .position(|ten| *ten == word)
            .map(|index| index * 10 + 20)
    }
    fn strip_suffix(word: &str) -> Option<&str> {
        word.strip_suffix("th")
            .or_else(|| word.strip_suffix("st"))
            .or_else(|| word.strip_suffix("nd"))
            .or_else(|| word.strip_suffix("rd"))
    }
    fn lookup(word: &str) -> Option<usize> {
        if let Some((_, value)) = IRREGULAR.iter().find(|(stem, _)| *stem == word) {
            return Some(*value);
        }
        // "zero" is not a statutory ordinal: no proviso is the zeroth.
        unit_of(word)
            .filter(|unit| *unit > 0)
            .or_else(|| tens_of(word))
            // "<ten>ieth": "twentieth" -> "twentie" -> "twenty".
            .or_else(|| {
                word.strip_suffix("ie").and_then(|base| {
                    TENS.iter()
                        .position(|ten| *ten == &format!("{base}y"))
                        .map(|index| index * 10 + 20)
                })
            })
    }

    // Split on the three hyphens a printer may use: ASCII, U+2010, U+2019.
    let lowered = word.to_lowercase();
    let parts: Vec<&str> = lowered
        .split(['-', '\u{2010}', '\u{2019}'])
        .filter(|part| !part.is_empty())
        .collect();

    match parts.as_slice() {
        [single] => lookup(single).or_else(|| strip_suffix(single).and_then(lookup)),
        [tens, unit] => Some(tens_of(tens)? + lookup(unit)?),
        _ => None,
    }
}
/// The start of the clause a citation's relation is decided in: just after the nearest
/// preceding boundary. A boundary is `.` or `;` followed by whitespace or the end of the
/// slice, or a `,`. Requiring whitespace after the `.` is what stops the decimal in
/// "1.5 per cent" from truncating the scope (`A16`).
fn relation_start(text: &str, byte_start: usize) -> usize {
    let head = &text[..byte_start];
    let mut index = head.len();
    while index > 0 {
        let ch = head[..index]
            .chars()
            .next_back()
            .expect("index > 0 implies a character");
        let boundary = ch == ','
            || ((ch == '.' || ch == ';')
                && head[index..]
                    .chars()
                    .next()
                    .map_or(true, char::is_whitespace));
        if boundary {
            return index;
        }
        index -= ch.len_utf8();
    }
    0
}

/// The end of that clause: the nearest following boundary. Boundaries are `,`, `;`, a `.`
/// before whitespace or the end of the text, and the whitespace-delimited connectives
/// `and`, `but`, `or`.
fn relation_end(text: &str, byte_end: usize) -> usize {
    const CONNECTIVES: [&str; 3] = [" and ", " but ", " or "];
    let tail = &text[byte_end..];
    let mut cut = tail.len();
    for (index, ch) in tail.char_indices() {
        if ch == ','
            || ch == ';'
            || (ch == '.'
                && tail[index + 1..]
                    .chars()
                    .next()
                    .map_or(true, char::is_whitespace))
        {
            cut = index;
            break;
        }
    }
    for word in CONNECTIVES {
        if let Some(at) = tail.find(word) {
            cut = cut.min(at);
        }
    }
    byte_end + cut
}

/// Detect the relation from the citation's own clause, resolving competing triggers by
/// proximity rather than by a fixed `else if` order.
fn detect_relation_type(clause: &str) -> RelationType {
    let lower = clause.to_lowercase();
    let notwithstanding = lower.rfind("notwithstanding");
    let subject_to = lower.rfind("subject to");
    match (notwithstanding, subject_to) {
        (Some(n), Some(s)) if s > n => RelationType::SubjectTo,
        (Some(_), _) => RelationType::Notwithstanding,
        (None, Some(_)) => RelationType::SubjectTo,
        (None, None) => RelationType::Reference,
    }
}

/// Convert byte range to character span [start, end].
fn byte_to_char_span(text: &str, byte_start: usize, byte_end: usize) -> [usize; 2] {
    let char_start = text[..byte_start].chars().count();
    let char_count = text[byte_start..byte_end].chars().count();
    [char_start, char_start + char_count]
}

/// Checks if a citation occurrence is immediately qualified by an external Act (e.g. "... of the Integrated Goods and Services Tax Act").
fn check_external_act(text: &str, byte_end: usize) -> Option<(String, usize)> {
    let remainder = &text[byte_end..];
    let re = get_external_act_re();
    if let Some(caps) = re.captures(remainder) {
        let whole_match = caps.get(0).unwrap();
        let act_name = caps.get(1).unwrap().as_str().trim().to_owned();
        return Some((act_name, byte_end + whole_match.end()));
    }
    None
}

/// True when `[start, end)` overlaps any already-recorded range.
fn is_byte_covered(covered: &[(usize, usize)], start: usize, end: usize) -> bool {
    covered.iter().any(|&(s, e)| start < e && s < end)
}

/// True when the text after a reverse citation's match opens a forward locator, i.e. when the
/// match is the bare `section N` prefix of something like "section 16(2)". The reverse pass
/// must not claim those bytes: it records the narrower range first and `is_byte_covered`
/// would then suppress the wider forward match. This is the guard the plan writes as
/// `(?!\s*\()`; the `regex` crate has no look-around, so it is applied here instead.
fn followed_by_locator(text: &str, byte_end: usize) -> bool {
    text[byte_end..].trim_start().starts_with('(')
}

/// Extracts all statutory citations from a block of text, calculating exact character spans,
/// relation types, and normalizing to Akoma Ntoso eId targets.
pub fn extract_citations(parent_eid: &str, text: &str) -> Vec<CitationReference> {
    let mut results = Vec::new();
    let mut covered_bytes = Vec::new();

    // 1. Proviso citations (e.g., "second proviso to sub-section (2) of section 16")
    for caps in get_proviso_cite_re().captures_iter(text) {
        let mat = caps.get(0).unwrap();
        let b_start = mat.start();
        let mut b_end = mat.end();

        // An absent word means "the proviso", i.e. the first. A determiner means the same.
        // Any other word is an ordinal this parser cannot read: abstain, so the bytes fall
        // through and the document gets an honest bare-section link instead of a wrong
        // proviso link.
        const NON_ORDINAL: [&str; 8] = [
            "the",
            "a",
            "an",
            "such",
            "aforesaid",
            "above",
            "following",
            "preceding",
        ];
        let ord_num = match caps.name("ord").map(|m| m.as_str()) {
            None => 1,
            Some(word) => match parse_ordinal(word) {
                Some(n) => n,
                None if NON_ORDINAL.contains(&word.trim().to_ascii_lowercase().as_str()) => 1,
                None => continue,
            },
        };

        // The `cl` group is matched so `raw_text` and `span` cover the whole phrase and the
        // reverse pass stops reporting the clause as a citation in its own right; it is
        // deliberately excluded from the eId (`eid.rs` invariant 1).
        let parent_prefix = {
            let mut prefix = eid::section_eid(caps.name("sec").unwrap().as_str());
            if let Some(subsec) = caps.name("subsec") {
                prefix = eid::subsection_eid(&prefix, subsec.as_str());
            }
            prefix
        };
        let target_eid = eid::proviso_eid(&parent_prefix, ord_num);

        let mut external = false;
        let mut target_type = None;
        if let Some((_act, new_end)) = check_external_act(text, b_end) {
            b_end = new_end;
            external = true;
            target_type = Some(ExternalTargetType::ExternalAct);
        }

        let span = byte_to_char_span(text, b_start, b_end);
        let relation =
            detect_relation_type(&text[relation_start(text, b_start)..relation_end(text, b_end)]);
        covered_bytes.push((b_start, b_end));

        results.push(CitationReference {
            parent_eid: parent_eid.to_owned(),
            raw_text: text[b_start..b_end].to_owned(),
            span,
            target_eid,
            relation,
            resolved: false,
            external,
            target_type,
        });
    }

    // 2. Explanation citations (e.g., "Explanation 1 to clause (b) of sub-section (2) of section 17")
    for caps in get_expl_cite_re().captures_iter(text) {
        let mat = caps.get(0).unwrap();
        let b_start = mat.start();
        let mut b_end = mat.end();
        if is_byte_covered(&covered_bytes, b_start, b_end) {
            continue;
        }

        let mut target_eid = eid::section_eid(caps.name("sec").unwrap().as_str());
        if let Some(subsec) = caps.name("subsec") {
            target_eid = eid::subsection_eid(&target_eid, subsec.as_str());
        }
        if let Some(cl) = caps.name("cl") {
            target_eid = eid::clause_eid(&target_eid, cl.as_str());
        }
        let target_eid = match caps.name("expl_num") {
            Some(num) => eid::numbered_explanation_eid(&target_eid, num.as_str()),
            None => eid::unnumbered_explanation_eid(&target_eid, 1),
        };

        let mut external = false;
        let mut target_type = None;
        if let Some((_act, new_end)) = check_external_act(text, b_end) {
            b_end = new_end;
            external = true;
            target_type = Some(ExternalTargetType::ExternalAct);
        }

        let span = byte_to_char_span(text, b_start, b_end);
        let relation =
            detect_relation_type(&text[relation_start(text, b_start)..relation_end(text, b_end)]);
        covered_bytes.push((b_start, b_end));

        results.push(CitationReference {
            parent_eid: parent_eid.to_owned(),
            raw_text: text[b_start..b_end].to_owned(),
            span,
            target_eid,
            relation,
            resolved: false,
            external,
            target_type,
        });
    }

    // 3. Reverse citations (e.g. "sub-section (1) of section 22", "clause (b) of sub-section (2) of section 16")
    for caps in get_reverse_cite_re().captures_iter(text) {
        let mat = caps.get(0).unwrap();
        let b_start = mat.start();
        let mut b_end = mat.end();
        if is_byte_covered(&covered_bytes, b_start, b_end) {
            continue;
        }
        if followed_by_locator(text, b_end) {
            continue;
        }

        let target_eid = eid::provision_eid(&LocatorPath {
            section: caps.name("sec").unwrap().as_str().to_owned(),
            subsection: caps.name("subsec").map(|m| m.as_str().to_owned()),
            clause: caps.name("cl").map(|m| m.as_str().to_owned()),
            subclause: caps.name("subcl").map(|m| m.as_str().to_owned()),
        });

        let mut external = false;
        let mut target_type = None;
        if let Some((_act, new_end)) = check_external_act(text, b_end) {
            b_end = new_end;
            external = true;
            target_type = Some(ExternalTargetType::ExternalAct);
        }

        let span = byte_to_char_span(text, b_start, b_end);
        let relation =
            detect_relation_type(&text[relation_start(text, b_start)..relation_end(text, b_end)]);
        covered_bytes.push((b_start, b_end));

        results.push(CitationReference {
            parent_eid: parent_eid.to_owned(),
            raw_text: text[b_start..b_end].to_owned(),
            span,
            target_eid,
            relation,
            resolved: false,
            external,
            target_type,
        });
    }

    // 4. Rule citations (e.g. "rule 36(4)", "sub-rule (2) of rule 37", "rule 42")
    for caps in get_rule_cite_re().captures_iter(text) {
        let mat = caps.get(0).unwrap();
        let b_start = mat.start();
        let mut b_end = mat.end();
        if is_byte_covered(&covered_bytes, b_start, b_end) {
            continue;
        }

        let rule = caps.name("rule").unwrap().as_str();
        let subrule = caps
            .name("subrule_rev")
            .or_else(|| caps.name("subrule_fwd"));
        let mut target_eid = eid::rule_eid(rule);
        if let Some(sr) = subrule {
            target_eid = eid::subrule_eid(&target_eid, sr.as_str());
        }

        // The instrument is part of the citation's extent, so `raw_text` and `span` must
        // include it. The target is still the rule, so `target_type` stays `Rule`.
        if let Some((_act, new_end)) = check_external_act(text, b_end) {
            b_end = new_end;
        }

        let span = byte_to_char_span(text, b_start, b_end);
        let relation =
            detect_relation_type(&text[relation_start(text, b_start)..relation_end(text, b_end)]);
        covered_bytes.push((b_start, b_end));

        // Rules are external to the Bare Act itself
        results.push(CitationReference {
            parent_eid: parent_eid.to_owned(),
            raw_text: text[b_start..b_end].to_owned(),
            span,
            target_eid,
            relation,
            resolved: false,
            external: true,
            target_type: Some(ExternalTargetType::Rule),
        });
    }

    // 5. Forward citations (e.g. "section 22", "section 16(2)", "section 16(2)(b)")
    for caps in get_forward_cite_re().captures_iter(text) {
        let mat = caps.get(0).unwrap();
        let b_start = mat.start();
        let mut b_end = mat.end();
        if is_byte_covered(&covered_bytes, b_start, b_end) {
            continue;
        }

        let target_eid = eid::provision_eid(&LocatorPath {
            section: caps.name("sec").unwrap().as_str().to_owned(),
            subsection: caps.name("subsec").map(|m| m.as_str().to_owned()),
            clause: caps.name("cl").map(|m| m.as_str().to_owned()),
            subclause: caps.name("subcl").map(|m| m.as_str().to_owned()),
        });

        let mut external = false;
        let mut target_type = None;
        if let Some((_act, new_end)) = check_external_act(text, b_end) {
            b_end = new_end;
            external = true;
            target_type = Some(ExternalTargetType::ExternalAct);
        }

        let span = byte_to_char_span(text, b_start, b_end);
        let relation =
            detect_relation_type(&text[relation_start(text, b_start)..relation_end(text, b_end)]);
        covered_bytes.push((b_start, b_end));

        results.push(CitationReference {
            parent_eid: parent_eid.to_owned(),
            raw_text: text[b_start..b_end].to_owned(),
            span,
            target_eid,
            relation,
            resolved: false,
            external,
            target_type,
        });

        // A plural head ("sections 16 and 17") names further provisions after the first.
        for (numeral, start, end) in enumerated_sections(text, b_end) {
            results.push(CitationReference {
                parent_eid: parent_eid.to_owned(),
                raw_text: numeral.clone(),
                span: byte_to_char_span(text, start, end),
                target_eid: eid::section_eid(&numeral),
                relation,
                resolved: false,
                external: false,
                target_type: None,
            });
            covered_bytes.push((start, end));
        }
    }

    // Sort by span start
    results.sort_by_key(|c| c.span[0]);
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_reverse_citations() {
        let text = "Notwithstanding anything contained in sub-section (1) of section 22, every person shall register.";
        let citations = extract_citations("sec_24", text);
        assert_eq!(citations.len(), 1);
        let cite = &citations[0];
        assert_eq!(cite.raw_text, "sub-section (1) of section 22");
        assert_eq!(cite.target_eid, "sec_22__subsec_1");
        assert_eq!(cite.relation, RelationType::Notwithstanding);
        assert!(!cite.external);
    }

    #[test]
    fn extracts_nested_reverse_citations() {
        let text = "Subject to the provisions of clause (b) of sub-section (2) of section 16, credit shall be allowed.";
        let citations = extract_citations("sec_17", text);
        assert_eq!(citations.len(), 1);
        let cite = &citations[0];
        assert_eq!(cite.raw_text, "clause (b) of sub-section (2) of section 16");
        assert_eq!(cite.target_eid, "sec_16__subsec_2__cl_b");
        assert_eq!(cite.relation, RelationType::SubjectTo);
        assert!(!cite.external);
    }

    #[test]
    fn extracts_rule_citation() {
        let text = "in accordance with rule 36(4) and sub-rule (2) of rule 37 as prescribed.";
        let citations = extract_citations("sec_16__subsec_2", text);
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].raw_text, "rule 36(4)");
        assert_eq!(citations[0].target_eid, "rule_36__subrule_4");
        assert!(citations[0].external);
        assert_eq!(citations[0].target_type, Some(ExternalTargetType::Rule));

        assert_eq!(citations[1].raw_text, "sub-rule (2) of rule 37");
        assert_eq!(citations[1].target_eid, "rule_37__subrule_2");
        assert!(citations[1].external);
        assert_eq!(citations[1].target_type, Some(ExternalTargetType::Rule));
    }

    #[test]
    fn extracts_external_act_citation() {
        let text = "referred to in section 5 of the Integrated Goods and Services Tax Act, 2017.";
        let citations = extract_citations("sec_9", text);
        assert_eq!(citations.len(), 1);
        assert!(citations[0].external);
        assert_eq!(
            citations[0].target_type,
            Some(ExternalTargetType::ExternalAct)
        );
    }
}
