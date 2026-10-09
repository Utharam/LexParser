use super::ast::AmendmentMetadata;
use regex::Regex;
use std::sync::OnceLock;

static AMENDMENT_RE: OnceLock<Regex> = OnceLock::new();
static WEF_RE: OnceLock<Regex> = OnceLock::new();
static FOR_RE: OnceLock<Regex> = OnceLock::new();
static SEC_RE: OnceLock<Regex> = OnceLock::new();
static ACT_RE: OnceLock<Regex> = OnceLock::new();

/// The lead vocabulary an amendment footnote opens with. Shared verbatim by the
/// separation test in `parser.rs` and by the parse below, so the two halves of one
/// feature cannot drift again. The *separation* form is the union: `The words` with
/// no `omitted` refinement, and `w.e.f.`, which the amendment side must accept too.
///
/// `Provided` is deliberately absent: a line opening `Provided` is a proviso, and a
/// footnote lead matching `Provided` would swallow provisos.
/// The two verbs appear in both the abbreviated and the spelled-out form because this
/// alternation is the only consumer of the spelled form: a gazette note prints
/// "1. Substituted for … by Act …" as often as "1. Subs. for …", and only the latter
/// matched. `parse_amendment_footnote` derives `action` from substrings rather than from
/// this group, so widening the group cannot change any action it reports.
pub(crate) const LEAD_ALTERNATION: &str =
    r"(?:Subs\.|Ins\.|Substituted|Inserted|Omitted|The\s+words|Amended|w\.e\.f\.)";

/// Gazette footnote numbers are printed inconsistently: `1. Subs. by` and
/// `1 Subs. by` are both in circulation, and the period is present only in the
/// first. Requiring it loses every note of the second kind.
pub(crate) const LEAD_NUMBER: &str = r"(?P<num>[0-9]+)\.?\s*";

fn get_amendment_re() -> &'static Regex {
    AMENDMENT_RE.get_or_init(|| {
        Regex::new(
            &[
                r"(?i)^",
                LEAD_NUMBER,
                r"(?P<lead>",
                LEAD_ALTERNATION,
                r").*$",
            ]
            .concat(),
        )
        .unwrap()
    })
}

fn get_wef_re() -> &'static Regex {
    WEF_RE.get_or_init(|| Regex::new(r"(?i)\(w\.e\.f\.\s*([^)]+)\)").unwrap())
}

fn get_for_re() -> &'static Regex {
    FOR_RE.get_or_init(|| Regex::new(r#"(?i),\s*for\s+["“](.*?)["”]"#).unwrap())
}

fn get_sec_re() -> &'static Regex {
    SEC_RE.get_or_init(|| Regex::new(r"(?i)\b(?:s\.|section)\s*([0-9]+[A-Za-z]*)").unwrap())
}

fn get_act_re() -> &'static Regex {
    ACT_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\bby\s+((?:Act\s+[0-9]+\s+of\s+[0-9]{4}|the\s+[^,]+Act(?:,\s*[0-9]{4})?))\b",
        )
        .unwrap()
    })
}

/// Reads a bare integer from the front of a note, across an optional period and
/// any run of spaces. Gazette footnotes print `1.`, `1` and `1 ` interchangeably,
/// and the remainder of the line is unpunctuated until the first full stop much
/// later, so the delimiter cannot be used to find the end of the number.
pub(crate) fn leading_integer(text: &str) -> &str {
    let end = text
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map_or(text.len(), |(index, _)| index);
    text[..end].trim_end_matches(['.', ' '])
}

/// Parses raw footnote text into structured amendment metadata if it describes a statutory amendment.
pub fn parse_amendment_footnote(text: &str) -> Option<AmendmentMetadata> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Footnote lead line: e.g. "1. Subs. by...", "2. Ins. by...", or the
    // period-less gazette form "1 Omitted "..." by Act 12 of 2020".
    let re = get_amendment_re();
    let num = if let Some(caps) = re.captures(trimmed) {
        caps.name("num").map(|m| m.as_str().to_owned())?
    } else {
        // No lead matched, so the note carries no amendative lead vocabulary.
        // It may still be a numbered note, and the number is what links it to a
        // marker in the body, so read a bare leading integer.
        leading_integer(trimmed).to_owned()
    };

    let lower = trimmed.to_lowercase();
    let action = if lower.contains("subs.") || lower.contains("substituted") {
        "substituted".to_owned()
    } else if lower.contains("ins.") || lower.contains("inserted") {
        "inserted".to_owned()
    } else if lower.contains("omitted") {
        "omitted".to_owned()
    } else if lower.contains("amended") || lower.contains("amendment") {
        "amended".to_owned()
    } else {
        // A note with no amendative verb is a note, not an amendment. `None` keeps it in
        // `doc.footnotes` with `amendment: null` instead of fabricating an "amended"
        // record whose every other field is also None.
        return None;
    };

    let effective_date = get_wef_re()
        .captures(trimmed)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_owned());

    let substituted_for = get_for_re()
        .captures(trimmed)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_owned());

    let amending_section = get_sec_re()
        .captures(trimmed)
        .map(|caps| caps[0].trim().to_owned());

    let amending_act = get_act_re()
        .captures(trimmed)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_owned());

    Some(AmendmentMetadata {
        footnote_number: num,
        action,
        amending_act,
        amending_section,
        effective_date,
        substituted_for,
        raw_text: trimmed.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_substituted_amendment() {
        let text =
            r#"1. Subs. by Act 12 of 2020, s. 118, for "sub-section (4)" (w.e.f. 1-1-2021)."#;
        let meta = parse_amendment_footnote(text).expect("should parse");
        assert_eq!(meta.footnote_number, "1");
        assert_eq!(meta.action, "substituted");
        assert_eq!(meta.amending_act.as_deref(), Some("Act 12 of 2020"));
        assert_eq!(meta.amending_section.as_deref(), Some("s. 118"));
        assert_eq!(meta.effective_date.as_deref(), Some("1-1-2021"));
        assert_eq!(meta.substituted_for.as_deref(), Some("sub-section (4)"));
    }

    #[test]
    fn parses_inserted_amendment() {
        let text = "2. Ins. by Act 31 of 2018, s. 8 (w.e.f. 1-2-2019).";
        let meta = parse_amendment_footnote(text).expect("should parse");
        assert_eq!(meta.footnote_number, "2");
        assert_eq!(meta.action, "inserted");
        assert_eq!(meta.effective_date.as_deref(), Some("1-2-2019"));
    }
}
