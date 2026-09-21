use edge_mtf_bench::{Roi, Rotation};

#[test]
fn rotation_permutes_centers_and_keeps_half_pixel() {
    let roi = Roi {
        x0: 10,
        y0: 20,
        w: 5,
        h: 3,
        rotation: Rotation(0),
    };
    for rot in [0u16, 90, 180, 270] {
        let r = Roi {
            rotation: Rotation(rot),
            ..roi
        };
        let (lw, lh) = r.local_size();
        for u in 0..lw {
            for v in 0..lh {
                let (cx, cy) = r.map_center(u, v);
                // every mapped center is exactly integer + 0.5 (no interpolation)
                assert!((cx.fract() - 0.5).abs() < 1e-12, "rot {rot} cx={cx}");
                assert!((cy.fract() - 0.5).abs() < 1e-12, "rot {rot} cy={cy}");
                let (ox, oy) = r.map_pixel(u, v);
                assert!((cx - ox as f64 - 0.5).abs() < 1e-12);
                assert!((cy - oy as f64 - 0.5).abs() < 1e-12);
                assert!(ox >= 10 && ox < 15 && oy >= 20 && oy < 23);
            }
        }
        // all local pixels form a bijection onto the same original set
        let mut mapped: Vec<(u32, u32)> = Vec::new();
        for u in 0..lw {
            for v in 0..lh {
                mapped.push(r.map_pixel(u, v));
            }
        }
        mapped.sort();
        mapped.dedup();
        assert_eq!(mapped.len(), 15, "rotation {rot} must be bijective");
    }
}

#[test]
fn known_rotation_mappings() {
    // 5 wide (x 0..5), 3 high (y 0..3)
    let r90 = Roi {
        x0: 0,
        y0: 0,
        w: 5,
        h: 3,
        rotation: Rotation(90),
    };
    assert_eq!(r90.local_size(), (3, 5));
    assert_eq!(r90.map_pixel(0, 0), (4, 0));
    assert_eq!(r90.map_pixel(0, 4), (0, 0));
    assert_eq!(r90.map_pixel(2, 0), (4, 2));
    assert_eq!(r90.map_pixel(2, 4), (0, 2));
    let r180 = Roi {
        x0: 0,
        y0: 0,
        w: 5,
        h: 3,
        rotation: Rotation(180),
    };
    assert_eq!(r180.map_pixel(0, 0), (4, 2));
    assert_eq!(r180.map_pixel(4, 2), (0, 0));
    let r270 = Roi {
        x0: 0,
        y0: 0,
        w: 5,
        h: 3,
        rotation: Rotation(270),
    };
    assert_eq!(r270.map_pixel(0, 0), (0, 2));
    assert_eq!(r270.map_pixel(2, 4), (4, 0));
}

#[test]
fn centers_of_same_original_pixel_coincide_after_rotation() {
    // ROI 90-degree rotation scans the same pixels; pick the original pixel
    // (x=4,y=0) and check its representation in the 0 and 90 frames.
    let r0 = Roi {
        x0: 0,
        y0: 0,
        w: 5,
        h: 3,
        rotation: Rotation(0),
    };
    let r90 = Roi {
        x0: 0,
        y0: 0,
        w: 5,
        h: 3,
        rotation: Rotation(90),
    };
    let a = r0.map_center(4, 0);
    // in 90 frame original (4,0) is local (u=0, v=0)
    let b = r90.map_center(0, 0);
    assert_eq!(a, b);
}

#[test]
fn cropped_roi_preserves_original_pixel_centers() {
    // A non-origin, non-square crop must reference the same original pixel
    // centers (no recentering / interpolation after cropping).
    let r = Roi {
        x0: 17,
        y0: 9,
        w: 11,
        h: 7,
        rotation: Rotation(0),
    };
    assert_eq!(r.map_pixel(0, 0), (17, 9));
    assert_eq!(r.map_pixel(10, 6), (27, 15));
    let (cx, cy) = r.map_center(10, 6);
    assert!((cx - 27.5).abs() < 1e-12);
    assert!((cy - 15.5).abs() < 1e-12);
    // invalid crop rejected
    assert!(Roi {
        x0: 10,
        y0: 0,
        w: 100,
        h: 5,
        rotation: Rotation(0)
    }
    .validate(50, 10)
    .is_err());
    // rotation on an offset crop keeps absolute original centers
    let r2 = Roi {
        x0: 17,
        y0: 9,
        w: 11,
        h: 7,
        rotation: Rotation(90),
    };
    let (cx, cy) = r2.map_center(0, 0);
    let (ox, oy) = r2.map_pixel(0, 0);
    assert_eq!((ox, oy), (27, 9));
    assert!((cx - 27.5).abs() < 1e-12 && (cy - 9.5).abs() < 1e-12);
}
