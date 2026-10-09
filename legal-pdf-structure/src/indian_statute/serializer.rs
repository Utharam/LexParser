use super::ast::IndianStatuteDocument;

/// Serializes an `IndianStatuteDocument` into a JSON string formatted according to the AKN-flavored schema.
pub fn to_akn_json_string(
    document: &IndianStatuteDocument,
    pretty: bool,
) -> serde_json::Result<String> {
    if pretty {
        serde_json::to_string_pretty(document)
    } else {
        serde_json::to_string(document)
    }
}
