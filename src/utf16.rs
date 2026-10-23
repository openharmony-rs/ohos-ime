fn is_high_surrogate(unit: u16) -> bool {
    (0xD800..=0xDBFF).contains(&unit)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

/// Returns the last `max_len` UTF-16 code units of `text`, without splitting a surrogate pair.
pub(crate) fn tail(text: &str, max_len: usize) -> Vec<u16> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut start = units.len().saturating_sub(max_len);
    if units.get(start).copied().is_some_and(is_low_surrogate) {
        start += 1;
    }
    units[start..].to_vec()
}

/// Returns the first `max_len` UTF-16 code units of `text`, without splitting a surrogate pair.
pub(crate) fn head(text: &str, max_len: usize) -> Vec<u16> {
    let mut units: Vec<u16> = text
        .encode_utf16()
        .take(max_len.saturating_add(1))
        .collect();
    if units.len() > max_len {
        units.truncate(max_len);
        if units.last().copied().is_some_and(is_high_surrogate) {
            units.pop();
        }
    }
    units
}

/// Encodes `text` as UTF-16 followed by a NUL terminator.
#[cfg(feature = "api-20")]
pub(crate) fn nul_terminated(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Copies `units` into a buffer provided by the framework and stores the copied length.
///
/// # Safety
///
/// `length` must be valid for writes. If `units` is not empty, `text` must be valid for
/// writing `units.len()` code units.
pub(crate) unsafe fn write(units: &[u16], text: *mut u16, length: *mut usize) {
    if !units.is_empty() {
        // SAFETY: Guaranteed by the caller.
        unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), text, units.len()) };
    }
    // SAFETY: Guaranteed by the caller.
    unsafe { length.write(units.len()) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_the_end() {
        assert_eq!(tail("hello", 3), "llo".encode_utf16().collect::<Vec<_>>());
        assert_eq!(tail("hi", 5), "hi".encode_utf16().collect::<Vec<_>>());
        assert!(tail("hello", 0).is_empty());
    }

    #[test]
    fn tail_does_not_split_surrogate_pairs() {
        // U+1F600 is encoded as two code units.
        assert_eq!(tail("a😀", 1), Vec::<u16>::new());
        assert_eq!(tail("a😀", 2), "😀".encode_utf16().collect::<Vec<_>>());
        assert_eq!(tail("😀b", 2), vec![u16::from(b'b')]);
    }

    #[test]
    fn head_keeps_the_start() {
        assert_eq!(head("hello", 3), "hel".encode_utf16().collect::<Vec<_>>());
        assert_eq!(head("hi", 5), "hi".encode_utf16().collect::<Vec<_>>());
        assert!(head("hello", 0).is_empty());
    }

    #[test]
    fn head_does_not_split_surrogate_pairs() {
        assert_eq!(head("😀a", 1), Vec::<u16>::new());
        assert_eq!(head("😀a", 2), "😀".encode_utf16().collect::<Vec<_>>());
        assert_eq!(head("b😀", 2), vec![u16::from(b'b')]);
    }

    #[test]
    #[cfg(feature = "api-20")]
    fn nul_terminated_appends_nul() {
        assert_eq!(
            nul_terminated("ab"),
            vec![u16::from(b'a'), u16::from(b'b'), 0]
        );
        assert_eq!(nul_terminated(""), vec![0]);
    }
}
