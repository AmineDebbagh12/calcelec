use calcelec::{
    electrical::{BusData, Conductor, ElectricalData, Grounding, Impedance, SourceData},
    geometry::{Coordinate, Geometry},
    model::{Equipment, Network, Object},
    reference_cases,
    short_circuit::{self, Options, ProtectionRequirement, ResidualCriterion, ResidualRequirement},
};
use std::collections::BTreeMap;

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} != {expected} (ε={tolerance})"
    );
}

fn source_only() -> Network {
    let mut bus = Object::new(
        "Bus HTA",
        Equipment::Bus {
            voltage_v: 20_000.0,
        },
    );
    bus.geometry = Geometry::Point(Coordinate(0.0, 0.0));
    bus.electrical = Some(ElectricalData::Bus(BusData {
        conductors: vec![Conductor::A, Conductor::B, Conductor::C],
        neutral_grounding: Grounding::NotApplicable,
    }));
    let mut source = Object::new(
        "Source",
        Equipment::Source {
            bus: bus.id,
            voltage_pu: 1.0,
        },
    );
    source.geometry = Geometry::Point(Coordinate(0.0, 0.0));
    source.electrical = Some(ElectricalData::Source(SourceData {
        positive_sequence_ohm: Some(Impedance { r: 0.5, x: 2.0 }),
        negative_sequence_ohm: Some(Impedance { r: 0.5, x: 2.0 }),
        zero_sequence_ohm: Some(Impedance { r: 1.0, x: 4.0 }),
        neutral_grounding: Grounding::NotApplicable,
    }));
    Network {
        name: "Source analytique".into(),
        frequency_hz: 50.0,
        coordinate_reference: Default::default(),
        technologies: BTreeMap::new(),
        objects: BTreeMap::from([(bus.id, bus), (source.id, source)]),
    }
}

#[test]
fn source_only_matches_sequence_closed_forms_for_iccbmin_and_three_i0() {
    let network = source_only();
    let bus_id = network
        .objects
        .values()
        .find(|object| matches!(object.equipment, Equipment::Bus { .. }))
        .unwrap()
        .id;
    let options = Options {
        protection_requirements: vec![
            ProtectionRequirement {
                name: "Plafond résiduel".into(),
                bus_id,
                minimum_icc_b_a: Some(4_000.0),
                residual: Some(ResidualRequirement {
                    criterion: ResidualCriterion::MaximumAdmissible,
                    threshold_a: 4_100.0,
                }),
            },
            ProtectionRequirement {
                name: "Seuil de détection".into(),
                bus_id,
                minimum_icc_b_a: None,
                residual: Some(ResidualRequirement {
                    criterion: ResidualCriterion::MinimumDetection,
                    threshold_a: 4_100.0,
                }),
            },
        ],
        ..Options::default()
    };
    let result = short_circuit::solve(&network, &options).unwrap();
    let bus = &result.buses[0];
    let z12 = (1.0_f64.powi(2) + 4.0_f64.powi(2)).sqrt();
    near(bus.icc_b_min_a, 0.95 * 20_000.0 / z12, 1e-9);
    let z120 = (2.0_f64.powi(2) + 8.0_f64.powi(2)).sqrt();
    near(
        bus.three_i0_a.unwrap(),
        3.0_f64.sqrt() * 0.95 * 20_000.0 / z120,
        1e-9,
    );
    assert_eq!(result.minimum_icc_b_bus, bus.bus_id);
    assert_eq!(result.maximum_three_i0_bus, Some(bus.bus_id));
    let assessment = &result.protection_assessments[0];
    assert_eq!(assessment.icc_b_passed, Some(true));
    assert_eq!(assessment.residual_passed, Some(true));
    assert!(assessment.icc_b_margin_a.unwrap() > 0.0);
    assert!(assessment.residual_margin_a.unwrap() > 0.0);
    let detection = &result.protection_assessments[1];
    assert_eq!(detection.residual_passed, Some(false));
    assert!(detection.residual_margin_a.unwrap() < 0.0);
}

#[test]
fn rural_sequence_paths_include_lines_reset_zero_at_dyn_and_flag_explicit_neutral() {
    let network = reference_cases::rural_unbalanced();
    let result = short_circuit::solve(&network, &Options::default()).unwrap();
    assert_eq!(result.buses.len(), 8);

    let named = |name: &str| {
        let id = network
            .objects
            .values()
            .find(|object| object.name == name)
            .unwrap()
            .id;
        result.buses.iter().find(|bus| bus.bus_id == id).unwrap()
    };
    let first_hta = named("Poste rural 1 — HTA");
    near(first_hta.positive_sequence.r_ohm, 1.04, 1e-12);
    near(first_hta.positive_sequence.x_ohm, 2.405, 1e-12);

    let grounded_secondary = named("Poste rural 2 — BT");
    assert!(grounded_secondary.three_i0_a.is_some());
    assert!(grounded_secondary.zero_sequence.is_some());

    let terminal = named("Extrémité départ BT");
    assert!(terminal.icc_b_min_a.is_finite());
    assert!(terminal.three_i0_a.is_none());
    assert!(terminal.earth_fault_limitation.is_some());
}

#[test]
fn meshed_fault_network_is_rejected_explicitly_for_this_first_version() {
    let error =
        short_circuit::solve(&reference_cases::rural_meshed(), &Options::default()).unwrap_err();
    assert!(error.to_string().contains("mailles"));
}
