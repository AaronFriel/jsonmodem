use std::{cell::RefCell, hint::black_box};

use arbitrary::Arbitrary;
use jsonmodem::{BufferOptions, ParserOptions, ValuesOptions};
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

pub struct PreparedInput {
    pub flags: u8,
    pub chunks: Vec<String>,
}

pub fn prepare_input(data: &[u8]) -> Option<PreparedInput> {
    if data.len() < HEADER {
        return None;
    }

    let flags = data[0];
    let split_seed = u32::from_le_bytes(data[1..HEADER].try_into().unwrap()) as u64;
    let payload = &data[HEADER..];

    if payload.is_empty() {
        return None;
    }

    let source = String::from_utf8_lossy(payload).into_owned();
    let chunks = split_into_safe_chunks(&source, split_seed)
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if chunks.is_empty() {
        return None;
    }

    Some(PreparedInput { flags, chunks })
}

pub fn parser_options(flags: u8) -> ParserOptions {
    ParserOptions::default()
        .with_allow_multiple_json_values(flags & 1 != 0)
        .with_allow_uppercase_u(flags & 2 != 0)
        .with_allow_unicode_whitespace(flags & 4 != 0)
        .with_panic_on_error(false)
}

pub fn consume_results<I, T, E>(iter: I)
where
    I: IntoIterator<Item = Result<T, E>>,
{
    for item in iter {
        match item {
            Ok(value) => {
                black_box(value);
            }
            Err(err) => {
                black_box(err);
            }
        }
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
        let node_type = u.choose_index(21)?;
        let value = match node_type {
            0 => Value::Null,
            1 => Value::Bool(u.arbitrary()?),
            2 => {
                let n: f64 = u.arbitrary()?;
                Value::Number(
                    serde_json::Number::from_f64(n).ok_or(arbitrary::Error::IncorrectFormat)?,
                )
            }
            3..=10 => Value::String(u.arbitrary()?),
            11..=15 => {
                let elems: Vec<ArbitraryValue> = u.arbitrary()?;
                Value::Array(elems.into_iter().map(|v| v.0).collect())
            }
            16..=20 => {
                let m: Vec<(String, ArbitraryValue)> = u.arbitrary()?;
                Value::Object(Map::from_iter(m.into_iter().map(|(k, v)| (k, v.0))))
            }
            _ => Err(arbitrary::Error::IncorrectFormat)?,
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
