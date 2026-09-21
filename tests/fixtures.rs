use edge_mtf_bench::{all_fixtures, checksum_hex};

#[test]
fn fixtures_are_byte_deterministic_and_published() {
    // Regenerating fixtures must yield identical bytes (fixed seeds).
    let a = all_fixtures();
    let b = all_fixtures();
    assert_eq!(a.len(), 3);
    for (x, y) in a.iter().zip(b.iter()) {
        assert_eq!(
            x.image.data, y.image.data,
            "{} must be deterministic",
            x.key
        );
        assert_eq!(checksum_hex(&x.image.data), checksum_hex(&y.image.data));
    }
    // Pin the checksums documented implicitly by the app, so a silent
    // generator change fails CI.
    let expect = [
        ("bad_rows", "2d3c6c042ba7664d"),
        ("dual_pitch", "522fe7d5a0ff3bed"),
        ("ringing_edge", "b7edc03a2b42d298"),
    ];
    for ((key, want), f) in expect.iter().zip(&a) {
        assert_eq!(key, &f.key);
        assert_eq!(&checksum_hex(&f.image.data), want);
    }
}

#[test]
fn rng_is_deterministic() {
    use edge_mtf_bench::image::Rng;
    let s1: Vec<u64> = {
        let mut r = Rng::new(42);
        (0..8).map(|_| r.next_u64()).collect()
    };
    let s2: Vec<u64> = {
        let mut r = Rng::new(42);
        (0..8).map(|_| r.next_u64()).collect()
    };
    assert_eq!(s1, s2);
    assert_ne!(s1[0], s1[1]);
}
