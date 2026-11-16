#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = radarvector_data::dat::parse(s);
        let _ = radarvector_data::boundaries::parse(s);
    }
});
