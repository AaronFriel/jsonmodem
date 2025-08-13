use jsonmodem::parser::DecodeMode;
use jsonmodem::{
    BufferOptions, JsonModem, JsonModemBuffers, JsonModemValues, ParseEvent, ParserOptions,
    RawBufferAssembler, RawContext, StreamingValue, ValuesOptions,
};
use jsonmodem::value::Value;

const SURROGATE_JSON: &str = r#"\uD83D\uDE00"#;
const EXPECTED_BYTES: &[u8] = &[0xF0, 0x9F, 0x98, 0x80];

fn surrogate_options() -> ParserOptions {
    ParserOptions::default().with_decode_mode(DecodeMode::SurrogatePreserving)
}

fn buffer_options() -> BufferOptions {
    BufferOptions::default()
}

#[test]
fn jsonmodem_raw_context_emits_wtf8_bytes() {
    let mut modem = JsonModem::<RawContext>::new(surrogate_options());

    let mut fragments = Vec::new();
    for result in modem.feed(SURROGATE_JSON) {
        let event = result.expect("jsonmodem raw context parse error");
        if let ParseEvent::String {
            fragment, is_final, ..
        } = event
        {
            fragments.extend_from_slice(fragment.as_ref());
            if is_final {
                break;
            }
        }
    }

    for result in modem.finish() {
        if let ParseEvent::String {
            fragment, is_final, ..
        } = result.expect("jsonmodem finish error")
        {
            fragments.extend_from_slice(fragment.as_ref());
            if is_final {
                break;
            }
        }
    }

    assert_eq!(fragments, EXPECTED_BYTES);
}

#[test]
fn jsonmodem_buffers_raw_context_buffers_string_value() {
    let mut modem =
        JsonModemBuffers::<RawContext, RawBufferAssembler>::with_builder(
            surrogate_options(),
            RawBufferAssembler::new(buffer_options()),
        );

    let mut buffered_value = None;

    for result in modem.feed(SURROGATE_JSON) {
        match result.expect("buffer parsing failure") {
            jsonmodem::BufferedEvent::String {
                path,
                value,
                is_final,
                ..
            } if path.is_empty() && is_final => {
                buffered_value = Some(value.expect("missing buffered value"));
            }
            _ => {}
        }
    }

    for result in modem.finish() {
        match result.expect("buffer finish failure") {
            jsonmodem::BufferedEvent::String {
                path,
                value,
                is_final,
                ..
            } if path.is_empty() && is_final => {
                buffered_value.get_or_insert_with(|| value.expect("missing buffered value"));
            }
            _ => {}
        }
    }

    let value = buffered_value.expect("expected buffered string value");
    assert_eq!(value.as_ref(), EXPECTED_BYTES);
}

#[test]
fn jsonmodem_values_with_raw_context_produces_value() {
    let mut modem = JsonModemValues::with_buffer_builder(
        surrogate_options(),
        ValuesOptions::default(),
        RawBufferAssembler::new(buffer_options()),
    );

    let mut outputs = Vec::new();
    outputs.extend(modem.feed(SURROGATE_JSON).map(|v| v.expect("stream error")));
    outputs.extend(modem.finish().map(|v| v.expect("stream finish error")));

    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].value, Value::String("😀".to_owned()));
}
