use alloc::vec::Vec;

/// Split `payload` into approximately equal-sized chunks without
/// breaking UTF-8 code points.
///
/// # Panics
///
/// Panics if `parts` is zero.
#[must_use]
pub fn produce_chunks(payload: &str, parts: usize) -> Vec<&str> {
    assert!(parts > 0);
    let len = payload.len();
    let chunk_size = len.div_ceil(parts);
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < len {
        let mut end = core::cmp::min(start + chunk_size, len);
        while end < len && !payload.is_char_boundary(end) {
            end += 1;
        }
        chunks.push(&payload[start..end]);
        start = end;
    }
    chunks
}

/// Return a sequence of prefixes converging to `payload`.
///
/// # Panics
///
/// Panics if `parts` is zero.
#[must_use]
pub fn produce_prefixes(payload: &str, parts: usize) -> Vec<&str> {
    let chunks = produce_chunks(payload, parts);
    let mut prefixes = Vec::with_capacity(chunks.len());
    let mut end = 0;
    for chunk in chunks {
        end += chunk.len();
        prefixes.push(&payload[..end]);
    }
    prefixes
}
