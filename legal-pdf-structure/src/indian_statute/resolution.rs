use super::ast::{CitationReference, ProvisionNode};
use std::collections::HashSet;

/// Collects all provision eIds in the document tree into a set for fast cross-referencing.
pub fn build_provision_table(provisions: &[ProvisionNode]) -> HashSet<String> {
    let mut table = HashSet::new();
    fn visit(node: &ProvisionNode, table: &mut HashSet<String>) {
        table.insert(node.eid.clone());
        for child in &node.children {
            visit(child, table);
        }
    }
    for provision in provisions {
        visit(provision, &mut table);
    }
    table
}

/// Resolves a list of citation references against the document's provision table.
///
/// Rules:
/// - If `citation.external == true`:
///     `resolved: false`, `external: true`, and `target_type` is preserved.
/// - If internal (not external):
///     - If target eId is present in `provision_table`: `resolved: true`, `external: false`.
///     - If target eId is absent from `provision_table`: `resolved: false`, `external: false` (broken internal link).
pub fn resolve_citations(citations: &mut [CitationReference], provision_table: &HashSet<String>) {
    for cite in citations {
        if cite.external {
            cite.resolved = false;
        } else if provision_table.contains(&cite.target_eid) {
            cite.resolved = true;
            cite.external = false;
        } else {
            // A target eId absent from the table is an unresolved internal link. Walking to
            // an ancestor provision is a *different* resolution policy and is not
            // implemented; do not imply it here. (`A23`)
            cite.resolved = false;
            cite.external = false;
        }
    }
}

/// Resolves citations throughout the entire provision hierarchy.
pub fn resolve_provision_tree_citations(
    nodes: &mut [ProvisionNode],
    provision_table: &HashSet<String>,
) {
    for node in nodes {
        resolve_citations(&mut node.citations, provision_table);
        resolve_provision_tree_citations(&mut node.children, provision_table);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indian_statute::ast::{ExternalTargetType, ProvisionKind, RelationType};

    #[test]
    fn resolves_internal_references_correctly() {
        let sec22 = ProvisionNode {
            eid: "sec_22__subsec_1".to_owned(),
            kind: ProvisionKind::Subsection,
            num: "(1)".to_owned(),
            heading: None,
            text: "Every person...".to_owned(),
            parent_eid: Some("sec_22".to_owned()),
            children: vec![],
            citations: vec![],
            amendments: vec![],
            page_numbers: vec![1],
        };

        let root = ProvisionNode {
            eid: "sec_22".to_owned(),
            kind: ProvisionKind::Section,
            num: "22".to_owned(),
            heading: Some("Persons liable for registration".to_owned()),
            text: "Persons liable...".to_owned(),
            parent_eid: None,
            children: vec![sec22],
            citations: vec![],
            amendments: vec![],
            page_numbers: vec![1],
        };

        let table = build_provision_table(&[root.clone()]);
        assert!(table.contains("sec_22"));
        assert!(table.contains("sec_22__subsec_1"));

        let mut citations = vec![
            CitationReference {
                parent_eid: "sec_24".to_owned(),
                raw_text: "sub-section (1) of section 22".to_owned(),
                span: [0, 30],
                target_eid: "sec_22__subsec_1".to_owned(),
                relation: RelationType::Notwithstanding,
                resolved: false,
                external: false,
                target_type: None,
            },
            CitationReference {
                parent_eid: "sec_24".to_owned(),
                raw_text: "section 999".to_owned(),
                span: [35, 46],
                target_eid: "sec_999".to_owned(),
                relation: RelationType::Reference,
                resolved: false,
                external: false,
                target_type: None,
            },
            CitationReference {
                parent_eid: "sec_24".to_owned(),
                raw_text: "rule 36(4)".to_owned(),
                span: [50, 60],
                target_eid: "rule_36__subrule_4".to_owned(),
                relation: RelationType::Reference,
                resolved: false,
                external: true,
                target_type: Some(ExternalTargetType::Rule),
            },
        ];

        resolve_citations(&mut citations, &table);

        // Found internal target:
        assert!(citations[0].resolved);
        assert!(!citations[0].external);

        // Broken internal target:
        assert!(!citations[1].resolved);
        assert!(!citations[1].external);

        // External rule target:
        assert!(!citations[2].resolved);
        assert!(citations[2].external);
        assert_eq!(citations[2].target_type, Some(ExternalTargetType::Rule));
    }
}
