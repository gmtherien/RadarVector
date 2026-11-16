//! Checks that signatures produced by `cargo packager` (release workflow)
//! verify with RadarVector's updater. Ignored by default; run with
//! RV_SIGNED_FILE, RV_SIGNATURE_FILE and RV_PUBLIC_KEY_FILE set.

use radarvector_feed::appupdate::verify;

#[test]
#[ignore = "needs files produced by cargo packager"]
fn packager_signature_verifies() {
    let data = std::fs::read(std::env::var("RV_SIGNED_FILE").unwrap()).unwrap();
    let sig = std::fs::read_to_string(std::env::var("RV_SIGNATURE_FILE").unwrap()).unwrap();
    let pk = std::fs::read_to_string(std::env::var("RV_PUBLIC_KEY_FILE").unwrap()).unwrap();
    verify(&data, &sig, &pk).expect("valid signature");
    let mut tampered = data.clone();
    let mid = tampered.len() / 2;
    tampered[mid] ^= 0xFF;
    assert!(
        verify(&tampered, &sig, &pk).is_err(),
        "tampered package must be refused"
    );
}
