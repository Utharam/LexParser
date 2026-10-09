//! Text repair for gazetted statute PDFs whose font character map misreports
//! quotation mark glyphs.
//!
//! The observed defect: one font emits its double quotation glyphs as U+2015
//! HORIZONTAL BAR and U+2016 DOUBLE VERTICAL LINE. U+2016 has no legitimate use
//! in prose, so it is always repaired; U+2015 is a real character, so it is only
//! reinterpreted once [`has_broken_quotation_font`] has confirmed the document.
//!
//! Pairing is deliberately not used as evidence. Across a 351-section Act the
//! wrong glyph counts balance exactly, yet quotes also open in one provision and
//! close in another, so a rule demanding a partner inside the same string would
//! drop real ones. The repair is a per-character map and looks at one line at a
//! time.

use std::borrow::Cow;

const BAR: char = '\u{2015}';
const VERTICAL_LINE: char = '\u{2016}';
const LEFT_DOUBLE_QUOTE: char = '\u{201c}';
const RIGHT_DOUBLE_QUOTE: char = '\u{201d}';

/// A character map is chosen once for a whole document, so a real defect hits
/// every quotation mark in it. A handful of hits is equally consistent with
/// genuine bars and stray glyphs, and a genuine bar rewritten into a quote
/// cannot be undone downstream, so the signature must be well established.
const MIN_SIGNAL_HITS: usize = 8;

/// A mis-mapped font emits nearly every double quotation as a wrong glyph, so
/// correct U+201C/U+201D marks that survive came from some other font. Once real
/// double quotes reach this share of the bar count the bars are likelier to be
/// genuine, and the signature is rejected.
const REAL_QUOTE_FACTOR: usize = 4;

#[derive(Default)]
struct GlyphCounts {
    bars: usize,
    verticals: usize,
    real_quotes: usize,
}

fn count_glyphs(document_text: &str) -> GlyphCounts {
    let mut counts = GlyphCounts::default();
    for character in document_text.chars() {
        match character {
            BAR => counts.bars += 1,
            VERTICAL_LINE => counts.verticals += 1,
            LEFT_DOUBLE_QUOTE | RIGHT_DOUBLE_QUOTE => counts.real_quotes += 1,
            _ => {}
        }
    }
    counts
}

/// Decides whether a document carries the broken quotation font signature.
///
/// Changes nothing. Run once over the whole concatenated document, then pass the
/// verdict to [`rewrite_quotation_glyphs`] for each line.
pub fn has_broken_quotation_font(document_text: &str) -> bool {
    let counts = count_glyphs(document_text);
    // U+2016 cannot occur in prose at all, so its presence is the anchor; the
    // threshold rejects a stray glyph or two; the ratio rejects a document that
    // has both a working quote map and genuine bars.
    counts.verticals > 0
        && counts.bars + counts.verticals >= MIN_SIGNAL_HITS
        && counts.real_quotes * REAL_QUOTE_FACTOR <= counts.bars
}

/// Repairs the quotation glyphs of one string.
///
/// U+2016 DOUBLE VERTICAL LINE always becomes U+201D RIGHT DOUBLE QUOTATION
/// MARK. U+2015 HORIZONTAL BAR becomes U+201C LEFT DOUBLE QUOTATION MARK **only**
/// when `broken_font` is `true`, and that flag must come from
/// [`has_broken_quotation_font`] run over the whole document: a PDF that uses
/// U+2015 as a real bar would otherwise have its bars silently turned into
/// quotes. Every other character, including U+2013 EN DASH and U+2014 EM DASH,
/// passes through untouched, and the borrow is returned unchanged when there is
/// no wrong glyph to repair.
///
/// The rarer `‘‘` (U+2018 doubled) artifact is left alone: the same shape occurs
/// for a genuine doubled single quote, so no rule here separates the two.
pub fn rewrite_quotation_glyphs(text: &str, broken_font: bool) -> Cow<'_, str> {
    if !text.contains(BAR) && !text.contains(VERTICAL_LINE) {
        return Cow::Borrowed(text);
    }
    let mut rewritten = String::with_capacity(text.len());
    for character in text.chars() {
        rewritten.push(match character {
            VERTICAL_LINE => RIGHT_DOUBLE_QUOTE,
            BAR if broken_font => LEFT_DOUBLE_QUOTE,
            other => other,
        });
    }
    Cow::Owned(rewritten)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    // Shape only: interleaved wrong glyphs standing in for the broken gazette
    // font, with every phrase invented.
    const WRONG_GLYPH_LINES: [&str; 8] = [
        "Section 2.- \u{2015}chargeable event\u{2016} has the meaning given in section 3.",
        "Section 4.- \u{2015}designated port\u{2016} is the port so nominated.",
        "Section 6.- \u{2015}assessable value\u{2016} has the value stated in the rules.",
        "Section 8.- \u{2015}competent officer\u{2016} means the officer appointed under rule 5.",
        "Section 10.- \u{2015}levy on goods\u{2016} shall be levied at the notified rate.",
        "Section 12.- \u{2015}place of supply\u{2016} is the place stated in the notification.",
        "Section 14.- \u{2015}registered dealer\u{2016} holds a registration in force.",
        "Section 16.- \u{2015}input credit\u{2016} is the credit allowed in section 17.",
    ];

    // Well past the threshold on bars alone, so the absent U+2016 is the only
    // thing keeping this document out of the gate.
    const REAL_BAR_LINES: [&str; 8] = [
        "Extent A\u{2015}B is the whole of the block so marked.",
        "The margin key runs A\u{2015}B\u{2015}C down the outer edge.",
        "A rule panel ends with A\u{2015}B\u{2015}C\u{2015}.",
        "The bracket A\u{2015}B opens the first column.",
        "The bracket B\u{2015}C closes the second column.",
        "Marginalia carry A\u{2015}B beside every clause.",
        "The ruled line reads A\u{2015}B in the schedule.",
        "The legend uses A\u{2015}B as a placeholder.",
    ];

    #[test]
    fn rewrites_a_document_carrying_the_broken_font() {
        let document = WRONG_GLYPH_LINES.join("\n");
        assert!(has_broken_quotation_font(&document));
        assert_eq!(
            rewrite_quotation_glyphs(WRONG_GLYPH_LINES[0], true),
            "Section 2.- \u{201c}chargeable event\u{201d} has the meaning given in section 3."
        );
        for line in WRONG_GLYPH_LINES {
            let rewritten = rewrite_quotation_glyphs(line, true);
            assert!(!rewritten.contains(BAR), "{rewritten}");
            assert!(!rewritten.contains(VERTICAL_LINE), "{rewritten}");
            assert_eq!(
                rewritten.matches(LEFT_DOUBLE_QUOTE).count(),
                1,
                "{rewritten}"
            );
            assert_eq!(
                rewritten.matches(RIGHT_DOUBLE_QUOTE).count(),
                1,
                "{rewritten}"
            );
        }
    }

    #[test]
    fn gate_rejects_a_document_with_no_double_vertical_lines() {
        let document = REAL_BAR_LINES.join("\n");
        assert!(document.matches(BAR).count() >= MIN_SIGNAL_HITS);
        assert!(!has_broken_quotation_font(&document));
    }

    #[test]
    fn leaves_a_real_horizontal_bar_document_untouched() {
        let broken = has_broken_quotation_font(&REAL_BAR_LINES.join("\n"));
        assert!(!broken);
        for line in REAL_BAR_LINES {
            assert_eq!(rewrite_quotation_glyphs(line, broken), line);
        }
    }

    #[test]
    fn leaves_a_document_with_correct_quotation_marks_unchanged() {
        let mut document = String::new();
        for line in WRONG_GLYPH_LINES {
            let fixed = line
                .replace(BAR, "\u{201c}")
                .replace(VERTICAL_LINE, "\u{201d}");
            document.push_str(&fixed);
            document.push('\n');
        }
        let broken = has_broken_quotation_font(&document);
        assert!(!broken);
        for line in document.lines() {
            assert_eq!(rewrite_quotation_glyphs(line, broken), line);
        }
    }

    #[test]
    fn gate_rejects_a_document_whose_quotes_are_genuinely_mapped() {
        // Clears the threshold and carries verticals, so only the share of real
        // double quotes keeps this document out of the gate.
        let mut document = String::new();
        for index in 1..=6 {
            let line = format!(
                "Section {index}: \u{2015}spare entry\u{2016} and \u{201c}genuine phrase\u{201d}."
            );
            document.push_str(&line);
            document.push('\n');
        }
        assert!(document.contains(VERTICAL_LINE));
        assert!(!has_broken_quotation_font(&document));
    }

    #[test]
    fn repairs_a_quote_that_opens_in_one_provision_and_closes_in_another() {
        let opening = "Section 30.- (1) Where a person refers to \u{2015}movable goods";
        let closing = "the words carry the meaning given in the next sub-section\u{2016}";
        assert!(!opening.contains(VERTICAL_LINE));
        assert!(!closing.contains(BAR));

        assert_eq!(
            rewrite_quotation_glyphs(opening, true),
            "Section 30.- (1) Where a person refers to \u{201c}movable goods"
        );
        assert_eq!(
            rewrite_quotation_glyphs(closing, true),
            "the words carry the meaning given in the next sub-section\u{201d}"
        );
    }

    #[test]
    fn keeps_en_dash_and_em_dash_untouched() {
        let line = "\u{2015}levy on goods\u{2016} for 2013\u{2013}2014 \u{2014} the rates fixed.";
        assert_eq!(
            rewrite_quotation_glyphs(line, true),
            "\u{201c}levy on goods\u{201d} for 2013\u{2013}2014 \u{2014} the rates fixed."
        );
    }

    #[test]
    fn borrows_the_input_when_there_is_no_wrong_glyph() {
        let plain = "Section 2.- \"chargeable event\" has the meaning given in section 3.";
        assert!(plain.is_ascii());
        assert!(!has_broken_quotation_font(plain));
        assert!(matches!(
            rewrite_quotation_glyphs(plain, true),
            Cow::Borrowed(_)
        ));
        assert_eq!(rewrite_quotation_glyphs(plain, true), plain);
    }
}
