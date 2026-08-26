//! Deliberately invalid test library. Never link this into jsonmodem.

#[unsafe(no_mangle)]
pub extern "C" fn check_address_sanitizer() -> u8 {
    let bytes = vec![42_u8; std::hint::black_box(8)];
    // Deliberate out-of-bounds read: the parent test requires an ASan failure.
    unsafe { std::ptr::read_volatile(bytes.as_ptr().add(bytes.len())) }
}
