you will create an execplan to ship the immutable backend for jsonmodem. start from the api below and call out that the immutable snapshots don’t need a lending iterator.

```rs
use jsonmodem::{
    BufferOptions, ImBackend, ImValueAssembler, JsonModem, JsonModemBuffers, JsonModemValues,
    ParserOptions, ValuesOptions,
};

let mut parser = JsonModem::<ImBackend>::new(ParserOptions::default());

let assembler = ImValueAssembler::new(BufferOptions::default());
let mut buffers = JsonModemBuffers::with_builder(ParserOptions::default(), assembler.clone());
let mut values = JsonModemValues::with_buffer_builder(
    ParserOptions::default(),
    ValuesOptions::default(),
    assembler,
);

#[cfg(feature = "im")]
{
    use jsonmodem::JsonModemIm;

    let mut snapshots = JsonModemIm::new();
    for view in snapshots.feed("{\"title\":\"he") {
        println!("partial: {}", view.value);
    }
    for view in snapshots.feed("llo\"}") {
        println!("partial: {}", view.value);
    }
    for view in snapshots.finish() {
        if view.is_final {
            println!("final: {}", view.value);
        }
    }
}
```

your plan should

clone ecow and rpds into tmp/upstream for research
build the im backend (value enum, zipper, applicator) under crates/jsonmodem/src/backend/im
re-export ImBackend, ImValueAssembler, JsonModemIm behind the `im` feature
document benchmarks comparing Std vs IM adapters
reuse the QuickCheck suites in crates/jsonmodem/src/tests/property_multivalue.rs and property_partition.rs for im
detail number/decode mode trade-offs and safety notes in the plan
