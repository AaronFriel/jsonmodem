#![expect(missing_docs)]
use jsonmodem::{JsonValueFactory, StdValueFactory, Value};

#[test]
fn std_factory_roundtrip() {
    let mut arr = StdValueFactory.new_array();
    StdValueFactory.push_array(
        &mut arr,
        StdValueFactory.build_from_bool(StdValueFactory.new_bool(true)),
    );
    let mut obj = StdValueFactory.new_object();
    StdValueFactory.insert_object(
        &mut obj,
        "n",
        StdValueFactory.build_from_num(StdValueFactory.new_number(1.0)),
    );
    let v_arr = StdValueFactory.build_from_array(arr);
    let v_obj = StdValueFactory.build_from_object(obj);
    assert_eq!(v_arr, Value::Array(vec![Value::Boolean(true)]));
    assert_eq!(
        v_obj,
        Value::Object([("n".to_string(), Value::Number(1.0))].into())
    );
}
