/// Controls buffering behaviour for adapters.
///
/// The API has been simplified: string buffering policy and non‑scalar emission
/// policy are no longer configurable at the crate level.
///
/// - String buffering (prefixes) is always enabled for the string‑accumulating
///   and raw assemblers.
/// - Non‑scalar values (arrays/objects) are always attached by assemblers that
///   build composite values (e.g., std values, raw), and never attached by the
///   string‑only assembler.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct BufferOptions {
    _private: (),
}
