use std::{cell::RefCell, hint::black_box};

use arbitrary::Arbitrary;
use jsonmodem::{BufferOptions, ParserOptions, ValuesOptions};
use libfuzzer_sys::{fuzz_mutator, fuzzer_mutate};
use rand::{Rng, SeedableRng, rngs::SmallRng};
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

#[derive(Clone)]
pub struct FuzzerInput {
    pub flags: u8,
    pub chunks: Vec<String>,
}

impl core::fmt::Debug for FuzzerInput {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let allow_multiple = (self.flags & 1) != 0;
        let uppercase_u = (self.flags & 2) != 0;
        let unicode_ws = (self.flags & 4) != 0;
        let partial = (self.flags & 0x10) != 0;
        writeln!(
            f,
            "flags: allow_multiple={}, uppercase_u={}, unicode_ws={}, partial_values={}",
            allow_multiple, uppercase_u, unicode_ws, partial
        )?;
        let joined = self.chunks.join("");
        writeln!(f, "text:\n{}", joined)?;
        writeln!(f, "chunks:{}", self.chunks.len())?;
        for (i, c) in self.chunks.iter().enumerate() {
            writeln!(f, "  [{}] {:?}", i, c)?;
        }
        Ok(())
    }
}

pub fn parser_options(flags: u8) -> ParserOptions {
    ParserOptions::default()
        .with_allow_multiple_json_values(flags & 1 != 0)
        .with_allow_uppercase_u(flags & 2 != 0)
        .with_allow_unicode_whitespace(flags & 4 != 0)
        .with_panic_on_error(false)
}

#[allow(dead_code)]
pub fn buffer_options(_flags: u8) -> BufferOptions {
    BufferOptions::default()
}

#[allow(dead_code)]
pub fn values_options(flags: u8) -> ValuesOptions {
    ValuesOptions::default().with_partial(flags & 0x10 != 0)
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
    // Cooperative: always (re)write a valid header (if room), then either
    // synthesize structured JSON payload (with optional corruption) or fall
    // back to default mutation for exploration.
    let mut rng = SmallRng::seed_from_u64(seed as u64);
    let cap = core::cmp::min(max_size, data.len());
    if cap < HEADER {
        return fuzzer_mutate(data, size, max_size);
    }

    // Flags: randomize parser behaviors; bit 0x08 toggles corruption mode the
    // target applies
    let mut flags: u8 = 0;
    if rng.random::<bool>() {
        flags |= 0x01;
    }
    if rng.random::<bool>() {
        flags |= 0x02;
    }
    if rng.random::<bool>() {
        flags |= 0x04;
    }
    if rng.random::<bool>() {
        flags |= 0x08;
    }
    if rng.random::<bool>() {
        flags |= 0x10;
    }
    data[0] = flags;
    let split_seed = rng.random::<u32>();
    data[1..HEADER].copy_from_slice(&split_seed.to_le_bytes());

    // With ~1/6 probability, delegate to the default mutator for diversity
    if seed.is_multiple_of(6) {
        return fuzzer_mutate(data, size, max_size);
    }

    let mut prefix = HEADER;
    // Decide how many roots to emit: 1..=3
    let roots = 1 + (seed as usize % 3);
    for r in 0..roots {
        // Leading whitespace before a value
        if prefix >= cap {
            break;
        }
        prefix += append_whitespace(&mut data[prefix..cap], cap - prefix);
        // Generate a JSON Value and serialize into-place
        if prefix >= cap {
            break;
        }
        prefix += append_value(&mut data[prefix..cap], size.max(32), cap - prefix);
        // Trailing whitespace after the value
        if prefix >= cap {
            break;
        }
        prefix += append_whitespace(&mut data[prefix..cap], cap - prefix);
        if prefix >= cap {
            break;
        }
        // Between roots, add a bit more whitespace
        if r + 1 != roots {
            prefix += append_whitespace(&mut data[prefix..cap], cap - prefix);
        }
        if prefix >= cap {
            break;
        }
    }
    core::cmp::min(prefix, cap)
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
        if limit == 0 || buf.is_empty() {
            return 0;
        }

        let cap = limit.min(buf.len());
        let n_codepoints = rng.random_range(1..=cap.min(8));
        let mut written = 0;

        for _ in 0..n_codepoints {
            let w = WS_TABLE[rng.random_range(0..WS_TABLE.len())];

            if written + w.len() > cap {
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

fn corrupt_utf8_text(mut s: String, seed: u64) -> String {
    let mut rng = SmallRng::seed_from_u64(seed);
    // 1..=4 operations
    let ops = rng.random_range(1..=4);
    for _ in 0..ops {
        let bounds: Vec<usize> = s
            .char_indices()
            .map(|(i, _)| i)
            .chain(core::iter::once(s.len()))
            .collect();
        if bounds.is_empty() {
            break;
        }
        match rng.random_range(0..8) {
            0 => {
                // delete a character
                if bounds.len() > 1 {
                    let i = bounds[rng.random_range(0..bounds.len() - 1)];
                    s.remove(i);
                }
            }
            1 => {
                // insert delimiter
                let delims = ["{", "}", "[", "]", ",", ":"];
                let pos = bounds[rng.random_range(0..bounds.len())];
                s.insert_str(pos, delims[rng.random_range(0..delims.len())]);
            }
            2 => {
                // break string
                let pos = bounds[rng.random_range(0..bounds.len())];
                s.insert_str(pos, if rng.random::<bool>() { "\n" } else { "\"" });
            }
            3 => {
                // break escape
                let pos = bounds[rng.random_range(0..bounds.len())];
                let c = ["u", "x", "U", "\\", "\"", "/"][rng.random_range(0..6)];
                s.insert(pos, '\\');
                s.insert_str(pos + 1, c);
            }
            4 => {
                // wrap with unbalancing brackets
                let add = if rng.random::<bool>() { "[," } else { "{" };
                let close = if add == "[," { "]" } else { "}" };
                s = format!("{}{}{}", add, s, close);
            }
            5 => {
                // invalid number fragment
                let pos = bounds[rng.random_range(0..bounds.len())];
                let frag = ["01", "-", "+1", "1.", "1e", "--1"][rng.random_range(0..6)];
                s.insert_str(pos, frag);
            }
            6 => {
                // control code via unicode escape
                let pos = bounds[rng.random_range(0..bounds.len())];
                let ch = ["\u{0000}", "\u{0001}", "\u{001F}"][rng.random_range(0..3)];
                s.insert_str(pos, ch);
            }
            _ => {
                // random bracket/quote
                let add = ["{", "[", "]", "}", "\""][rng.random_range(0..5)];
                let pos = bounds[rng.random_range(0..bounds.len())];
                s.insert_str(pos, add);
            }
        }
    }
    s
}

impl<'a> Arbitrary<'a> for FuzzerInput {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let all = u.bytes(u.len())?;
        if all.len() < HEADER {
            return Err(arbitrary::Error::NotEnoughData);
        }
        let flags = all[0];
        let split_seed = u32::from_le_bytes(all[1..HEADER].try_into().unwrap()) as u64;
        let mut text = String::from_utf8_lossy(&all[HEADER..]).into_owned();
        if (flags & 0x08) != 0 {
            text = corrupt_utf8_text(text, split_seed);
        }
        let chunks = split_into_safe_chunks(&text, split_seed)
            .into_iter()
            .map(|s| s.to_owned())
            .collect();
        Ok(FuzzerInput { flags, chunks })
    }
}
