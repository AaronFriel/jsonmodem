#![allow(missing_docs)]
use jsonmodem::{JsonFactory, Value};

#[test]
fn std_factory_roundtrip() {
    let mut arr = Value::new_array();
    Value::push_array(&mut arr, Value::from_bool(Value::new_bool(true)));
    let mut obj = Value::new_object();
    Value::insert_object(&mut obj, "n", Value::from_num(Value::new_number(1.0)));
    let v_arr = Value::from_array(arr);
    let v_obj = Value::from_object(obj);
    assert_eq!(v_arr, Value::Array(vec![Value::Boolean(true)]));
    assert_eq!(
        v_obj,
        Value::Object([("n".to_string(), Value::Number(1.0))].into())
    );
}
