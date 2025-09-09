use std::{cell::RefCell, hint::black_box};

use arbitrary::Arbitrary;
use jsonmodem::{BufferOptions, ParserOptions, ValuesOptions, lending_iterator::LendingIterator};
use libfuzzer_sys::{fuzz_mutator, fuzzer_mutate};
use rand::{Rng, RngCore, SeedableRng, rngs::SmallRng};
use serde_json::{Map, Value};

pub const HEADER: usize = 5; // mode u8 + split-seed u32

thread_local! {
    static RNG: RefCell<SmallRng> = RefCell::new(SmallRng::from_os_rng());
}

static WS_TABLE: &[&[u8]] = &[
    b" ",
    b"\t",
    b"\n",
    b"\r",
    "\u{1680}".as_bytes(),
    "\u{2000}".as_bytes(),
    "\u{2001}".as_bytes(),
    "\u{2002}".as_bytes(),
    "\u{2003}".as_bytes(),
    "\u{2004}".as_bytes(),
    "\u{2005}".as_bytes(),
    "\u{2006}".as_bytes(),
    "\u{2007}".as_bytes(),
    "\u{2008}".as_bytes(),
    "\u{2009}".as_bytes(),
    "\u{200A}".as_bytes(),
    "\u{2028}".as_bytes(),
    "\u{2029}".as_bytes(),
    "\u{202F}".as_bytes(),
    "\u{205F}".as_bytes(),
    "\u{3000}".as_bytes(),
];

#[derive(Clone, Copy, Debug, Arbitrary)]
pub struct FuzzFlags {
    pub allow_multiple_json_values: bool,
    pub allow_uppercase_u: bool,
    pub allow_unicode_whitespace: bool,
    pub partial_values: bool,
}

#[derive(Debug)]
pub struct PreparedInput {
    pub flags: FuzzFlags,
    pub chunks: Vec<String>,
}

pub fn parser_options(flags: FuzzFlags) -> ParserOptions {
    ParserOptions::default()
        .with_allow_multiple_json_values(flags.allow_multiple_json_values)
        .with_allow_uppercase_u(flags.allow_uppercase_u)
        .with_allow_unicode_whitespace(flags.allow_unicode_whitespace)
        .with_panic_on_error(false)
}

#[allow(dead_code)]
pub fn buffer_options(_flags: FuzzFlags) -> BufferOptions {
    BufferOptions::default()
}

#[allow(dead_code)]
pub fn values_options(flags: FuzzFlags) -> ValuesOptions {
    ValuesOptions::default().with_partial(flags.partial_values)
}

pub fn consume_results<I>(iter: &mut I)
where
    I: LendingIterator,
{
    while let Some(item) = iter.next() {
        black_box(item);
    }
}

fn with_rng<F, R>(f: F) -> R
where
    F: FnOnce(&mut SmallRng) -> R,
{
    RNG.with(|cell| f(&mut cell.borrow_mut()))
}

// Map arbitrary bytes to mostly-printable ASCII to keep UTF‑8 intact.
fn map_to_ascii(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .map(|b| if b.is_ascii() { *b } else { (b % 0x5e) + 0x20 })
        .collect()
}

fn to_utf8_prefix(bytes: &[u8]) -> Option<&str> {
    match core::str::from_utf8(bytes) {
        Ok(s) => Some(s),
        Err(e) => {
            let n = e.valid_up_to();
            if n > 0 {
                core::str::from_utf8(&bytes[..n]).ok()
            } else {
                None
            }
        }
    }
}

fn corrupt_json(mut s: String, seed: u64) -> String {
    with_rng(|rng| {
        // Mix in the libfuzzer seed to get deterministic-but-varied ops
        let mut prng = SmallRng::seed_from_u64(seed ^ rng.next_u64());
        if s.is_empty() {
            s.push_str("\"\"");
        }
        let ops = prng.random_range(1..=4);
        for _ in 0..ops {
            match prng.random_range(0..8) {
                0 => {
                    let idxs: Vec<_> = s.char_indices().map(|(i, _)| i).collect();
                    if idxs.len() > 1 {
                        let i = idxs[prng.random_range(0..idxs.len())];
                        s.remove(i);
                    }
                }
                1 => {
                    let delims = ["{", "}", "[", "]", ",", ":"];
                    let pos = prng.random_range(0..=s.len());
                    s.insert_str(pos, delims[prng.random_range(0..delims.len())]);
                }
                2 => {
                    let pos = prng.random_range(0..=s.len());
                    s.insert_str(pos, if prng.random::<bool>() { "\n" } else { "\"" });
                }
                3 => {
                    let pos = prng.random_range(0..=s.len());
                    let c = ["u", "x", "U", "\\", "\"", "/"][prng.random_range(0..6)];
                    s.insert_str(pos, "\\");
                    s.insert_str(pos + 1, c);
                }
                4 => {
                    let add = if prng.random::<bool>() { "[," } else { "{" };
                    let close = if add == "[," { "]" } else { "}" };
                    s = format!("{}{}{}", add, s, close);
                }
                5 => {
                    let pos = prng.random_range(0..=s.len());
                    let frag = ["01", "-", "+1", "1.", "1e", "--1"][prng.random_range(0..6)];
                    s.insert_str(pos, frag);
                }
                6 => {
                    let pos = prng.random_range(0..=s.len());
                    let ch = ["\u{0000}", "\u{0001}", "\u{001F}"][prng.random_range(0..3)];
                    s.insert_str(pos, ch);
                }
                7 => {
                    let add = ["{", "[", "]", "}", "\""][prng.random_range(0..5)];
                    let pos = prng.random_range(0..=s.len());
                    s.insert_str(pos, add);
                }
                _ => {}
            }
        }
        s
    })
}

fn mutator(data: &mut [u8], size: usize, max_size: usize, seed: u32) -> usize {
    if max_size < HEADER {
        return fuzzer_mutate(data, size, max_size);
    }

    // With probability ~1/8, let the default mutator explore unstructured space.
    if seed.count_ones() % 8 == 0 {
        return fuzzer_mutate(data, size, max_size);
    }

    // Header
    // Heavily favor corrupt inputs: ~10% structured, ~80% corrupt, ~10% raw ASCII
    let draw = with_rng(|rng| rng.random_range(0..10));
    let mode: u8 = if draw == 0 {
        0
    } else if draw <= 8 {
        1
    } else {
        2
    }; // 0=structured,1=corrupt,2=raw-ascii
    let flags = with_rng(|rng| rng.next_u32() as u8 & 0xF0); // carry option bits in high nybble
    let byte0 = mode | flags;
    let split_seed = with_rng(|rng| rng.next_u32());
    data[0] = byte0;
    data[1..HEADER].copy_from_slice(&split_seed.to_le_bytes());

    // Payload buffer after header
    let buf = &mut data[HEADER..max_size];

    // Decide target payload length based on incoming size to help shrinking
    let want = size.saturating_sub(HEADER).max(16).min(buf.len());

    let written = match mode {
        // Structured: generate valid JSON + whitespace like before
        0 => {
            let mut prefix = 0usize;
            while prefix < want {
                let limit = want - prefix;
                prefix += append_whitespace(&mut buf[prefix..], limit);
                prefix += append_value(&mut buf[prefix..], want, limit);
                prefix += append_whitespace(&mut buf[prefix..], limit);
            }
            prefix
        }
        // Corrupt-from-valid: build simple valid JSON and then break it
        1 => {
            // Produce a base value
            let base_len = want.min(buf.len());
            let mut tmp = Vec::with_capacity(base_len);
            // reuse append_value into a scratch vec
            // synthesise by writing into a temp slice backed by vec
            tmp.resize(base_len, 0);
            let n = append_value(&mut tmp[..], want, base_len);
            let s = core::str::from_utf8(&tmp[..n])
                .unwrap_or("{}" /* fallback */)
                .to_string();
            let broken = corrupt_json(s, split_seed as u64);
            let bytes = broken.as_bytes();
            let n = bytes.len().min(buf.len());
            buf[..n].copy_from_slice(&bytes[..n]);
            n
        }
        // Raw ASCII
        _ => {
            let ascii = b"{}[],:\"-+eE truefalsnul 0123456789 \n\r\t xyz";
            for i in 0..want {
                buf[i] = ascii[with_rng(|rng| rng.random_range(0..ascii.len()))];
            }
            want
        }
    };

    HEADER + written
}

fuzz_mutator!(|data: &mut [u8], size: usize, max_size: usize, seed: u32| {
    mutator(data, size, max_size, seed)
});

#[derive(Debug)]
struct ArbitraryValue(Value);

impl<'a> Arbitrary<'a> for ArbitraryValue {
    fn arbitrary(u: &mut arbitrary::Unstructured<'_>) -> arbitrary::Result<Self> {
        let node_type = u.choose_index(6)?;
        let value = match node_type {
            0 => Value::Null,
            1 => Value::Bool(u.arbitrary()?),
            2 => {
                let r = match u.choose_index(3)? {
                    0 => serde_json::Number::from_f64(u.arbitrary()?)
                        .ok_or(arbitrary::Error::IncorrectFormat)?,
                    1 => serde_json::Number::from(u.arbitrary::<i64>()?),
                    2 => serde_json::Number::from(u.arbitrary::<u64>()?),
                    _ => unreachable!(),
                };
                Value::Number(r)
            }
            3 => Value::String(u.arbitrary()?),
            4 => {
                let elems: Vec<ArbitraryValue> = u.arbitrary()?;
                Value::Array(elems.into_iter().map(|v| v.0).collect())
            }
            5 => {
                let m: Vec<(String, ArbitraryValue)> = u.arbitrary()?;
                Value::Object(Map::from_iter(m.into_iter().map(|(k, v)| (k, v.0))))
            }
            _ => unreachable!(),
        };
        Ok(ArbitraryValue(value))
    }
}

fn append_whitespace(buf: &mut [u8], limit: usize) -> usize {
    with_rng(|rng| {
        if limit == 0 {
            return 0;
        }

        let n_codepoints = rng.random_range(1..=limit.min(8));
        let mut written = 0;

        for _ in 0..n_codepoints {
            let w = WS_TABLE[rng.random_range(0..WS_TABLE.len())];

            if written + w.len() > limit {
                break;
            }

            buf[written..written + w.len()].copy_from_slice(w);
            written += w.len();
        }

        written
    })
}

fn append_value(data: &mut [u8], size: usize, limit: usize) -> usize {
    let value = loop {
        let s = with_rng(|rng| rng.random_range(size / 2..size * 2).min(limit));
        let bytes: Vec<u8> = with_rng(|rng| (0..s).map(|_| rng.random::<u8>()).collect());
        match ArbitraryValue::arbitrary(&mut arbitrary::Unstructured::new(&bytes)) {
            Ok(value) => break value,
            Err(_) => continue,
        };
    };

    let serialized = serde_json::to_vec(&value.0).expect("Failed to serialize arbitrary value");
    let len = serialized.len().min(limit);
    data[..len].copy_from_slice(&serialized[..len]);
    len
}

pub fn split_into_safe_chunks(serialized: &str, split_seed: u64) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let len = serialized.len();

    while start < len {
        let remaining = len - start;
        let mut size = (split_seed as usize % remaining) + 1;

        while start + size < len && !serialized.is_char_boundary(start + size) {
            size += 1;
        }

        chunks.push(&serialized[start..start + size]);
        start += size;
    }

    chunks
}

// New: Structured input for the fuzz target with Arbitrary decoding
impl<'a> Arbitrary<'a> for PreparedInput {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        // Read header compatible with our mutator: mode + split seed
        let mode_and_flags: u8 = u.arbitrary()?;
        let split_seed: u32 = u.arbitrary()?;

        let flags = FuzzFlags {
            allow_multiple_json_values: mode_and_flags & 0x10 != 0,
            allow_uppercase_u: mode_and_flags & 0x20 != 0,
            allow_unicode_whitespace: mode_and_flags & 0x40 != 0,
            partial_values: mode_and_flags & 0x80 != 0,
        };

        let mut mode = mode_and_flags & 0x03; // 0=structured,1=corrupt,2=raw/ascii,3=as-is

        // Bias generation: only 1/10 times allow mode 0 (structured).
        if mode == 0 {
            let allow_structured = u.ratio(1u32, 10u32)?;
            if !allow_structured {
                mode = 1; // prefer corrupt path
            }
        }

        let out = match mode {
            0 => {
                // Structured: build N valid JSON roots and whitespace
                let n_roots = 1 + u.choose_index(4)?; // 1..=4
                let mut s = String::new();
                for i in 0..n_roots {
                    if u.arbitrary::<bool>()? {
                        append_ws_str(&mut s, u, flags.allow_unicode_whitespace)?;
                    }
                    let v: ArbitraryValue = u.arbitrary()?;
                    let json = serde_json::to_string(&v.0)
                        .map_err(|_| arbitrary::Error::IncorrectFormat)?;
                    s.push_str(&json);
                    if i + 1 != n_roots || u.arbitrary::<bool>()? {
                        append_ws_str(&mut s, u, flags.allow_unicode_whitespace)?;
                    }
                }
                if !flags.allow_multiple_json_values
                    && let Some(idx) = s.find('}')
                {
                    s.truncate(idx + 1);
                }
                s
            }
            1 => {
                // Corrupt-from-valid: produce a valid value, then perturb it
                let v: ArbitraryValue = u.arbitrary()?;
                let base =
                    serde_json::to_string(&v.0).map_err(|_| arbitrary::Error::IncorrectFormat)?;
                corrupt_json(base, split_seed as u64)
            }
            2 => {
                // Raw ASCII from remaining bytes
                let rest = u.bytes(u.len())?;
                let mapped = map_to_ascii(rest);
                core::str::from_utf8(&mapped)
                    .map(|s| s.to_owned())
                    .map_err(|_| arbitrary::Error::IncorrectFormat)?
            }
            _ => {
                // As-is: take UTF‑8 prefix from remaining bytes
                let rest = u.bytes(u.len())?;
                to_utf8_prefix(rest)
                    .map(|s| s.to_owned())
                    .ok_or(arbitrary::Error::NotEnoughData)?
            }
        };

        let chunks = split_into_safe_chunks(&out, split_seed as u64)
            .into_iter()
            .map(|s| s.to_owned())
            .collect::<Vec<_>>();

        Ok(PreparedInput { flags, chunks })
    }
}

fn append_ws_str(
    target: &mut String,
    u: &mut arbitrary::Unstructured<'_>,
    unicode: bool,
) -> arbitrary::Result<()> {
    // Up to 8 codepoints of whitespace
    let n = 1 + u.choose_index(8)?;
    for _ in 0..n {
        if unicode {
            let idx = u.choose_index(WS_TABLE.len())?;
            target.push_str(std::str::from_utf8(WS_TABLE[idx]).unwrap());
        } else {
            target.push(match u.choose_index(4)? {
                0 => ' ',
                1 => '\t',
                2 => '\n',
                _ => '\r',
            });
        }
    }
    Ok(())
}
