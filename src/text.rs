//! Character classes shared by the sanitisers in `evidence`, `run` and
//! `output`.

/// Invisible formatting characters that `char::is_control` misses: bidi
/// embeddings, overrides, isolates and marks (U+061C, U+200E/F,
/// U+202A–U+202E, U+2066–U+2069), zero-width and invisible characters
/// (U+180E, U+200B–U+200D, U+2060–U+2064, U+FEFF), and the Unicode
/// line/paragraph separators (U+2028/U+2029). Subject-derived text can use
/// them to reorder or hide what a terminal displays ("Trojan Source"
/// style), so they are treated like control characters.
pub(crate) fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_bidi_and_zero_width_only() {
        for c in [
            '\u{061C}', '\u{180E}', '\u{200B}', '\u{200D}', '\u{200E}', '\u{200F}', '\u{2028}',
            '\u{2029}', '\u{202A}', '\u{202E}', '\u{2060}', '\u{2066}', '\u{2069}', '\u{FEFF}',
        ] {
            assert!(is_invisible_format(c), "{:?} should be flagged", c);
        }
        for c in ['a', 'é', 'ü', '\u{2027}', '\u{2065}', '\u{2070}', ' ', '-'] {
            assert!(!is_invisible_format(c), "{:?} should not be flagged", c);
        }
    }
}
