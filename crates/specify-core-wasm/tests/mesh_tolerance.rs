use specify_core_wasm::session::Session;

#[test]
fn snapshot_mesh_option_is_measured_and_none_is_byte_identical() {
    let session = Session::open(
        "sphere.step",
        include_bytes!("../../../tests/fixtures/parts/mesh_sphere.step"),
    )
    .unwrap();
    assert_eq!(
        session.snapshot().to_string(),
        session
            .snapshot_with_max_deviation(None)
            .unwrap()
            .to_string()
    );
    for limit in [0.1, 0.01, 1.0] {
        let value = session.snapshot_with_max_deviation(Some(limit)).unwrap();
        assert_eq!(value["mesh"]["max_deviation_mm"], limit);
        assert!(value["mesh"]["achieved_deviation_mm"].as_f64().unwrap() <= limit);
    }
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            session
                .snapshot_with_max_deviation(Some(invalid))
                .unwrap_err()
                .contains("positive finite")
        );
    }
}

#[test]
fn whole_torus_meets_both_requested_bounds() {
    let session = Session::open(
        "torus.step",
        include_bytes!("../../../tests/fixtures/parts/mesh_torus.step"),
    )
    .unwrap();
    for limit in [0.1, 0.01] {
        let snapshot = session.snapshot_with_max_deviation(Some(limit)).unwrap();
        assert!(snapshot["mesh"]["missing"].as_array().unwrap().is_empty());
        assert!(snapshot["mesh"]["achieved_deviation_mm"].as_f64().unwrap() <= limit);
    }
}
