# JSON Unicode Escape Handling: Decode/Encode Modes and Test Plan

Scope: define decode/encode behavior options for a new JSON parser focusing on Unicode escape sequences ("\uXXXX") and UTF-16 surrogate pair handling, plus a comprehensive test catalog.

Goals
- Strict, standards-compliant default (rejects lone surrogates, validates escapes).
- Compatibility modes that match popular parsers (Node, Python), without complicating the public API.
- Predictable behavior across streaming and non-streaming decoders.

Non-Goals
- Rewriting general JSON grammar; scope is Unicode escape, surrogate handling, and serialization choices.

Note
- This document focuses on decode/encode behavior. For the ongoing Scanner refactor of the streaming parser internals, see critic_proposal.md and the task checklist in critic_todo.md.

## Terminology
- High surrogate: U+D800–U+DBFF (inclusive).
- Low surrogate: U+DC00–U+DFFF (inclusive).
- Valid pair: a high surrogate immediately followed by a low surrogate, representing one non‑BMP scalar (U+10000–U+10FFFF).

## Decode API

Public surface (suggested):

- enum DecodeMode { StrictUnicode, SurrogatePreserving, ReplaceInvalid }
- struct DecodeOptions {
  - mode: DecodeMode
  - allow_uppercase_U: bool (default false)
  - allow_short_hex: bool (default false)
  - allow_trailing_commas: bool (out of scope here, default false)
}
- enum OutputStringKind { Utf8String, Utf16Units, Wtf8String }
- output_string_kind: OutputStringKind (default Utf8String)

Rationale and behavior:
- StrictUnicode: Join valid pairs; error on any unpaired surrogate; error on invalid escapes.
- SurrogatePreserving: Join valid pairs; preserve any unpaired surrogate as-is.
  - Requires an output representation that can carry surrogates:
    - Utf16Units (Vec<u16>) or
    - WTF‑8 wrapper if you want to stay in byte/UTF‑8 domain.
  - If `Utf8String` is selected, SurrogatePreserving MUST be internally promoted to ReplaceInvalid (documented).
- ReplaceInvalid: Join valid pairs; replace any unpaired surrogate with U+FFFD.

Derived internal flags (from mode):
- join_pairs = true (always)
- allow_unpaired = (mode != StrictUnicode)
- unpaired_action = error | preserve | replace_with_U+FFFD
- validate_scalar_range = true (reject > U+10FFFF)
- escape_syntax: requires lowercase 'u', exactly 4 hex digits (unless allow_uppercase_U / allow_short_hex)

Notes:
- JSON allows hex digits in either case; the 'u' introducer is lowercase in the grammar; most parsers reject uppercase 'U'. Keep `allow_uppercase_U=false` by default.
- Streaming decoders must buffer at most one code unit of lookahead to resolve a surrogate pair. When a high surrogate is seen, delay emission until next escape (or code unit) is known.

## Encode API

Public surface (suggested):

- enum EncodeMode { EncodeStrictUnicode, EncodeSurrogatesEscaped, EncodeReplaceInvalid }
- struct EncodeOptions {
  - mode: EncodeMode
  - ascii_only: bool (default false)  // like Python ensure_ascii
  - escape_solidus: bool (default false)
  - hex_uppercase: bool (default false) // for emitted hex digits only
}

Rationale and behavior:
- EncodeStrictUnicode: Reject input strings containing unpaired surrogates. Emit valid scalars as raw UTF‑8 unless `ascii_only`.
- EncodeSurrogatesEscaped: Allow inputs with unpaired surrogates; escape them as `\uD8xx/\uDCxx` pairs or single `\uXXXX` code units, matching Python/Node behavior.
- EncodeReplaceInvalid: Replace unpaired surrogates with U+FFFD before encoding.

Notes:
- Rust/UTF‑8 strings cannot contain surrogates. If your host language string type is UTF‑8, EncodeSurrogatesEscaped may be unrepresentable; expose it only for UTF‑16/byte-buffer inputs or provide a `Utf16Units` serializer.

## Error Taxonomy (Decode)
- json_invalid_escape: bad introducer (e.g., `\UD83D`), non-hex digit, or short length.
- json_unexpected_eof_in_escape: truncated `\u` sequence.
- json_lone_leading_surrogate: high surrogate not paired.
- json_lone_trailing_surrogate: low surrogate without preceding high.
- json_scalar_out_of_range: code point > U+10FFFF.

## Error Taxonomy (Encode)
- json_encode_surrogate_disallowed: unpaired surrogate in EncodeStrictUnicode.

## Test Catalog
Each test specifies JSON input (for decode) or input string contents (for encode) and expected results by mode.

Legend: ok → accepted with described output; error → specified error code.

### Decode Tests (JSON inputs)

1. valid_pair_grinning_face
- JSON: "\uD83D\uDE00"
- StrictUnicode: ok → U+1F600
- SurrogatePreserving: ok → U+1F600
- ReplaceInvalid: ok → U+1F600

2. valid_pair_smile
- JSON: "\uD83D\uDE0A"
- StrictUnicode: ok → U+1F60A
- SurrogatePreserving: ok → U+1F60A
- ReplaceInvalid: ok → U+1F60A

3. emoji_literal
- JSON: "😀"
- StrictUnicode: ok → U+1F600
- SurrogatePreserving: ok → U+1F600
- ReplaceInvalid: ok → U+1F600

4. lone_high
- JSON: "\uD83D"
- StrictUnicode: error → json_unexpected_eof_in_escape (or json_lone_leading_surrogate if complete)
- SurrogatePreserving: ok → U+D83D (surrogate)
- ReplaceInvalid: ok → U+FFFD

5. lone_low
- JSON: "\uDE00"
- StrictUnicode: error → json_lone_trailing_surrogate
- SurrogatePreserving: ok → U+DE00 (surrogate)
- ReplaceInvalid: ok → U+FFFD

6. reversed_pair
- JSON: "\uDE00\uD83D"
- StrictUnicode: error → json_lone_trailing_surrogate
- SurrogatePreserving: ok → U+DE00, U+D83D (surrogates)
- ReplaceInvalid: ok → U+FFFD, U+FFFD

7. high_then_letter
- JSON: "\uD83D\u0041"
- StrictUnicode: error → json_lone_leading_surrogate
- SurrogatePreserving: ok → U+D83D, 'A'
- ReplaceInvalid: ok → U+FFFD, 'A'

8. letter_then_low
- JSON: "\u0041\uDE00"
- StrictUnicode: error → json_lone_trailing_surrogate
- SurrogatePreserving: ok → 'A', U+DE00
- ReplaceInvalid: ok → 'A', U+FFFD

9. high_high
- JSON: "\uD83D\uD83D"
- StrictUnicode: error → json_lone_leading_surrogate
- SurrogatePreserving: ok → U+D83D, U+D83D
- ReplaceInvalid: ok → U+FFFD, U+FFFD

10. low_low
- JSON: "\uDE00\uDE00"
- StrictUnicode: error → json_lone_trailing_surrogate
- SurrogatePreserving: ok → U+DE00, U+DE00
- ReplaceInvalid: ok → U+FFFD, U+FFFD

11. invalid_escape_hex
- JSON: "\uD83G"
- StrictUnicode: error → json_invalid_escape
- SurrogatePreserving: error → json_invalid_escape
- ReplaceInvalid: error → json_invalid_escape

12. uppercase_U_escape
- JSON: "\UD83D\UDE00"
- StrictUnicode: error → json_invalid_escape
- SurrogatePreserving: error → json_invalid_escape
- ReplaceInvalid: error → json_invalid_escape
- If `allow_uppercase_U=true`: treat like valid lowercase 'u'.

13. mixed_case_hex_digits
- JSON: "\uD83d\uDe00" (hex digits mixed-case)
- StrictUnicode: ok → U+1F600
- SurrogatePreserving: ok → U+1F600
- ReplaceInvalid: ok → U+1F600

14. nul_escape
- JSON: "\u0000"
- StrictUnicode: ok → U+0000
- SurrogatePreserving: ok → U+0000
- ReplaceInvalid: ok → U+0000

15. boundary_high_min
- JSON: "\uD800"
- StrictUnicode: error → json_lone_leading_surrogate
- SurrogatePreserving: ok → U+D800
- ReplaceInvalid: ok → U+FFFD

16. boundary_high_max
- JSON: "\uDBFF"
- StrictUnicode: error → json_lone_leading_surrogate
- SurrogatePreserving: ok → U+DBFF
- ReplaceInvalid: ok → U+FFFD

17. boundary_low_min
- JSON: "\uDC00"
- StrictUnicode: error → json_lone_trailing_surrogate
- SurrogatePreserving: ok → U+DC00
- ReplaceInvalid: ok → U+FFFD

18. boundary_low_max
- JSON: "\uDFFF"
- StrictUnicode: error → json_lone_trailing_surrogate
- SurrogatePreserving: ok → U+DFFF
- ReplaceInvalid: ok → U+FFFD

19. truncated_escape_length
- JSON: "\uD83"
- StrictUnicode: error → json_invalid_escape (short length)
- SurrogatePreserving: error → json_invalid_escape
- ReplaceInvalid: error → json_invalid_escape

20. pair_split_across_stream_chunks (streaming)
- Input arrives as "\uD83D" then next chunk "\uDE00" without intervening characters
- StrictUnicode: ok → U+1F600 (decoder must buffer and join across boundaries)
- SurrogatePreserving: ok → U+1F600
- ReplaceInvalid: ok → U+1F600

### Encode Tests (input contents → serialized JSON string)
Assume ASCII escaping policy is controlled by `ascii_only`.

1. scalar_grinning_face
- Input: U+1F600
- EncodeStrictUnicode: ok → raw "😀" (or "\ud83d\ude00" if ascii_only)
- EncodeSurrogatesEscaped: ok → same as above
- EncodeReplaceInvalid: ok → same as above

2. scalar_smile
- Input: U+1F60A
- Expected: same pattern as above

3. nul_character
- Input: U+0000
- All modes: ok → "\u0000" (may also allow raw NUL in memory; JSON must escape it)

4. lone_high
- Input: U+D83D (surrogate)
- EncodeStrictUnicode: error → json_encode_surrogate_disallowed
- EncodeSurrogatesEscaped: ok → "\ud83d"
- EncodeReplaceInvalid: ok → "\ufffd"

5. lone_low
- Input: U+DE00 (surrogate)
- EncodeStrictUnicode: error → json_encode_surrogate_disallowed
- EncodeSurrogatesEscaped: ok → "\ude00"
- EncodeReplaceInvalid: ok → "\ufffd"

6. reversed_pair
- Input: U+DE00 U+D83D
- EncodeStrictUnicode: error → json_encode_surrogate_disallowed
- EncodeSurrogatesEscaped: ok → "\ude00\ud83d"
- EncodeReplaceInvalid: ok → "\ufffd\ufffd"

7. high_then_letter
- Input: U+D83D, 'A'
- EncodeStrictUnicode: error → json_encode_surrogate_disallowed
- EncodeSurrogatesEscaped: ok → "\ud83dA"
- EncodeReplaceInvalid: ok → "\ufffdA"

8. letter_then_low
- Input: 'A', U+DE00
- EncodeStrictUnicode: error → json_encode_surrogate_disallowed
- EncodeSurrogatesEscaped: ok → "A\ude00"
- EncodeReplaceInvalid: ok → "A\ufffd"

## Compatibility Mapping
- serde_json: DecodeMode::StrictUnicode, EncodeMode::EncodeStrictUnicode.
- Pydantic v2: DecodeMode::StrictUnicode; EncodeMode::EncodeStrictUnicode (serialization errors on unpaired surrogates).
- Python json: DecodeMode::SurrogatePreserving; EncodeMode::EncodeSurrogatesEscaped (surrogates always escaped; non-ASCII as raw iff not ascii_only).
- Node: DecodeMode::SurrogatePreserving; EncodeMode::EncodeSurrogatesEscaped.

## Implementation Notes
- Join surrogate pairs by computing code point: 0x10000 + ((high-0xD800)<<10) + (low-0xDC00).
- For streaming, when a high surrogate is decoded, defer emission until the next escape is resolved; maintain a small state machine.
- For ReplaceInvalid, use U+FFFD replacement for each unpaired code unit; do not attempt to infer intended pairs across non-adjacent code units.
- Ensure error messages include line/column and a specific code (see taxonomy) to aid debugging.

## References
- RFC 8259 / ECMA‑404 (JSON)
- Unicode Standard, Section 3.9 (Unicode Encoding Forms)
- WHATWG Encoding (WTF‑8) for optional surrogate‑capable UTF‑8 representation
