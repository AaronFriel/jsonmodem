use alloc::{string::String, vec::Vec};

use quickcheck::{Arbitrary, Gen};

use crate::{value::Map, Array, StringValueMode, Value};

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
                    _ => Value::String(String::arbitrary(g).into()),
                }
            } else {
                match usize::arbitrary(g) % 6 {
                    0 => Value::Null,
                    1 => Value::Boolean(bool::arbitrary(g)),
                    2 => Value::Number(JsonNumber::arbitrary(g).0),
                    3 => Value::String(String::arbitrary(g).into()),
                    4 => {
                        let len = usize::arbitrary(g) % 3;
                        let mut vec = Array::new_sync();
                        for _ in 0..len {
                            vec.push_back_mut(gen_val(g, depth - 1));
                        }
                        Value::Array(vec)
                    }
                    _ => {
                        let len = usize::arbitrary(g) % 3;
                        let mut map = Map::new_sync();
                        for _ in 0..len {
                            let key = String::arbitrary(g).into();
                            let val = gen_val(g, depth - 1);
                            map = map.insert(key, val);
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

impl Arbitrary for StringValueMode {
    fn arbitrary(g: &mut Gen) -> Self {
        match usize::arbitrary(g) % 3 {
            0 => StringValueMode::None,
            1 => StringValueMode::Prefixes,
            _ => StringValueMode::Values,
        }
    }
}
