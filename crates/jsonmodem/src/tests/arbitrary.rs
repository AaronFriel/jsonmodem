use alloc::{string::String, vec::Vec};

use quickcheck::{Arbitrary, Gen};

use crate::{Value, value::Map};

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct JsonNumber(f64);

impl Arbitrary for JsonNumber {
    fn arbitrary(g: &mut Gen) -> Self {
        let mut value = f64::arbitrary(g);
        while !value.is_finite() {
            value = f64::arbitrary(g);
        }

        Self(value)
    }
}

impl Arbitrary for Value {
    fn arbitrary(g: &mut Gen) -> Self {
        fn gen_val(g: &mut Gen, depth: usize) -> Value {
            if depth == 0 {
                match usize::arbitrary(g) % 4 {
                    0 => Value::Null,
                    1 => Value::Boolean(bool::arbitrary(g)),
                    2 => Value::Number(JsonNumber::arbitrary(g).0),
                    _ => Value::String(String::arbitrary(g)),
                }
            } else {
                match usize::arbitrary(g) % 6 {
                    0 => Value::Null,
                    1 => Value::Boolean(bool::arbitrary(g)),
                    2 => Value::Number(JsonNumber::arbitrary(g).0),
                    3 => Value::String(String::arbitrary(g)),
                    4 => {
                        let len = usize::arbitrary(g) % 3;
                        let mut vec = Vec::new();
                        for _ in 0..len {
                            vec.push(gen_val(g, depth - 1));
                        }
                        Value::Array(vec)
                    }
                    _ => {
                        let len = usize::arbitrary(g) % 3;
                        let mut map = Map::new();
                        for _ in 0..len {
                            let key = String::arbitrary(g);
                            let val = gen_val(g, depth - 1);
                            map.insert(key.into(), val);
                        }
                        Value::Object(map)
                    }
                }
            }
        }

        let depth = usize::arbitrary(g) % 2;
        gen_val(g, depth)
    }
}

// StringValueMode removed; buffering policy is in JsonModemBuffers
