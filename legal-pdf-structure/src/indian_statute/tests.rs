use super::ast::{ExternalTargetType, ProvisionKind, RelationType};
use super::citation::extract_citations;
use super::resolution::{build_provision_table, resolve_citations};
use super::*;
use serde_json::json;

/// Fixture provenance, per `AGENTS.md`: most fixtures below are independently invented, and
/// the ones that are not say so. `F_LONG`, `parses_indian_statute_cgst_hierarchy` and the
/// fixtures in `contract.rs` reproduce section headings taken from the Central Goods and
/// Services Tax Act, 2017, which is a public statute published by the Government of India.
/// No genuine user query, bug-report identifier, private document or non-public locator
/// appears anywhere in this file.

fn line(
    page_number: u32,
    reading_order: usize,
    source_index: usize,
    text: &str,
) -> serde_json::Value {
    json!({
        "id": format!("l{page_number}-{source_index}"),
        "page_index": page_number - 1,
        "page_number": page_number,
        "source_index": source_index,
        "reading_order": reading_order,
        "block_index": 0,
        "text": text,
        "bbox": [0.0, 0.0, 100.0, 10.0]
    })
}

/// A line carrying superscript spans, which is how a gazette prints a note reference.
///
/// `starts` are CHARACTER offsets into `text`, matching what the extraction pass produces:
/// it clamps span offsets against `raw_text.chars().count()`, not against a byte length.
fn line_with_superscripts(
    page_number: u32,
    reading_order: usize,
    source_index: usize,
    text: &str,
    superscripts: &[&str],
) -> serde_json::Value {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = serde_json::Value::Array(Vec::new());
    let mut cursor = 0usize;
    for marker in superscripts {
        // Each marker is located by its first occurrence at or after the cursor, so the
        // fixture states intent rather than hand-computed offsets.
        let offset = chars
            .iter()
            .enumerate()
            .skip(cursor)
            .find(|(_, c)| **c == marker.chars().next().unwrap_or(' '))
            .map(|(index, _)| index);
        let Some(start) = offset else { continue };
        cursor = start + 1;
        let end = start + marker.chars().count();
        spans.as_array_mut().expect("array").push(json!({
            "id": format!("s{source_index}-{start}"),
            "text": marker,
            "bbox": [0.0, 0.0, 5.0, 5.0],
            "font": "",
            "size": 6.0,
            "flags": 1,
            "superscript": true,
            "start": start,
            "end": end
        }));
    }
    let mut value = line(page_number, reading_order, source_index, text);
    value["spans"] = spans;
    value
}

fn page(number: u32, lines: Vec<serde_json::Value>) -> serde_json::Value {
    json!({
        "id": format!("p{number}"),
        "index": number - 1,
        "number": number,
        "width": 100.0,
        "height": 100.0,
        "lines": lines,
        "regions": []
    })
}

fn pages_of(values: Vec<serde_json::Value>) -> Vec<legal_pdf_core::model::Page> {
    serde_json::from_value(json!(values)).expect("pages")
}

fn tree_eids(nodes: &[ProvisionNode]) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(nodes: &[ProvisionNode], out: &mut Vec<String>) {
        for node in nodes {
            out.push(node.eid.clone());
            walk(&node.children, out);
        }
    }
    walk(nodes, &mut out);
    out
}

fn count_nodes(nodes: &[ProvisionNode]) -> usize {
    nodes.iter().map(|n| 1 + count_nodes(&n.children)).sum()
}

/// `A4` harm 1: no eId may appear twice. No current test asserted this.
fn assert_eids_unique(doc: &IndianStatuteDocument) {
    let tree = tree_eids(&doc.akoma_ntoso.act.body);
    let mut seen = std::collections::HashSet::new();
    for eid in &tree {
        assert!(
            seen.insert(eid.clone()),
            "duplicate eId in body tree: {eid}"
        );
    }
    let flat: Vec<&str> = doc.provisions.iter().map(|p| p.eid.as_str()).collect();
    assert_eq!(
        flat.len(),
        seen.len(),
        "provisions is not the body tree exactly once each: {flat:?}"
    );
    for eid in &flat {
        assert!(
            seen.contains(*eid),
            "provision {eid} is not in the body tree"
        );
    }
}

fn find<'a>(nodes: &'a [ProvisionNode], eid: &str) -> Option<&'a ProvisionNode> {
    nodes
        .iter()
        .find(|n| n.eid == eid)
        .or_else(|| nodes.iter().find_map(|n| find(&n.children, eid)))
}

fn codes(doc: &IndianStatuteDocument, code: &str) -> usize {
    doc.diagnostics.iter().filter(|d| d.code == code).count()
}

/// Long title, enacting formula, a chapter heading and its title, sections.
const F_LONG: &str = "\
THE CENTRAL GOODS AND SERVICES TAX ACT, 2017
An Act to provide for the levy and collection of goods and services tax.
Be it enacted by Parliament in the thirty-seventh year of the Republic as follows:-
CHAPTER I
PRELIMINARY
1. Short title and extent.-(1) This Act may be cited as the Short Title Act.
2. Definitions.-In this Act, unless the context otherwise requires, the assigned meaning prevails.
CHAPTER II
RATES
4. Levy on specified supplies.-Tax shall be levied at the notified rate.
5. Valuation.-The value of supply shall be the transaction value.
";

#[test]
fn parses_indian_statute_cgst_hierarchy() {
    let statutory_text = "\
16. Eligibility and conditions for taking input tax credit.—(1) Every registered person shall, subject to such conditions and restrictions as may be prescribed and in the manner specified in section 49, be entitled to take credit of input tax charged on any supply of goods or services or both to him.
(2) Notwithstanding anything contained in this section, no registered person shall be entitled to the credit of any input tax in respect of any supply of goods or services or both to him unless,—
(a) he is in possession of a tax invoice or debit note issued by a supplier registered under this Act;
(b) 1[he has received the goods or services or both in accordance with rule 36(4).]
(c) subject to the provisions of section 49, the tax charged in respect of such supply has been actually paid to the Government;
(d) he has furnished the return under section 39.
Provided that where the goods against an invoice are received in lots or instalments, the registered person shall be entitled to take credit upon receipt of the last lot or instalment:
Provided further that where a recipient fails to pay to the supplier, the amount shall be added to output tax liability.
Explanation 1.—For the purposes of this sub-section, goods shall be deemed to have been received.
22. Persons liable for registration.—(1) Every supplier shall be liable to be registered under this Act.
24. Compulsory registration in certain cases.—Notwithstanding anything contained in sub-section (1) of section 22, the following categories of persons shall be registered under this Act.
1. Subs. by Act 12 of 2020, s. 118, for \"he has received the goods\" (w.e.f. 1-1-2021).
";

    let doc = parse_indian_statute_from_text(
        statutory_text,
        Some("Central Goods and Services Tax Act, 2017"),
    );

    assert_eq!(doc.schema_version, "akn.statute.v1");
    // This fixture has no long title, so the caller's title is used.
    assert_eq!(
        doc.akoma_ntoso.act.meta.title,
        "Central Goods and Services Tax Act, 2017"
    );

    let sections = &doc.akoma_ntoso.act.body;
    assert_eq!(sections.len(), 3);

    let sec16 = &sections[0];
    assert_eq!(sec16.eid, "sec_16");
    assert_eq!(sec16.kind, ProvisionKind::Section);
    assert_eq!(sec16.num, "16");
    assert_eq!(
        sec16.heading.as_deref(),
        Some("Eligibility and conditions for taking input tax credit")
    );
    assert_eq!(
        sec16.text, "",
        "P21: the section must not repeat its first subsection"
    );

    assert_eq!(sec16.children.len(), 2);
    let subsec1 = &sec16.children[0];
    assert_eq!(subsec1.eid, "sec_16__subsec_1");
    assert_eq!(subsec1.kind, ProvisionKind::Subsection);

    let subsec2 = &sec16.children[1];
    assert_eq!(subsec2.eid, "sec_16__subsec_2");
    assert_eq!(subsec2.kind, ProvisionKind::Subsection);

    let sub2_children = &subsec2.children;
    let cl_a = sub2_children
        .iter()
        .find(|c| c.eid == "sec_16__subsec_2__cl_a")
        .expect("cl_a");
    assert_eq!(cl_a.kind, ProvisionKind::Clause);

    let cl_b = sub2_children
        .iter()
        .find(|c| c.eid == "sec_16__subsec_2__cl_b")
        .expect("cl_b");
    assert_eq!(cl_b.kind, ProvisionKind::Clause);

    assert_eq!(cl_b.amendments.len(), 1);
    assert_eq!(cl_b.amendments[0].footnote_number, "1");
    assert_eq!(cl_b.amendments[0].action, "substituted");
    assert_eq!(
        cl_b.amendments[0].amending_act.as_deref(),
        Some("Act 12 of 2020")
    );
    assert_eq!(
        cl_b.amendments[0].effective_date.as_deref(),
        Some("1-1-2021")
    );

    let prov1 = sub2_children
        .iter()
        .find(|c| c.eid == "sec_16__subsec_2__proviso_1")
        .expect("prov1");
    assert_eq!(prov1.kind, ProvisionKind::Proviso);
    let prov2 = sub2_children
        .iter()
        .find(|c| c.eid == "sec_16__subsec_2__proviso_2")
        .expect("prov2");
    assert_eq!(prov2.kind, ProvisionKind::Proviso);

    // The fixture's `Explanation 1` is preceded by two proviso lines, and every proviso /
    // explanation arm calls `flush_clause` first, so `current_clause` is `None` when the
    // explanation is processed. That non-obvious interaction is why this eId still exists.
    let expl1 = sub2_children
        .iter()
        .find(|c| c.eid == "sec_16__subsec_2__expl_1")
        .expect("expl1");
    assert_eq!(expl1.kind, ProvisionKind::Explanation);

    let sec24 = &sections[2];
    assert_eq!(sec24.eid, "sec_24");
    assert!(!sec24.citations.is_empty());
    let cite22 = sec24
        .citations
        .iter()
        .find(|c| c.target_eid == "sec_22__subsec_1")
        .expect("cite22");
    assert_eq!(cite22.raw_text, "sub-section (1) of section 22");
    assert_eq!(cite22.relation, RelationType::Notwithstanding);
    assert!(cite22.resolved);
    assert!(!cite22.external);

    let rule_cite = cl_b
        .citations
        .iter()
        .find(|c| c.target_eid == "rule_36__subrule_4")
        .expect("rule_cite");
    assert_eq!(rule_cite.raw_text, "rule 36(4)");
    assert!(!rule_cite.resolved);
    assert!(rule_cite.external);
    assert_eq!(rule_cite.target_type, Some(ExternalTargetType::Rule));

    assert_eq!(doc.footnotes.len(), 1);
    assert_eq!(doc.footnotes[0].number, "1");
    assert_eq!(
        doc.footnotes[0].associated_eid.as_deref(),
        Some("sec_16__subsec_2__cl_b")
    );

    assert_eids_unique(&doc);

    // A structured round-trip, not a substring search of the serialised form.
    let json = to_akn_json_string(&doc, true).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("round-trip");
    assert_eq!(value["schema_version"], "akn.statute.v1");
    assert_eq!(value["akomaNtoso"]["act"]["body"][0]["eid"], "sec_16");
    assert_eq!(
        value["akomaNtoso"]["act"]["body"][0]["children"][1]["children"][1]["eid"],
        "sec_16__subsec_2__cl_b"
    );
    let serialized_relations: Vec<&str> = value["citations"]
        .as_array()
        .expect("citations array")
        .iter()
        .map(|cite| cite["relation"].as_str().expect("relation"))
        .collect();
    assert!(
        serialized_relations.contains(&"notwithstanding"),
        "a sec_24 citation must serialize as notwithstanding: {serialized_relations:?}"
    );
    let sec_24_citation = doc
        .citations
        .iter()
        .find(|cite| cite.parent_eid == "sec_24")
        .expect("a sec_24 citation");
    assert_eq!(sec_24_citation.relation, RelationType::Notwithstanding);
}

#[test]
fn page_path_and_text_path_agree_on_top_level_eids() {
    // `A27`: the only substantive fixture used to go through the text path alone, a path
    // production never uses.
    let text = "\
16. Eligibility.—(1) Every person shall be entitled to credit.
(2) No person shall be entitled unless,—
(a) he holds an invoice;
(b) he has paid the tax.
22. Registration.—Every supplier shall be registered.
";
    let from_text = parse_indian_statute_from_text(text, Some("Fixture Act"));
    let from_pages = parse_indian_statute_from_pages(
        &pages_of(vec![page(
            1,
            text.lines()
                .filter(|source| !source.trim().is_empty())
                .enumerate()
                .map(|(index, source)| line(1, index, index, source))
                .collect(),
        )]),
        Some("Fixture Act"),
    );

    let text_eids: Vec<&str> = from_text
        .akoma_ntoso
        .act
        .body
        .iter()
        .map(|n| n.eid.as_str())
        .collect();
    let page_eids: Vec<&str> = from_pages
        .akoma_ntoso
        .act
        .body
        .iter()
        .map(|n| n.eid.as_str())
        .collect();
    assert_eq!(text_eids, page_eids);
    assert_eq!(text_eids, vec!["sec_16", "sec_22"]);
}

#[test]
fn page_lines_are_read_in_reading_order() {
    // `A6`: page 1 supplies reading_order 1 first and 0 second, which vector order alone
    // would reverse. The pre-existing page test supplied 0 then 1 in vector order and so
    // could not detect this. The reordering is *within* a page, which is what `Line`'
    // `reading_order` means; page order is document order.
    let pages = pages_of(vec![page(
        1,
        vec![
            line(1, 1, 1, "22. Second section.-Text."),
            line(1, 0, 0, "16. First section.-Text."),
        ],
    )]);
    let doc = parse_indian_statute_from_pages(&pages, None);
    let body = &doc.akoma_ntoso.act.body;
    assert_eq!(
        body[0].eid, "sec_16",
        "the reading_order 0 line must be consumed first"
    );
    assert_eq!(body[1].eid, "sec_22");
}

#[test]
fn pre_section_content_is_recovered() {
    // `A2`: five front-matter lines used to vanish with `roots=1, provisions=2`.
    let doc = parse_indian_statute_from_text(F_LONG, None);
    let body = &doc.akoma_ntoso.act.body;
    assert_eq!(body[0].eid, "act_preamble");
    assert_eq!(body[0].kind, ProvisionKind::Chapter);
    assert_eq!(body[0].parent_eid, None);
    assert!(body[0].text.contains("Be it enacted"));
    assert_eq!(codes(&doc, "statute.preamble_recovered"), 1);

    // Every input line appears in exactly one node's text.
    let combined = doc
        .akoma_ntoso
        .act
        .body
        .iter()
        .map(|n| format!("{} {}", n.heading.clone().unwrap_or_default(), n.text))
        .collect::<Vec<_>>()
        .join(" ");
    for needle in [
        "THE CENTRAL GOODS AND SERVICES TAX ACT, 2017",
        "An Act to provide for the levy",
        "Be it enacted by Parliament",
    ] {
        assert!(
            combined.contains(needle),
            "front-matter line lost: {needle:?}\n{combined}"
        );
    }
    assert_eids_unique(&doc);
}

#[test]
fn orphan_nodes_become_roots() {
    // `A2` probe P1a: a hierarchy with no section at all used to produce roots=0,
    // provisions=0.
    let doc = parse_indian_statute_from_text(
        "(1) A scheme shall be notified.\n(a) the scheme shall be published;\n(i) by hand;\n(ii) by post;\n(b) the scheme shall be audited.\n",
        None,
    );
    let body = &doc.akoma_ntoso.act.body;
    assert!(!body.is_empty(), "every provision was discarded");
    for root in body {
        assert_eq!(root.parent_eid, None);
    }
    // The `sec_unknown` marker survives as the record that no section ancestor was open.
    assert!(tree_eids(body)
        .iter()
        .any(|eid| eid.starts_with("sec_unknown")));
}

#[test]
fn orphan_line_without_any_container_is_reported() {
    // `A2`. The two arms of `append_continuation_line` that are reachable: a stray line
    // inside an open chapter joins that chapter, and front matter with nothing open becomes
    // `preamble`. The third arm (`statute.orphan_line`) requires `roots` non-empty, nothing
    // open, and the last root not a chapter — which no line sequence can produce, because
    // the only paths that leave nothing open either push a chapter or reopen a section. See
    // the implementation log: that arm is dead as specified and is left as a safety net.
    let inside_chapter = parse_indian_statute_from_text(
        "1. First section.-Text.\nCHAPTER I\nThis schedule is referred to as the First Schedule.\n2. Second section.-Text.\n",
        None,
    );
    let chapter = find(&inside_chapter.akoma_ntoso.act.body, "chapter_1").expect("chapter_1");
    assert!(chapter.text.contains("First Schedule"));
    assert_eq!(codes(&inside_chapter, "statute.orphan_line"), 0);

    let front_matter =
        parse_indian_statute_from_text("AN ACT to amend the law.\n1. Short title.-Text.\n", None);
    let preamble = find(&front_matter.akoma_ntoso.act.body, "act_preamble").expect("preamble");
    assert!(preamble.text.contains("AN ACT to amend the law."));
    assert_eq!(codes(&front_matter, "statute.orphan_line"), 0);
    assert_eq!(codes(&front_matter, "statute.preamble_recovered"), 1);
}

#[test]
fn decimals_dates_and_years_are_not_sections() {
    // `A4`, from the adjudication's table. `1. National Identity Document.` is the list-item
    // control: no local cue separates it from `1. Short title.`, so it *does* mint a section.
    // What must not happen is a decimal, a date or a bare year minting one.
    let text = "\
1.5 per cent per annum.
1.4.2019.
3.14. Value of supply is the transaction value.
2019. The Government may by notification specify a rate.
1. National Identity Document.
";
    let doc = parse_indian_statute_from_text(text, None);
    let body = &doc.akoma_ntoso.act.body;
    let sections: Vec<&ProvisionNode> = body
        .iter()
        .filter(|node| node.kind == ProvisionKind::Section)
        .collect();
    let numerals: Vec<&str> = sections.iter().map(|node| node.num.as_str()).collect();
    assert_eq!(
        numerals,
        vec!["1"],
        "only the list item may mint a section: {numerals:?}"
    );
    for spurious in ["sec_3", "sec_2019"] {
        assert!(
            !tree_eids(body).contains(&spurious.to_owned()),
            "{spurious} must not be minted"
        );
    }
    let sec1 = find(body, "sec_1").expect("sec_1");
    assert!(
        sec1.heading
            .as_deref()
            .unwrap_or_default()
            .contains("National Identity Document"),
        "sec_1 must be the list item, not the decimal: {:?} / {:?}",
        sec1.heading,
        sec1.text
    );
    assert_eq!(codes(&doc, "statute.duplicate_section_number"), 0);
}

#[test]
fn section_start_negative_controls_do_not_match() {
    let doc = parse_indian_statute_from_text(
        "1,000 units shall be supplied.\n10,000 rupees is the penalty.\nSection 22 states that X.\n1.4.2019 without a trailing dot\n",
        None,
    );
    let eids = tree_eids(&doc.akoma_ntoso.act.body);
    assert!(
        !eids
            .iter()
            .any(|eid| eid.starts_with("sec_") && eid != "sec_unknown"),
        "a negative control minted a section: {eids:?}"
    );
}

#[test]
fn a_section_number_may_reach_four_digits_with_the_prefix() {
    // This is the test Plan A's single `{1,3}` bound would fail.
    let doc = parse_indian_statute_from_text("Section 1234. Title.-text.\n", None);
    assert!(find(&doc.akoma_ntoso.act.body, "sec_1234").is_some());
}

#[test]
fn repeated_section_number_is_kept_as_text() {
    // `A4`. The `R1` shape relied on `1.5 per cent` minting a second `sec_1`; the new digit
    // guard stops that, so the decimal is a continuation of `sec_2` and no duplicate is
    // reported. The duplicate guard is then exercised with a genuinely repeated numeral.
    let doc = parse_indian_statute_from_text(
        "1. First section.-Text.\n2. Second section.-Text.\n1.5 per cent per annum.\n3. Third section.-Text.\n",
        None,
    );
    let sec2 = find(&doc.akoma_ntoso.act.body, "sec_2").expect("sec_2");
    assert!(
        sec2.text.contains("1.5 per cent per annum."),
        "the decimal must be kept as continuation text: {:?}",
        sec2.text
    );
    assert_eq!(codes(&doc, "statute.duplicate_section_number"), 0);
    assert_eids_unique(&doc);
    assert_eq!(
        build_provision_table(&doc.akoma_ntoso.act.body).len(),
        count_nodes(&doc.akoma_ntoso.act.body),
        "a node was dropped by the HashSet insert"
    );

    let repeated = parse_indian_statute_from_text(
        "1. First section.-Text.\n2. Second section.-Text.\n1. Repeated number.-More text.\n",
        None,
    );
    assert_eq!(codes(&repeated, "statute.duplicate_section_number"), 1);
    let sec2 = find(&repeated.akoma_ntoso.act.body, "sec_2").expect("sec_2");
    assert!(
        sec2.text.contains("Repeated number"),
        "the repeated numeral must be kept as text: {:?}",
        sec2.text
    );
    assert_eids_unique(&repeated);
}

#[test]
fn roman_subclauses_nest_under_their_clause() {
    // `A3` probe P18: pre-change `(i)` became `cl_i` and swallowed the rest.
    let doc = parse_indian_statute_from_text(
        "16. Credit.—(1) A person shall be eligible, namely:—\n(a) receipt, namely:—\n(i) by hand delivery;\n(ii) by post;\n(iii) by courier.\n",
        None,
    );
    for token in ["i", "ii", "iii"] {
        let node = find(
            &doc.akoma_ntoso.act.body,
            &format!("sec_16__subsec_1__cl_a__subcl_{token}"),
        )
        .unwrap_or_else(|| panic!("missing subclause {token}"));
        assert_eq!(node.kind, ProvisionKind::Subclause);
    }
    assert!(find(&doc.akoma_ntoso.act.body, "sec_16__subsec_1__cl_i").is_none());
}

#[test]
fn ninth_clause_letter_still_classifies_as_a_clause() {
    // This is the test Plan B's design fails.
    let clauses: String = (b'a'..=b'i')
        .map(|letter| format!("({}) a clause.\n", letter as char))
        .collect();
    let doc = parse_indian_statute_from_text(
        &format!("16. Credit.—(1) The person shall be eligible:—\n{clauses}"),
        None,
    );
    let ninth = find(&doc.akoma_ntoso.act.body, "sec_16__subsec_1__cl_i")
        .expect("the ninth clause is cl_i");
    assert_eq!(ninth.kind, ProvisionKind::Clause);
}

#[test]
fn three_letter_roman_is_always_a_subclause() {
    // `A3` probe P1b: with no clause open, `(iii)` is still a sub-clause. Plan A's
    // `current_clause.is_none()` disjunct sent this to the clause arm.
    let doc = parse_indian_statute_from_text(
        "16. Credit.—(1) The person shall be eligible:—\n(iii) by courier.\n",
        None,
    );
    let node = find(&doc.akoma_ntoso.act.body, "sec_16__subsec_1__subcl_iii")
        .expect("long roman is a sub-clause with no clause open");
    assert_eq!(node.kind, ProvisionKind::Subclause);
    assert_eq!(node.parent_eid.as_deref(), Some("sec_16__subsec_1"));
}

#[test]
fn c_and_d_remain_clauses() {
    // An un-narrowed `[ivxlcdm]+` reorder makes these sub-clauses of the previous clause.
    let doc = parse_indian_statute_from_text(
        "16. Credit.—(1) The person shall be eligible:—\n(a) first;\n(c) third;\n(d) fourth.\n",
        None,
    );
    for token in ["c", "d"] {
        let node = find(
            &doc.akoma_ntoso.act.body,
            &format!("sec_16__subsec_1__cl_{token}"),
        )
        .unwrap_or_else(|| panic!("({token}) must stay a clause"));
        assert_eq!(node.kind, ProvisionKind::Clause);
    }
}

#[test]
fn alphanumeric_subsection_is_untouched() {
    let doc = parse_indian_statute_from_text("16. Credit.—\n(1a) An inserted sub-section.\n", None);
    let node = find(&doc.akoma_ntoso.act.body, "sec_16__subsec_1a").expect("sec_16__subsec_1a");
    assert_eq!(node.kind, ProvisionKind::Subsection);
}

#[test]
fn proviso_lead_variants_are_recognised() {
    // `A15` probe S4, all nine lines.
    let leads = [
        "Provided that",
        "Provided further that",
        "Provided also that",
        "Provided even further that",
        "Provided that and where",
        "Provided further also that",
        "Provided further further that",
        "Provided always that",
        "Provided further,",
    ];
    for lead in leads {
        let doc = parse_indian_statute_from_text(
            &format!("16. Credit.—(2) A person shall be eligible.—\n{lead} the condition holds.\n"),
            None,
        );
        let proviso = find(&doc.akoma_ntoso.act.body, "sec_16__subsec_2__proviso_1")
            .unwrap_or_else(|| {
                panic!("lead {lead:?} produced no proviso; provisos leaked into text")
            });
        assert!(
            !proviso.text.is_empty(),
            "lead {lead:?} produced an empty proviso"
        );
        let parent = find(&doc.akoma_ntoso.act.body, "sec_16__subsec_2").expect("subsec_2");
        assert!(
            !parent.text.contains("the condition holds"),
            "lead {lead:?} leaked into the parent sub-section: {:?}",
            parent.text
        );
    }
}

#[test]
fn proviso_lead_is_kept_verbatim() {
    let doc = parse_indian_statute_from_text(
        "16. Credit.—(2) A person shall be eligible.—\nProvided further also that the condition holds.\n",
        None,
    );
    let proviso =
        find(&doc.akoma_ntoso.act.body, "sec_16__subsec_2__proviso_1").expect("proviso_1");
    assert_eq!(proviso.num, "Provided further also that");
}

#[test]
fn chapter_headings_do_not_corrupt_preceding_text() {
    // `A28` probe R13: `sec_1.text` used to be "Text. CHAPTER II RATES".
    let doc = parse_indian_statute_from_text(F_LONG, None);
    let body = &doc.akoma_ntoso.act.body;
    for eid in ["sec_1", "sec_2", "sec_4", "sec_5"] {
        let node = find(body, eid).unwrap_or_else(|| panic!("missing {eid}"));
        assert!(
            !node.text.contains("CHAPTER"),
            "{eid} absorbed a chapter heading: {:?}",
            node.text
        );
    }
    let chapter_1 = find(body, "chapter_1").expect("chapter_1");
    assert_eq!(chapter_1.heading.as_deref(), Some("PRELIMINARY"));
    let chapter_2 = find(body, "chapter_2").expect("chapter_2");
    assert_eq!(chapter_2.heading.as_deref(), Some("RATES"));
    assert_eq!(chapter_1.kind, ProvisionKind::Chapter);
    // Sections are not nested under a chapter, so no existing eId changes.
    assert!(find(body, "chapter_1")
        .expect("chapter_1")
        .children
        .is_empty());
    assert!(!tree_eids(body)
        .iter()
        .any(|eid| eid.starts_with("chapter_1__")));
}

#[test]
fn an_allcaps_provision_line_is_not_a_chapter_title() {
    // `.-` is a real marginal-note separator, so "Values" is the heading and the all-caps
    // line is the body. What the test pins is that it is *not* promoted to a chapter title.
    let doc = parse_indian_statute_from_text("1. Values.-THE VALUES PRESCRIBED\n", None);
    let sec1 = find(&doc.akoma_ntoso.act.body, "sec_1").expect("sec_1");
    assert_eq!(sec1.heading.as_deref(), Some("Values"));
    assert!(sec1.text.contains("THE VALUES PRESCRIBED"));
    assert!(
        find(&doc.akoma_ntoso.act.body, "chapter_1").is_none(),
        "an all-caps provision line must not become a chapter"
    );
}

#[test]
fn wrapped_footnote_is_not_leaked_into_a_provision() {
    // `A9` probe R8 verbatim.
    let doc = parse_indian_statute_from_text(
        "5. Where a taxable person fails to pay tax, the amount shall be recoverable at the rate of ten per cent.\n1. Subs. by Act 12 of 2020, s. 118, for\nthe words \"ten per cent\" (w.e.f. 1-1-2021).\n",
        None,
    );
    assert_eq!(doc.footnotes.len(), 1);
    let note = &doc.footnotes[0];
    assert!(note.raw_text.ends_with("(w.e.f. 1-1-2021)."));
    let amendment = note.amendment.as_ref().expect("amendment");
    assert_eq!(amendment.action, "substituted");
    assert_eq!(amendment.effective_date.as_deref(), Some("1-1-2021"));
    let sec5 = find(&doc.akoma_ntoso.act.body, "sec_5").expect("sec_5");
    assert!(
        !sec5.text.contains("w.e.f."),
        "the wrapped note leaked into sec_5: {:?}",
        sec5.text
    );
}

#[test]
fn footnote_wrap_stops_at_a_section_start() {
    let doc = parse_indian_statute_from_text(
        "5. First.-Text.\n1. Subs. by Act 12 of 2020, s. 118, for \"x\" (w.e.f. 1-1-2021).\n6. Eligibility and conditions for taking credit.-Body text.\n",
        None,
    );
    assert_eq!(doc.footnotes.len(), 1);
    let sec6 = find(&doc.akoma_ntoso.act.body, "sec_6").expect("sec_6");
    assert_eq!(
        sec6.heading.as_deref(),
        Some("Eligibility and conditions for taking credit")
    );
    assert_eq!(sec6.text, "Body text.");
    assert!(
        !sec6.text.contains("Act 12 of 2020"),
        "the note was absorbed into the following section"
    );
}

#[test]
fn lowercase_lead_is_a_footnote_not_a_section() {
    // `A10` probe P11b: pre-change this produced roots=2.
    let doc = parse_indian_statute_from_text(
        "5. First.-Text.\n1. subs. by Act 12 of 2020, s. 118, for \"x\" (w.e.f. 1-1-2021).\n",
        None,
    );
    assert_eq!(doc.footnotes.len(), 1);
    let roots: Vec<&str> = doc
        .akoma_ntoso
        .act
        .body
        .iter()
        .map(|n| n.eid.as_str())
        .collect();
    assert_eq!(roots, vec!["sec_5"]);
    assert_eq!(
        doc.footnotes[0]
            .amendment
            .as_ref()
            .expect("amendment")
            .action,
        "substituted"
    );
}

#[test]
fn wef_only_note_is_not_an_amendment() {
    let doc = parse_indian_statute_from_text(
        "5. First.-Text.\n1. w.e.f. 1-1-2021 the rate shall be five per cent.\n",
        None,
    );
    assert_eq!(doc.footnotes.len(), 1);
    assert_eq!(doc.akoma_ntoso.act.body.len(), 1);
    assert!(
        doc.footnotes[0].amendment.is_none(),
        "a note with no amendative verb is not an amendment"
    );
}

#[test]
fn the_words_alternative_still_separates_and_still_parses() {
    // `R7` control, and the regression test for the rejected narrowing of the shared
    // alternation: `The words` with no `omitted` refinement must still separate.
    let doc = parse_indian_statute_from_text(
        "1. The words \"ten per cent\" in section 2 thereof were omitted.\n",
        None,
    );
    assert_eq!(doc.footnotes.len(), 1);
    let amendment = doc.footnotes[0].amendment.as_ref().expect("amendment");
    assert_eq!(amendment.action, "omitted");
    assert_eq!(amendment.amending_section.as_deref(), Some("section 2"));
}

#[test]
fn amounts_and_list_items_are_not_footnote_markers() {
    // `A8`: each of these used to attach a note.
    for text in [
        "A fee of Rs 1 000 shall be paid.",
        "See item [1] of the Schedule.",
        "Payable within 1 year of supply.",
        "Standee 11[the words x] stands.",
        "The sum is Rs 30",
    ] {
        let doc = parse_indian_statute_from_text(
            &format!("1. First.-{text}\n1. Subs. by Act 12 of 2020, s. 118, for \"x\" (w.e.f. 1-1-2021).\n"),
            None,
        );
        let sec1 = find(&doc.akoma_ntoso.act.body, "sec_1").expect("sec_1");
        assert_eq!(
            sec1.amendments.len(),
            0,
            "{text:?} must not be read as a footnote marker"
        );
        assert_eq!(doc.footnotes[0].associated_eid, None, "{text:?}");
    }
}

#[test]
fn year_and_bare_numbers_are_not_footnote_markers() {
    for text in ["The tax was recovered in 2021.", "Nothing here matches."] {
        let doc = parse_indian_statute_from_text(
            &format!("1. First.-{text}\n1. Subs. by Act 12 of 2020, s. 118, for \"x\" (w.e.f. 1-1-2021).\n"),
            None,
        );
        let sec1 = find(&doc.akoma_ntoso.act.body, "sec_1").expect("sec_1");
        assert_eq!(sec1.amendments.len(), 0, "{text:?} must not match");
    }
}

#[test]
fn footnote_attaches_to_exactly_one_node() {
    // `A8` probe R4: three nodes contained `1` and all three got the amendment while
    // `associated_eid` named only the first.
    let doc = parse_indian_statute_from_text(
        "9. First.-A rate of 1 per cent applies.\n(1) A sub-section mentioning 1 rupee.\n10. Second.-Another rate of 1 per cent.1[he has received it.]\n1. Subs. by Act 12 of 2020, s. 118, for \"x\" (w.e.f. 1-1-2021).\n",
        None,
    );
    let owners: Vec<&str> = doc
        .provisions
        .iter()
        .filter(|node| node.amendments.len() == 1)
        .map(|node| node.eid.as_str())
        .collect();
    assert_eq!(
        owners.len(),
        1,
        "expected exactly one owner, got {owners:?}"
    );
    assert_eq!(
        owners[0], "sec_10",
        "only the node carrying the anchored 1[ marker may own the note"
    );
    assert_eq!(
        doc.footnotes[0].associated_eid.as_deref(),
        Some(owners[0]),
        "associated_eid and the per-node amendments must agree"
    );
}

#[test]
fn section_without_separator_keeps_its_body() {
    // `A5` probe P6: one section line yielded heading = whole body, text = "", citations = 0.
    let doc = parse_indian_statute_from_text(
        "50. The amount of credit shall be allowed in accordance with the credit under section 49 of this Act.\n",
        None,
    );
    let sec50 = find(&doc.akoma_ntoso.act.body, "sec_50").expect("sec_50");
    assert_eq!(sec50.heading, None);
    assert!(sec50.text.contains("section 49"));
    let cite = sec50
        .citations
        .iter()
        .find(|c| c.target_eid == "sec_49")
        .expect("the citation in the body survived");
    assert!(!cite.external);
}

#[test]
fn heading_only_section_stays_a_heading() {
    let doc = parse_indian_statute_from_text("1. Short title.\n", None);
    let sec1 = find(&doc.akoma_ntoso.act.body, "sec_1").expect("sec_1");
    assert_eq!(sec1.heading.as_deref(), Some("Short title."));
    assert_eq!(sec1.text, "");
}

#[test]
fn separator_is_chosen_by_position() {
    // `A25` probe S1. The discriminating case is a prose colon *followed by an upper-case
    // word* plus a later em-dash: the old candidate-order scan picked the em-dash and put
    // the prose in the heading, the position scan picks the colon.
    let doc = parse_indian_statute_from_text(
        "7. Power to make rules: The Government may make rules—prescribing the forms.\n",
        None,
    );
    let sec7 = find(&doc.akoma_ntoso.act.body, "sec_7").expect("sec_7");
    assert_eq!(sec7.heading.as_deref(), Some("Power to make rules"));
    assert_eq!(
        sec7.text,
        "The Government may make rules—prescribing the forms."
    );

    // A colon followed by a space is prose, not a separator, so the em-dash wins and the
    // heading runs to it. This is what the position scan plus the uppercase rule yields.
    let doc = parse_indian_statute_from_text(
        "7. Power to make rules: the Government may make rules—prescribing the forms.\n",
        None,
    );
    let sec7 = find(&doc.akoma_ntoso.act.body, "sec_7").expect("sec_7");
    assert_eq!(
        sec7.heading.as_deref(),
        Some("Power to make rules: the Government may make rules")
    );
    assert_eq!(sec7.text, "prescribing the forms.");

    // Tie-break: `.—` and `—` share an index, and the more specific separator must win.
    // `tests.rs`'s CGST section 16 depends on exactly this.
    let doc = parse_indian_statute_from_text(
        "16. Eligibility and conditions for taking input tax credit.—(1) A person may take credit.\n",
        None,
    );
    let sec16 = find(&doc.akoma_ntoso.act.body, "sec_16").expect("sec_16");
    assert_eq!(
        sec16.heading.as_deref(),
        Some("Eligibility and conditions for taking input tax credit")
    );
    assert!(find(&doc.akoma_ntoso.act.body, "sec_16__subsec_1").is_some());
}

#[test]
fn section_text_does_not_duplicate_its_subsection() {
    // `A22` probe P12: `sec_22.text` and `sec_22__subsec_1.text` used to be identical, and
    // the locator in the sentence was extracted twice.
    let sentence = "Every supplier liable under section 49 shall be registered under this Act.";
    let doc = parse_indian_statute_from_text(
        &format!("22. Persons liable for registration.—(1) {sentence}\n"),
        None,
    );
    let sec22 = find(&doc.akoma_ntoso.act.body, "sec_22").expect("sec_22");
    let sub = find(&doc.akoma_ntoso.act.body, "sec_22__subsec_1").expect("subsec_1");
    assert_eq!(sec22.text, "");
    assert_eq!(sub.text, sentence);
    let carrying: Vec<&CitationReference> = doc
        .citations
        .iter()
        .filter(|c| c.target_eid == "sec_49")
        .collect();
    assert_eq!(
        carrying.len(),
        1,
        "the locator must be extracted once, not twice: {:?}",
        doc.citations
            .iter()
            .map(|c| (c.parent_eid.as_str(), c.raw_text.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn provision_reports_every_page_it_spans() {
    // `A19` probe P4: pre-change `pages=[1]`.
    let pages = pages_of(vec![
        page(
            1,
            vec![line(1, 0, 0, "16. Credit.—(1) A person shall be eligible.")],
        ),
        page(
            2,
            vec![
                line(2, 0, 0, "The person shall file a return."),
                line(2, 1, 1, "A further sentence continues it."),
            ],
        ),
    ]);
    let doc = parse_indian_statute_from_pages(&pages, None);
    let sub = find(&doc.akoma_ntoso.act.body, "sec_16__subsec_1").expect("subsec_1");
    assert_eq!(sub.page_numbers, vec![1, 2]);
}

#[test]
fn explanation_under_a_clause_is_mintable() {
    // `A11` probe S2: pre-change the target `sec_17__subsec_2__cl_b__expl_1` was unmintable.
    let doc = parse_indian_statute_from_text(
        "17. Credit.—(2) The person shall be eligible:—\n(a) first;\n(b) second.\nExplanation 1.—For the purposes of this clause, receipt is deemed complete.\n",
        None,
    );
    let node = find(&doc.akoma_ntoso.act.body, "sec_17__subsec_2__cl_b__expl_1")
        .expect("an explanation under a clause is mintable");
    assert_eq!(node.kind, ProvisionKind::Explanation);

    // The citation the parser could already emit must now resolve against the minted shape.
    let mut citations = extract_citations(
        "sec_24",
        "see Explanation 1 to clause (b) of sub-section (2) of section 17",
    );
    assert_eq!(citations.len(), 1);
    assert_eq!(citations[0].target_eid, "sec_17__subsec_2__cl_b__expl_1");
    resolve_citations(
        &mut citations,
        &build_provision_table(&doc.akoma_ntoso.act.body),
    );
    assert!(
        citations[0].resolved,
        "probe S2 reported resolved=false for this target"
    );
}

#[test]
fn proviso_citation_naming_a_clause_targets_the_subsection() {
    let citations = extract_citations(
        "sec_24",
        "see the proviso to clause (b) of sub-section (2) of section 16",
    );
    assert_eq!(citations.len(), 1);
    assert_eq!(citations[0].target_eid, "sec_16__subsec_2__proviso_1");
    assert!(citations[0]
        .raw_text
        .contains("clause (b) of sub-section (2) of section 16"));
    assert!(
        !citations.iter().any(|c| c.target_eid.ends_with("__cl_b")),
        "the clause must not be reported as a citation in its own right: {:?}",
        citations
    );
}

#[test]
fn unnumbered_explanation_targets_the_first() {
    // A pinning test for an irreducible ambiguity, not a fix.
    let doc = parse_indian_statute_from_text(
        "16. Credit.—A person shall be eligible.\nExplanation.—The first explanation.\nExplanation.—The second explanation.\n",
        None,
    );
    assert!(find(&doc.akoma_ntoso.act.body, "sec_16__expl").is_some());
    assert!(find(&doc.akoma_ntoso.act.body, "sec_16__expl_2").is_some());
    let citations = extract_citations("sec_24", "see the Explanation to section 16");
    assert_eq!(citations[0].target_eid, "sec_16__expl");
}

#[test]
fn forward_citations_keep_their_locator() {
    // `A13` probe S7: all three reported `raw_text = "section 16"`, `span = [0,10]`.
    for (text, target) in [
        ("section 16(2)", "sec_16__subsec_2"),
        ("section 16(2)(b)", "sec_16__subsec_2__cl_b"),
        ("section 16(2)(b)(i)", "sec_16__subsec_2__cl_b__subcl_i"),
    ] {
        let citations = extract_citations("sec_24", text);
        assert_eq!(citations.len(), 1, "{text:?} -> {citations:?}");
        assert_eq!(citations[0].target_eid, target);
        assert_eq!(citations[0].raw_text, text);
        assert_eq!(citations[0].span, [0, text.chars().count()]);
    }
}

#[test]
fn forward_citation_of_an_external_act_keeps_the_instrument() {
    let citations = extract_citations("sec_24", "section 16(2) of the Customs Act, 1962");
    assert_eq!(citations.len(), 1);
    assert!(citations[0].external);
    assert_eq!(
        citations[0].target_type,
        Some(ExternalTargetType::ExternalAct)
    );
    assert!(citations[0].raw_text.contains("Customs Act"));
}

#[test]
fn mixed_clause_and_forward_citations_each_resolve() {
    let citations = extract_citations(
        "sec_24",
        "clause (b) of sub-section (2) of section 16 and section 17(3)",
    );
    assert_eq!(citations.len(), 2);
    assert_eq!(
        citations[0].raw_text,
        "clause (b) of sub-section (2) of section 16"
    );
    assert_eq!(citations[0].target_eid, "sec_16__subsec_2__cl_b");
    assert_eq!(citations[1].raw_text, "section 17(3)");
    assert_eq!(citations[1].target_eid, "sec_17__subsec_3");
}

#[test]
fn relation_is_scoped_to_the_citation_clause() {
    // `A14` / `A16`: the whole adjudication table, as assertions.
    let text = "Notwithstanding section 22 and subject to section 23, see section 24.";
    let citations = extract_citations("sec_1", text);
    let relation_of = |target: &str| {
        citations
            .iter()
            .find(|c| c.target_eid == target)
            .unwrap_or_else(|| panic!("no citation for {target}: {citations:?}"))
            .relation
    };
    assert_eq!(relation_of("sec_22"), RelationType::Notwithstanding);
    assert_eq!(relation_of("sec_23"), RelationType::SubjectTo);
    assert_eq!(relation_of("sec_24"), RelationType::Reference);

    let text = "Subject to section 22, nothing in section 23 shall apply.";
    let citations = extract_citations("sec_1", text);
    let relation_of = |target: &str| {
        citations
            .iter()
            .find(|c| c.target_eid == target)
            .unwrap_or_else(|| panic!("no citation for {target}"))
            .relation
    };
    assert_eq!(relation_of("sec_22"), RelationType::SubjectTo);
    assert_eq!(relation_of("sec_23"), RelationType::Reference);

    let text = "See section 22. Subject to section 23, section 24 applies.";
    let citations = extract_citations("sec_1", text);
    let relation_of = |target: &str| {
        citations
            .iter()
            .find(|c| c.target_eid == target)
            .unwrap_or_else(|| panic!("no citation for {target}"))
            .relation
    };
    assert_eq!(relation_of("sec_22"), RelationType::Reference);
    assert_eq!(relation_of("sec_23"), RelationType::SubjectTo);
    assert_eq!(relation_of("sec_24"), RelationType::Reference);
}

#[test]
fn subject_to_is_reachable_beside_notwithstanding() {
    // Currently impossible: `notwithstanding` earlier in the sentence made `subject to`
    // unreachable.
    let citations = extract_citations(
        "sec_1",
        "Notwithstanding section 22 and subject to section 23, see section 24.",
    );
    assert_eq!(
        citations
            .iter()
            .filter(|c| c.relation == RelationType::Notwithstanding)
            .count(),
        1
    );
    assert_eq!(
        citations
            .iter()
            .filter(|c| c.relation == RelationType::SubjectTo)
            .count(),
        1
    );
    assert_eq!(
        citations
            .iter()
            .filter(|c| c.relation == RelationType::Reference)
            .count(),
        1
    );
}

#[test]
fn decimal_rate_does_not_truncate_the_relation() {
    // The discriminating pairs have no comma: `relation_start` treats `,` as a boundary
    // (which `P13`'s own table requires — "Subject to section 22, nothing in section 23"
    // must leave section 23 a `Reference`), so with a comma present the window is cut there
    // and these read as `Reference`. Without one, the decimal is the only candidate
    // boundary and the whitespace rule rejects it, which is `A16`'s mechanism.
    for (text, expected) in [
        (
            "Notwithstanding the rate of 1.5 per cent section 22 shall not apply.",
            RelationType::Notwithstanding,
        ),
        (
            "Subject to a deposit of 2.5 per cent section 22 shall apply.",
            RelationType::SubjectTo,
        ),
    ] {
        let citations = extract_citations("sec_1", text);
        assert_eq!(citations[0].relation, expected, "for {text:?}");
    }
    // Non-decimal controls.
    for (text, expected) in [
        (
            "Notwithstanding the rate fixed section 22 shall not apply.",
            RelationType::Notwithstanding,
        ),
        (
            "Subject to a fixed deposit section 22 shall apply.",
            RelationType::SubjectTo,
        ),
    ] {
        let citations = extract_citations("sec_1", text);
        assert_eq!(citations[0].relation, expected, "for {text:?}");
    }
    // Recorded residual: `A16`'s prose quotes the comma-bearing forms as becoming
    // Notwithstanding / SubjectTo. They do not, because the comma boundary that `P13`'s
    // table depends on truncates them first. The two expectations are mutually exclusive;
    // the mechanism in the owning item wins.
    for text in [
        "Notwithstanding the rate of 1.5 per cent, nothing in section 22 shall apply.",
        "Subject to a deposit of 2.5 per cent, section 22 shall apply.",
    ] {
        let citations = extract_citations("sec_1", text);
        assert_eq!(
            citations[0].relation,
            RelationType::Reference,
            "for {text:?}"
        );
    }
}

#[test]
fn proviso_ordinals_beyond_fifth() {
    // Six provisos on one parent, then ordinals read against them.
    let mut text = String::from("16. Credit.—(2) A person shall be eligible:—\n");
    for index in 0..6 {
        text.push_str(&format!("Provided that condition {index} holds.\n"));
    }
    let doc = parse_indian_statute_from_text(&text, None);
    for ordinal in 1..=6 {
        assert!(
            find(
                &doc.akoma_ntoso.act.body,
                &format!("sec_16__subsec_2__proviso_{ordinal}")
            )
            .is_some(),
            "proviso_{ordinal} must exist"
        );
    }

    for (word, ordinal) in [("second", 2), ("fourth", 4), ("fifth", 5), ("sixth", 6)] {
        let citations = extract_citations(
            "sec_24",
            &format!("see the {word} proviso to sub-section (2) of section 16"),
        );
        assert_eq!(citations.len(), 1, "for {word:?}");
        assert_eq!(
            citations[0].target_eid,
            format!("sec_16__subsec_2__proviso_{ordinal}")
        );
        assert!(
            citations[0].raw_text.contains(word),
            "{word:?} must be inside raw_text: {:?}",
            citations[0].raw_text
        );
        let mut citations = citations;
        resolve_citations(
            &mut citations,
            &build_provision_table(&doc.akoma_ntoso.act.body),
        );
        assert!(citations[0].resolved, "{word:?} must resolve");
    }

    let citations = extract_citations("sec_24", "see the twenty-first proviso to section 16");
    assert_eq!(citations[0].target_eid, "sec_16__proviso_21");
    assert!(citations[0].raw_text.contains("twenty-first"));
}

#[test]
fn unreadable_ordinal_abstains() {
    let citations = extract_citations("sec_24", "see any other proviso to section 16");
    assert!(
        !citations
            .iter()
            .any(|c| c.target_eid.ends_with("__proviso_1")),
        "an unreadable ordinal must not point at the first proviso: {citations:?}"
    );
    assert!(
        citations.iter().any(|c| c.target_eid == "sec_16"),
        "the bytes must fall through to a bare-section link: {citations:?}"
    );
}

#[test]
fn determiner_proviso_is_the_first() {
    let citations = extract_citations("sec_24", "see the proviso to sub-section (2) of section 16");
    assert_eq!(citations[0].target_eid, "sec_16__subsec_2__proviso_1");
}

#[test]
fn rule_citation_span_includes_the_instrument() {
    let text = "in accordance with rule 36(4) of the Customs Act, 1962";
    let citations = extract_citations("sec_16__subsec_2", text);
    assert_eq!(citations.len(), 1);
    assert_eq!(citations[0].raw_text, "rule 36(4) of the Customs Act, 1962");
    assert!(citations[0].external);
    assert_eq!(citations[0].target_type, Some(ExternalTargetType::Rule));
    assert_eq!(citations[0].span, [19, text.chars().count()]);
}

#[test]
fn plural_section_references_yield_citations() {
    let citations = extract_citations("sec_1", "as referred to in sections 16 and 17 of the Act.");
    assert_eq!(citations.len(), 2);
    assert_eq!(citations[0].target_eid, "sec_16");
    assert_eq!(citations[1].target_eid, "sec_17");
    assert_eq!(citations[1].raw_text, "17");
    let text = "as referred to in sections 16 and 17 of the Act.";
    let start = text.find("17").expect("numeral");
    assert_eq!(citations[1].span, [start, start + 2]);

    let citations = extract_citations("sec_1", "under sections 16, 17 and 18.");
    assert_eq!(citations.len(), 3);
    assert_eq!(
        citations
            .iter()
            .map(|c| c.target_eid.as_str())
            .collect::<Vec<_>>(),
        vec!["sec_16", "sec_17", "sec_18"]
    );

    // Control: the spelled-out form is unchanged.
    let citations = extract_citations(
        "sec_1",
        "as referred to in section 16 and section 17 of the Act.",
    );
    assert_eq!(citations.len(), 2);
    assert_eq!(citations[0].raw_text, "section 16");
    assert_eq!(citations[1].raw_text, "section 17");
}

#[test]
fn provisions_are_flat() {
    let doc = parse_indian_statute_from_text(F_LONG, None);
    assert!(
        doc.provisions.iter().all(|entry| entry.children.is_empty()),
        "provisions must be a flat index, not a second copy of the tree"
    );
    assert_eq!(doc.provisions.len(), count_nodes(&doc.akoma_ntoso.act.body));
    assert_eids_unique(&doc);
}

#[test]
fn flat_entries_carry_the_tree_annotations() {
    let doc = parse_indian_statute_from_text(
        "16. Credit.—(1) A person shall be eligible, subject to section 49.\n(a) first;\n(b) second, subject to section 50.\n",
        None,
    );
    let mut checked = 0;
    fn walk(nodes: &[ProvisionNode], doc: &IndianStatuteDocument) -> usize {
        let mut checked = 0;
        for node in nodes {
            let flat = doc
                .provisions
                .iter()
                .find(|entry| entry.eid == node.eid)
                .unwrap_or_else(|| panic!("missing flat entry {}", node.eid));
            assert_eq!(
                flat.citations.len(),
                node.citations.len(),
                "citation parity for {}",
                node.eid
            );
            assert_eq!(
                flat.amendments.len(),
                node.amendments.len(),
                "amendment parity for {}",
                node.eid
            );
            checked += 1 + walk(&node.children, doc);
        }
        checked
    }
    checked += walk(&doc.akoma_ntoso.act.body, &doc);
    assert!(checked > 0);
}

#[test]
fn provision_table_has_one_entry_per_node() {
    let doc = parse_indian_statute_from_text(F_LONG, None);
    let nodes = count_nodes(&doc.akoma_ntoso.act.body);
    assert_eq!(
        build_provision_table(&doc.akoma_ntoso.act.body).len(),
        nodes
    );
}

#[test]
fn meta_is_recovered_from_the_long_title() {
    let doc = parse_indian_statute_from_text(F_LONG, Some("ignored.pdf"));
    let meta = &doc.akoma_ntoso.act.meta;
    assert_eq!(meta.title, "The Central Goods and Services Tax Act, 2017");
    assert_eq!(meta.act_year.as_deref(), Some("2017"));
    assert_eq!(meta.source, None);
    assert_eq!(meta.date, None);
}

#[test]
fn meta_falls_back_to_the_caller_title() {
    let doc = parse_indian_statute_from_text("1. Short title.-Text.\n", Some("X Act"));
    assert_eq!(doc.akoma_ntoso.act.meta.title, "X Act");
}

#[test]
fn meta_falls_back_to_the_neutral_title() {
    // The branch that was dead before, and the regression test for `A24`'s dead fallback.
    let doc = parse_indian_statute_from_text("1. Short title.-Text.\n", None);
    assert_eq!(doc.akoma_ntoso.act.meta.title, "Indian Bare Act");
}

#[test]
fn act_number_is_recovered_when_present() {
    let doc = parse_indian_statute_from_text(
        "THE FINANCE (NO. 2) ACT, 2017\nAn Act to amend Act 12 of 2017.\n1. Short title.-Text.\n",
        None,
    );
    assert_eq!(
        doc.akoma_ntoso.act.meta.act_number.as_deref(),
        Some("Act 12 of 2017")
    );
}

#[test]
fn period_less_gazette_note_is_separated_from_its_provision() {
    // Gazette footnotes print the number with or without a period. Invented text, shaped
    // like the period-less form: a solid note block whose label carries no full stop.
    let text = concat!(
        "16. Eligibility.—(1) Every registered person shall be entitled to credit.\n",
        "(2) No credit shall be allowed unless,—\n",
        "(b) 1[he has received the goods in accordance with rule 36(4)]\n",
        "1 Omitted \u{201C}except the State of Example\u{201D} by Act 12 of 2020, s. 118 ",
        "\u{2013} Brought into force w.e.f. 1-1-2021.\n",
    );
    let doc = parse_indian_statute_from_text(text, None);

    assert_eq!(
        doc.footnotes.len(),
        1,
        "the period-less note must separate rather than fall into the body stream"
    );
    let note = &doc.footnotes[0];
    assert_eq!(note.number, "1");
    assert_eq!(
        note.amendment.as_ref().map(|a| a.action.as_str()),
        Some("omitted"),
        "the note must be read as an amendment, not discarded"
    );

    let clause = doc
        .provisions
        .iter()
        .find(|p| p.eid == "sec_16__subsec_2__cl_b")
        .expect("clause (b)");
    assert!(
        !clause.text.contains("Brought into force"),
        "note text must not be spliced into the provision: {:?}",
        clause.text
    );
    assert!(
        !clause.text.contains("Act 12 of 2020"),
        "the amending act must not leak into the provision text"
    );
}

#[test]
fn period_less_note_does_not_capture_a_real_dash_or_numbered_paragraph() {
    // `Omitted` only opens a note when it follows a note number. A bare paragraph
    // number, and a horizontal bar used as a dash, must both stay in the body.
    let text = concat!(
        "7. Ordinary prose.—(1) The following shall apply.—\n",
        "(2) The rate is 12.5 per cent as set out in the Schedule.\n",
        "3. A later section.—This is body text, not a note.\n",
    );
    let doc = parse_indian_statute_from_text(text, None);
    assert!(
        doc.footnotes.is_empty(),
        "no note should be invented from ordinary body text"
    );
    assert!(doc.provisions.iter().any(|p| p.eid == "sec_7__subsec_2"));
    assert!(doc.provisions.iter().any(|p| p.eid == "sec_3"));
}

#[test]
fn uppercase_clause_label_is_a_clause() {
    let doc = parse_indian_statute_from_text(
        concat!(
            "16. Credit.—(2) The person shall be eligible:—\n",
            "(a) first condition;\n",
            "(A) an inserted condition;\n",
            "(B) a second inserted condition;\n",
            "(i) by hand delivery;\n",
            "(b) second condition.\n",
        ),
        None,
    );
    // `clause_eid` lower-cases, so `(A)` collides with `(a)` and `dedup_eid` makes it
    // distinct. Assert on the printed designator rather than on the eId.
    let by_num = |want: &str| {
        doc.provisions
            .iter()
            .find(|p| p.kind == super::ast::ProvisionKind::Clause && p.num == want)
    };
    let inserted = by_num("(A)").expect("(A) is a clause");
    assert!(inserted.text.contains("inserted condition"));
    assert!(
        by_num("(B)")
            .expect("(B) is a clause")
            .text
            .contains("second inserted"),
        "{:?}",
        doc.provisions
            .iter()
            .map(|p| p.num.clone())
            .collect::<Vec<_>>()
    );
    // The upper-case designator must not consume a slot in the lower-case sequence, so `(i)`
    // after `(a)`, `(A)`, `(B)` is still the first sub-clause of `(b)`.
    let sub = doc
        .provisions
        .iter()
        .find(|p| p.kind == super::ast::ProvisionKind::Subclause && p.num == "(i)")
        .expect("(i) is a sub-clause");
    assert!(sub.eid.ends_with("__cl_b__subcl_i"), "{}", sub.eid);
}

#[test]
fn unlabelled_parenthetical_line_is_preserved() {
    let doc = parse_indian_statute_from_text(
        concat!(
            "16. Credit.—(1) A person shall be eligible for the credit.\n",
            "(including the credit allowable under section 49 of this Act)\n",
        ),
        None,
    );
    let sub = doc
        .provisions
        .iter()
        .find(|p| p.eid == "sec_16__subsec_1")
        .expect("subsec_1");
    assert!(
        sub.text.contains("including the credit allowable"),
        "a non-designator bracket line was discarded: {:?}",
        sub.text
    );
}

#[test]
fn spelled_out_note_lead_separates_as_a_note() {
    // `Subs.`/`Ins.` with a period was the only lead form the alternation accepted, but a
    // gazette note prints "Substituted for" at least as often.
    let doc = parse_indian_statute_from_text(
        concat!(
            "16. Credit.—(1) A person shall be eligible for the credit.\n",
            "1. Substituted for \"the notified sum\" by Act 31 of 2018 – Brought into force w.e.f. 1st February, 2019.\n",
        ),
        None,
    );
    assert_eq!(doc.footnotes.len(), 1, "{:?}", doc.footnotes);
    assert_eq!(
        doc.footnotes[0]
            .amendment
            .as_ref()
            .map(|a| a.action.as_str()),
        Some("substituted")
    );
    for provision in &doc.provisions {
        assert!(
            !provision.text.contains("Brought into force"),
            "note text leaked into {}: {:?}",
            provision.eid,
            provision.text
        );
    }
}

#[test]
fn note_tail_joins_its_note_across_a_line_break() {
    let doc = parse_indian_statute_from_text(
        concat!(
            "16. Credit.—(1) A person shall be eligible for the credit.\n",
            "1. Subs. for \"the notified sum\" in section 7 of the Example Act, 2018\n",
            "– Brought into force w.e.f. 1st February, 2019.\n",
            "(2) Another sub-section of operative text.\n",
        ),
        None,
    );
    assert_eq!(doc.footnotes.len(), 1, "{:?}", doc.footnotes);
    assert!(
        doc.footnotes[0].raw_text.contains("1st February, 2019"),
        "the tail was cut off the note: {:?}",
        doc.footnotes[0].raw_text
    );
    for provision in &doc.provisions {
        assert!(
            !provision.text.contains("Brought into force"),
            "note text leaked into {}: {:?}",
            provision.eid,
            provision.text
        );
    }
}

#[test]
fn an_operative_sentence_is_not_a_note_tail() {
    // The dash-anchored opening is the discriminator, so prose that opens with the phrase
    // without a dash stays in the body.
    let doc = parse_indian_statute_from_text(
        "16. Credit.—(1) Brought into force on the appointed day, the credit shall apply.\n",
        None,
    );
    assert!(doc.footnotes.is_empty(), "{:?}", doc.footnotes);
    let sub = doc
        .provisions
        .iter()
        .find(|p| p.eid == "sec_16__subsec_1")
        .expect("subsec_1");
    assert!(sub.text.contains("Brought into force on the appointed day"));
    assert!(
        !doc.diagnostics
            .iter()
            .any(|d| d.code == "statute.note_tail_unattached"),
        "{:?}",
        doc.diagnostics
    );
}

#[test]
fn plural_division_headings_are_boundaries_and_prose_is_not() {
    for heading in ["SCHEDULES", "PARTS", "CHAPTERS"] {
        let doc = parse_indian_statute_from_text(
            &format!(
                "16. Credit.—(1) A person shall be eligible.\n{heading}\n17. Levy.—Tax shall be levied at the notified rate.\n"
            ),
            None,
        );
        // A schedule is its own division kind: its paragraphs restart at 1 and would
        // otherwise collide with every section numeral. A bare `SCHEDULES` carries no
        // designator and is the running head this Act prints on every schedule page, so it
        // is not an instance of a schedule.
        let expected = if heading == "SCHEDULES" {
            super::ast::ProvisionKind::Chapter
        } else if heading.starts_with("SCHEDULE") {
            super::ast::ProvisionKind::Schedule
        } else {
            super::ast::ProvisionKind::Chapter
        };
        assert!(
            doc.akoma_ntoso
                .act
                .body
                .iter()
                .any(|n| n.kind == expected && n.num == heading),
            "{heading:?} minted no {expected:?} division"
        );
        assert!(
            doc.provisions.iter().any(|p| p.eid == "sec_17"),
            "{heading:?} swallowed sec_17"
        );
    }
    for prose in [
        "Part of the value of supply shall be credited.",
        "Parts of the value of supply shall be credited.",
        "Chapter XXVII of the Code of Criminal Procedure shall apply.",
    ] {
        let doc = parse_indian_statute_from_text(
            &format!("16. Credit.—(1) A person shall be eligible.\n{prose}\n"),
            None,
        );
        assert!(
            !doc.provisions.iter().any(|p| p.eid.starts_with("chapter_")),
            "{prose:?} minted a division heading"
        );
        // `(1)` was carried onto the subsection, so the continuation lands there, not on
        // the section node whose body the separator consumed.
        let sub = doc
            .provisions
            .iter()
            .find(|p| p.eid == "sec_16__subsec_1")
            .expect("subsec_1");
        assert!(
            sub.text.contains("credited") || sub.text.contains("shall apply"),
            "{prose:?} was not kept as continuation text: {:?}",
            sub.text
        );
    }
}

// A gazette note reference is a superscript digit welded to a bracketed phrase, and the
// line carrying it also contains the typographic double quotes, which are three bytes each.
// The span offsets the extraction pass reports are character offsets, so a byte slice at
// those offsets lands mid-character. This is the shape that produced both a wrong marker
// number and a vanished phrase.
const ADJUDICATING: &str = "\u{201C}adjudicating authority\u{201D} means any authority, \
but does not include the 3[Central Board of Indirect Taxes and Customs], the Revisional \
Authority, 4[the National Appellate Authority for Advance Ruling,] and 5[the Appellate \
Authority, the Appellate Tribunal]; ";

#[test]
fn a_multibyte_character_before_a_note_reference_does_not_shift_it() {
    let pages = pages_of(vec![page(
        1,
        vec![
            line_with_superscripts(1, 0, 0, ADJUDICATING, &["3", "4", "5"]),
            line(1, 1, 1, "3 Omitted by Act 12 of 2020, s. 118."),
            line(
                1,
                2,
                2,
                "4 Substituted by Act 31 of 2018, s. 4 (w.e.f. 1-2-2019).",
            ),
            line(
                1,
                3,
                3,
                "5 Substituted by Act 31 of 2018, s. 5 (w.e.f. 1-2-2019).",
            ),
        ],
    )]);

    let doc = parse_indian_statute_from_pages(&pages, None);
    let text = &doc.provisions[0].text;

    assert_eq!(
        text.matches('[').count(),
        text.matches(']').count(),
        "unbalanced brackets: {text:?}"
    );
    for marker in ["[3]", "[4]", "[5]"] {
        assert!(text.contains(marker), "{marker} missing from {text:?}");
    }
    // Every phrase the gazette brackets must survive intact.
    for phrase in [
        "Central Board of Indirect Taxes and Customs",
        "the National Appellate Authority for Advance Ruling",
        "the Appellate Authority, the Appellate Tribunal",
    ] {
        assert!(text.contains(phrase), "{phrase:?} was lost from {text:?}");
    }
}

#[test]
fn adjacent_note_references_each_get_their_own_marker() {
    // Two superscripts printed back to back, as happens where a gazette references two notes
    // at one point. Both must survive as separate markers.
    let pages =
        pages_of(vec![page(
            1,
            vec![
            line(1, 0, 0, "1. Certain supplies.—(1) A supply of goods is treated as specified."),
            line_with_superscripts(
                1,
                1,
                1,
                "6[7[Supply of goods from a non-taxable territory to another place.] is specified.",
                &["6", "7"],
            ),
            line(1, 2, 2, "6 Substituted by Act 12 of 2020, s. 1."),
            line(1, 3, 3, "7 Substituted by Act 12 of 2020, s. 2."),
        ],
        )]);

    let doc = parse_indian_statute_from_pages(&pages, None);
    let item = doc
        .provisions
        .iter()
        .find(|p| p.text.contains("Supply of goods from a non-taxable"))
        .expect("the referenced provision");

    assert!(item.text.contains("[6]"), "{:?}", item.text);
    assert!(item.text.contains("[7]"), "{:?}", item.text);
    assert!(
        !item.text.contains("[67]"),
        "two references must not merge into one: {:?}",
        item.text
    );
    assert!(item.text.contains("to another place.] is specified"));
}

#[test]
fn a_gazette_amendment_wrapper_does_not_hide_a_section() {
    // A gazette marks an inserted or substituted heading with the amendment ordinal and an
    // opening bracket. The ordinal belongs to the amending footnote, not to the section.
    for (line, expected) in [
        (
            "26[Section 11A. Power not to recover tax not levied or short-levied.",
            "11A",
        ),
        (
            "4[Section 101A. Constitution of the National Appellate Authority.",
            "101A",
        ),
        (
            "19[Section 114. Financial and administrative powers of the President.-",
            "114",
        ),
        (
            "18[Section 20. Manner of distribution of credit by an Input Service Distributor.-",
            "20",
        ),
        // The same heading after `mark_note_references` has canonicalised the superscript
        // ordinal into a note marker, which is the form the PDF path actually delivers.
        (
            "[26][Section 11A. Power not to recover tax not levied or short-levied.",
            "11A",
        ),
        (
            "[4][Section 101A. Constitution of the National Appellate Authority.",
            "101A",
        ),
        ("[3]20. Manner of distribution of credit.", "20"),
    ] {
        let doc = parse_indian_statute_from_text(&format!("{line}\n(1) The body text.\n"), None);
        assert!(
            doc.provisions
                .iter()
                .any(|p| p.eid == format!("sec_{}", expected.to_lowercase())),
            "{line:?} did not mint sec_{expected}; got {:?}",
            doc.provisions
                .iter()
                .map(|p| p.eid.clone())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn a_bare_year_is_still_not_a_section() {
    // The wrapper allowance must not reopen the decimal/date/bare-year harms.
    for line in [
        "2019. The Government may make rules.",
        "1.5 per cent of the value.",
        "3.14. Value of supply.",
    ] {
        assert!(
            parse_indian_statute_from_text(&format!("{line}\n"), None)
                .provisions
                .iter()
                .all(|p| p.kind != super::ast::ProvisionKind::Section),
            "{line:?} minted a section"
        );
    }
}

#[test]
fn a_division_title_after_a_colon_is_still_a_boundary() {
    // A gazette prints the division title after a colon as often as after a dash. Only the
    // dash form was accepted, so `CHAPTER II : RATES` was read as body text.
    for heading in [
        "CHAPTER II : RATES",
        "PART I : PRELIMINARY",
        "SCHEDULE I : GOODS",
    ] {
        let doc = parse_indian_statute_from_text(
            &format!(
                "16. Credit.—(1) A person shall be eligible.\n{heading}\n17. Levy.—Tax shall be levied.\n"
            ),
            None,
        );
        let expected = if heading.starts_with("SCHEDULE") {
            super::ast::ProvisionKind::Schedule
        } else {
            super::ast::ProvisionKind::Chapter
        };
        assert!(
            doc.provisions.iter().any(|p| p.kind == expected),
            "{heading:?} minted no boundary; got {:?}",
            doc.provisions
                .iter()
                .map(|p| p.eid.clone())
                .collect::<Vec<_>>()
        );
        assert!(
            doc.provisions.iter().any(|p| p.eid == "sec_17"),
            "{heading:?} swallowed the following section"
        );
    }
}

#[test]
fn division_prose_with_a_colon_is_not_a_boundary() {
    for prose in [
        "Part of the value of supply: it shall be credited.",
        "Chapter XXVII of the Code of Criminal Procedure shall apply.",
    ] {
        let doc = parse_indian_statute_from_text(
            &format!("16. Credit.—(1) A person shall be eligible.\n{prose}\n"),
            None,
        );
        assert!(
            !doc.provisions.iter().any(|p| p.eid.starts_with("chapter_")),
            "{prose:?} minted a division heading"
        );
    }
}

#[test]
fn a_schedule_owns_its_numbered_paragraphs() {
    // Schedule paragraphs restart at 1, so every one collides with a section numeral.
    // Before they were a container each was rejected as a duplicate section and kept as
    // continuation text.
    let doc = parse_indian_statute_from_text(
        concat!(
            "16. Credit.—(1) A person shall be eligible.\n",
            "SCHEDULE I\n",
            "1. First matter.-Text of the first matter.\n",
            "2. Second matter.-Text of the second matter.\n",
            "17. Levy.—Tax shall be levied.\n",
        ),
        None,
    );
    let schedule = doc
        .provisions
        .iter()
        .find(|p| p.kind == super::ast::ProvisionKind::Schedule)
        .expect("SCHEDULE I is a schedule, not a chapter");
    assert_eq!(schedule.eid, "schedule_1");

    let item = doc
        .provisions
        .iter()
        .find(|p| p.kind == super::ast::ProvisionKind::ScheduleItem && p.num == "2")
        .expect("paragraph 2 is an item of the schedule");
    assert_eq!(item.eid, "schedule_1__item_2");
    assert_eq!(item.parent_eid.as_deref(), Some("schedule_1"));
    assert!(item.text.contains("second matter"));

    assert!(
        doc.provisions.iter().any(|p| p.eid == "sec_17"),
        "the schedule swallowed the section after it"
    );
    assert_eq!(codes(&doc, "statute.duplicate_provision_eid"), 0);
}

#[test]
fn schedule_ordinals_parse_from_arabic_roman_and_words() {
    for (word, expected) in [
        ("1", 1),
        ("I", 1),
        ("FIRST", 1),
        ("II", 2),
        ("SECOND", 2),
        ("III", 3),
        ("IX", 9),
        ("NINTH", 9),
    ] {
        assert_eq!(
            super::eid::parse_schedule_ordinal(word),
            Some(expected),
            "{word:?}"
        );
    }
    assert_eq!(super::eid::parse_schedule_ordinal("0"), None);
    assert_eq!(super::eid::parse_schedule_ordinal("ALPHA"), None);
}

#[test]
fn a_bare_schedules_heading_is_not_a_schedule_instance() {
    // `SCHEDULES` with no designator is this Act's running head on every schedule page.
    // Minting a schedule from it would invent one per page.
    let doc = parse_indian_statute_from_text(
        concat!(
            "16. Credit.—(1) A person shall be eligible.\n",
            "SCHEDULES\n",
            "SCHEDULE I\n",
            "1. First matter.-Text.\n",
        ),
        None,
    );
    let schedules: Vec<_> = doc
        .provisions
        .iter()
        .filter(|p| p.kind == super::ast::ProvisionKind::Schedule)
        .collect();
    assert_eq!(schedules.len(), 1, "{:?}", schedules);
    assert_eq!(schedules[0].eid, "schedule_1");
    assert_eq!(schedules[0].num, "I");
}

#[test]
fn a_wrapped_line_beginning_with_a_division_word_is_not_a_boundary() {
    // The gazette wraps body text, and a continuation can start with "Chapter" without
    // being a heading: "as per Chapter XIX-C;" is the tail of a sentence. Treating it as a
    // boundary closed the open provision and everything after it was orphaned.
    for tail in ["Chapter XIX-C;", "Chapter II.", "Part B,", "Schedule II:"] {
        let doc = parse_indian_statute_from_text(
            &format!(
                "95. Tax credit.—(4) \"advance tax\" means the advance tax payable as per\n{tail}\n(5) Another sub-section.\n"
            ),
            None,
        );
        assert!(
            !doc.provisions.iter().any(|p| p.eid.starts_with("chapter_")),
            "{tail:?} was read as a division heading"
        );
        let sub = doc
            .provisions
            .iter()
            .find(|p| p.eid == "sec_95__subsec_5")
            .expect("the sub-section after the wrapped line survives");
        assert!(sub.text.contains("Another sub-section"));
        assert!(
            doc.provisions
                .iter()
                .find(|p| p.eid == "sec_95__subsec_4")
                .expect("subsec 4 survives")
                .text
                .contains("advance tax"),
            "the wrapped tail was detached from its own sentence"
        );
    }
}

#[test]
fn a_real_division_heading_is_still_a_boundary() {
    // The terminator rule must not cost us real headings: none of these ends in
    // sentence punctuation.
    for heading in [
        "CHAPTER I",
        "CHAPTER II : RATES",
        // Written as an escape so the dash survives any editor or shell that does not
        // agree with this file about UTF-8. The marker uses the same escape for the same
        // reason: a mangled dash silently widens or narrows the class it belongs to.
        "CHAPTER II\u{2014}RATES",
        "PART A",
        "SCHEDULE I",
        "SCHEDULES",
        "CHAPTER XIX-C",
    ] {
        let doc = parse_indian_statute_from_text(
            &format!(
                "16. Credit.—(1) A person shall be eligible.\n{heading}\n17. Levy.—Tax shall be levied.\n"
            ),
            None,
        );
        assert!(
            doc.provisions.iter().any(|p| matches!(
                p.kind,
                super::ast::ProvisionKind::Chapter | super::ast::ProvisionKind::Schedule
            )),
            "{heading:?} is no longer a boundary"
        );
        assert!(
            doc.provisions.iter().any(|p| p.eid == "sec_17"),
            "{heading:?} swallowed sec_17"
        );
    }
}

#[test]
fn a_division_designator_inside_a_sentence_is_not_a_boundary() {
    // A hyphen once shared the separator class with the colon and the dashes, so everything
    // after `IV-D` was taken as a chapter title. Both of these are one sentence each, wrapped
    // so that a line happens to begin with the word; each became a heading, which closed the
    // open section and orphaned every sub-section after it.
    let cases = [
        // Section 43, book profit.
        "43. (1) \"book profit\" means the net profit, as shown in the profit\n\
         and loss account for the relevant tax year, computed as per\n\
         Chapter IV-D as increased by the aggregate amount of the\n\
         remuneration to all the partners of the firm, if such amount has been\n\
         deducted while computing the net profit;\n\
         (2) Another sub-section.\n",
        // Section 202, new tax regime, referenced mid-clause.
        "202. (1) Irrespective of anything contained in this Act other than\n\
         Chapter XVII-B but subject to Parts A, B, E and this Part of this Chapter, the\n\
         income-tax payable by a person shall be as follows:—\n\
         (a) nil, where the total income does not exceed the threshold.\n",
    ];
    for (index, body) in cases.iter().enumerate() {
        let doc = parse_indian_statute_from_text(body, None);
        assert!(
            !doc.provisions.iter().any(|p| p.eid.starts_with("chapter_")),
            "case {index} read a designator inside a sentence as a heading"
        );
    }
    let doc = parse_indian_statute_from_text(cases[0], None);
    let subsec = doc
        .provisions
        .iter()
        .find(|p| p.eid == "sec_43__subsec_2")
        .expect("the sub-section after the wrapped definition survives");
    assert!(subsec.text.contains("Another sub-section"));
    let first = doc
        .provisions
        .iter()
        .find(|p| p.eid == "sec_43__subsec_1")
        .expect("subsec 1 survives");
    assert!(
        first
            .text
            .contains("deducted while computing the net profit"),
        "the definition was truncated at the designator"
    );
}

#[test]
fn a_designator_alone_after_an_unfinished_sentence_is_not_a_boundary() {
    // The structural test cannot see this one. When the column runs out straight after the
    // connector the designator sits alone on its line and is character for character a
    // heading, so only the sentence in front of it gives it away.
    for connector in [
        "as per",
        "under",
        "in accordance with",
        "referred to in",
        "and",
    ] {
        let doc = parse_indian_statute_from_text(
            &format!(
                "95. Advance tax.—(1) The amount shall be computed {connector}\n\
                 Chapter XIX-C\n\
                 and shall be payable in the manner prescribed.\n\
                 (2) Another sub-section.\n"
            ),
            None,
        );
        assert!(
            !doc.provisions.iter().any(|p| p.eid.starts_with("chapter_")),
            "{connector:?} was read as ending a sentence"
        );
        assert!(
            doc.provisions
                .iter()
                .any(|p| p.eid == "sec_95__subsec_2" && p.text.contains("Another sub-section")),
            "{connector:?} orphaned the following sub-section"
        );
    }
}

#[test]
fn a_division_heading_after_a_complete_line_is_still_a_boundary() {
    // The carry-over guard must not swallow real headings. Each of these is preceded by a
    // line that ends a sentence outright, so none of them can be a continuation.
    for heading in [
        "CHAPTER I",
        "CHAPTER II : RATES",
        "PART A",
        "SCHEDULE I",
        "CHAPTER XIX-C",
    ] {
        let doc = parse_indian_statute_from_text(
            &format!(
                "16. Credit.—(1) A person shall be eligible.\n{heading}\n17. Levy.—Tax shall be levied.\n"
            ),
            None,
        );
        assert!(
            doc.provisions.iter().any(|p| matches!(
                p.kind,
                super::ast::ProvisionKind::Chapter | super::ast::ProvisionKind::Schedule
            )),
            "{heading:?} is no longer a boundary"
        );
        assert!(
            doc.provisions.iter().any(|p| p.eid == "sec_17"),
            "{heading:?} swallowed sec_17"
        );
    }
}

#[test]
fn a_schedule_item_keeps_its_own_text_and_its_clauses() {
    // A schedule item is a container. It used to be pushed to the document root and forgotten
    // on the spot, so nothing could be added to it afterwards: the continuation lines that
    // followed had nowhere to go and were dropped outright, and the `(a)`, `(b)`, `(i)`
    // markers escaped to the root as `sec_unknown` clauses, carrying the schedule's text far
    // from the schedule that owns it. In the Income-tax Act 2025 that was 410 orphan roots
    // holding 99,374 characters, and two lines of Schedule I that appeared nowhere at all.
    let text = "\
70. Capital gains.-(1) In this section-\n\
(a) a fund established or incorporated outside India;\n\
\n\
SCHEDULE I\n\
[See section 9(12)]\n\
CONDITIONS FOR CERTAIN ACTIVITIES NOT TO CONSTITUTE BUSINESS CONNECTION IN INDIA.\n\
1. (1) The eligible investment fund referred to in section 9(12), means a fund\n\
established or incorporated or registered outside India, which collects funds from its\n\
members for investing it for their benefit and fulfils the following conditions:-\n\
(a) the fund is not a person resident in India;\n\
(b) the fund is-\n\
(i) a resident of a country with which an agreement has been entered into; or\n\
(ii) established or registered in a country as notified in this behalf:\n\
2. In this Schedule,-\n\
(a) a first item;\n\
(b) a second item.\n";

    let doc = parse_indian_statute_from_text(text, None);

    assert!(
        !doc.provisions
            .iter()
            .any(|p| p.eid.starts_with("sec_unknown")),
        "schedule clauses escaped to the root as sec_unknown: {:?}",
        doc.provisions
            .iter()
            .map(|p| p.eid.as_str())
            .collect::<Vec<_>>()
    );

    let schedule = doc
        .akoma_ntoso
        .act
        .body
        .iter()
        .find(|p| p.kind == super::ast::ProvisionKind::Schedule)
        .expect("a schedule was opened");
    assert_eq!(schedule.children.len(), 2, "two numbered paragraphs");

    let first = &schedule.children[0];
    assert_eq!(first.eid, "schedule_1__item_1");

    // The continuation lines that used to vanish.
    for fragment in [
        "means a fund",
        "established or incorporated or registered outside India",
        "which collects funds from its",
        "members for investing it for their benefit",
        "fulfils the following conditions",
    ] {
        assert!(
            first.text.contains(fragment),
            "dropped continuation line: {fragment:?}\n  item text: {:?}",
            first.text
        );
    }

    // And the clauses that used to escape.
    assert_eq!(
        first
            .children
            .iter()
            .map(|c| c.eid.as_str())
            .collect::<Vec<_>>(),
        vec!["schedule_1__item_1__cl_a", "schedule_1__item_1__cl_b"],
        "clauses are not parented to the item"
    );
    assert_eq!(
        first.children[0].text,
        "the fund is not a person resident in India;"
    );
    assert_eq!(
        first.children[0].parent_eid.as_deref(),
        Some("schedule_1__item_1")
    );
    assert_eq!(
        first.children[1]
            .children
            .iter()
            .map(|c| c.eid.as_str())
            .collect::<Vec<_>>(),
        vec![
            "schedule_1__item_1__cl_b__subcl_i",
            "schedule_1__item_1__cl_b__subcl_ii",
        ]
    );

    // The second paragraph, whose clauses used to collide into sec_unknown__cl_a__dupN.
    let second = &schedule.children[1];
    assert_eq!(second.text, "In this Schedule,-");
    assert_eq!(
        second
            .children
            .iter()
            .map(|c| c.eid.as_str())
            .collect::<Vec<_>>(),
        vec!["schedule_1__item_2__cl_a", "schedule_1__item_2__cl_b"]
    );

    // The section before the schedule is untouched: the item is held in the section slot
    // while it is open, so a section that follows must still close it cleanly.
    // `provisions` is the flat list and carries no children; the nesting is in the tree.
    let s70 = doc
        .akoma_ntoso
        .act
        .body
        .iter()
        .find(|p| p.eid == "sec_70")
        .expect("sec_70 survives");
    assert_eq!(s70.children.len(), 1);
    assert_eq!(s70.children[0].eid, "sec_70__subsec_1");
    assert_eq!(s70.children[0].children[0].eid, "sec_70__subsec_1__cl_a");
}
