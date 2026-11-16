#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = radarvector_feed::models::decode_data_feed(data);
    let _ = radarvector_feed::models::decode_transceivers(data);
});
