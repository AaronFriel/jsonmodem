#![allow(missing_docs)]
use jsonmodem::{JsonFactory, StdFactory, Value};

#[test]
fn std_factory_roundtrip() {
    let f = StdFactory;
    let mut arr = f.new_array();
    f.push_array(&mut arr, f.into_any_bool(f.new_bool(true)));
    let mut obj = f.new_object();
    f.insert_object(&mut obj, "n", f.into_any_num(f.new_number(1.0)));
    let v_arr = f.into_any_array(arr);
    let v_obj = f.into_any_object(obj);
    assert_eq!(v_arr, Value::Array(vec![Value::Boolean(true)]));
    assert_eq!(
        v_obj,
        Value::Object([("n".to_string(), Value::Number(1.0))].into())
    );
}
