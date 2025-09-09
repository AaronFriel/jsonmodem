use std::{cell::RefCell, hint::black_box};

use arbitrary::Arbitrary;
use jsonmodem::{BufferOptions, ParserOptions, ValuesOptions, lending_iterator::LendingIterator};
use libfuzzer_sys::{fuzz_mutator, fuzzer_mutate};
use rand::{Rng, RngCore, SeedableRng, rngs::SmallRng};
use serde_json::{Map, Value};

pub const HEADER: usize = 5;

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

fn mutator(data: &mut [u8], size: usize, max_size: usize, seed: u32) -> usize {
    if size < HEADER || seed.is_multiple_of(10) {
        data[0] = with_rng(|rng| rng.next_u32() as u8 & 0x1F);
        data[1..HEADER].copy_from_slice(&with_rng(|rng| rng.next_u32().to_le_bytes()));

        let mut prefix = HEADER;

        while prefix < size {
            let limit = max_size - prefix;
            prefix += append_whitespace(&mut data[prefix..], limit);
            prefix += append_value(&mut data[prefix..], size, limit);
            prefix += append_whitespace(&mut data[prefix..], limit);
        }

        prefix
    } else {
        fuzzer_mutate(data, size, max_size)
    }
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
        // Choose flags first so that option behavior is driven by bytes
        let flags: FuzzFlags = u.arbitrary()?;

        // Decide how many JSON roots to emit (exercise multi-root option)
        let n_roots = 1 + u.choose_index(4)?; // 1..=4

        // Build one or more arbitrary JSON Values and serialize them with random
        // whitespace
        let mut out = String::new();
        for i in 0..n_roots {
            // Optional leading/trailing unicode whitespace depending on flags / bytes
            if u.arbitrary::<bool>()? {
                append_ws_str(&mut out, u, flags.allow_unicode_whitespace)?;
            }

            let v: ArbitraryValue = u.arbitrary()?;
            let s = serde_json::to_string(&v.0).map_err(|_| arbitrary::Error::IncorrectFormat)?;
            out.push_str(&s);

            if i + 1 != n_roots || u.arbitrary::<bool>()? {
                append_ws_str(&mut out, u, flags.allow_unicode_whitespace)?;
            }
        }

        // If multiple roots are not allowed, bias towards single-root by occasionally
        // trimming
        if !flags.allow_multiple_json_values
            && let Some(idx) = out.find('}')
        {
            out.truncate(idx + 1);
        }

        // Choose a split seed and split along char boundaries
        let split_seed: u64 = u.arbitrary()?;
        let chunks = split_into_safe_chunks(&out, split_seed)
            .into_iter()
            .map(ToOwned::to_owned)
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
