/// Decode UTF-16 from the C IME `insert_text` callback and insert it at most once.
///
/// Valid UTF-16 inserts the decoded string. Malformed UTF-16 returns the decode
/// error and inserts nothing — including no leftover empty string.
pub(crate) fn apply_insert_text(
    slice: &[u16],
    insert: impl FnOnce(String),
) -> Result<(), std::string::FromUtf16Error> {
    match String::from_utf16(slice) {
        Ok(s) => {
            insert(s);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::apply_insert_text;

    fn record_inserts(slice: &[u16]) -> (Vec<String>, bool) {
        let mut inserts = Vec::new();
        let ok = apply_insert_text(slice, |s| inserts.push(s)).is_ok();
        (inserts, ok)
    }

    #[test]
    fn valid_utf16_inserts_decoded_string_exactly_once() {
        let utf16: Vec<u16> = "hello".encode_utf16().collect();
        let (inserts, ok) = record_inserts(&utf16);
        assert!(ok);
        assert_eq!(inserts, ["hello"]);
    }

    #[test]
    fn malformed_utf16_does_not_insert_empty_fallback() {
        // Unpaired high surrogate: invalid UTF-16, must not insert "" leftover.
        let (inserts, ok) = record_inserts(&[0xD800]);
        assert!(!ok);
        assert!(
            inserts.is_empty(),
            "malformed UTF-16 must not insert, got {inserts:?}"
        );
    }
}
