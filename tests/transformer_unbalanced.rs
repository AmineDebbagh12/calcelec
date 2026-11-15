use calcelec::{
    electrical::*,
    geometry::{Coordinate, Geometry},
    load_flow::CalculationError,
    model::{Equipment, Object},
    reference_cases,
    unbalanced::{self, Options, Phasor},
};

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{actual} != {expected} (ε={tolerance})"
    );
}

fn add(a: Phasor, b: Phasor) -> Phasor {
    Phasor {
        re: a.re + b.re,
        im: a.im + b.im,
    }
}

fn scale(a: Phasor, value: f64) -> Phasor {
    Phasor {
        re: a.re * value,
        im: a.im * value,
    }
}

fn mul(a: Phasor, b: Phasor) -> Phasor {
    Phasor {
        re: a.re * b.re - a.im * b.im,
        im: a.re * b.im + a.im * b.re,
    }
}

fn sequence(phases: [Phasor; 3]) -> [Phasor; 3] {
    let a = Phasor {
        re: -0.5,
        im: 3.0_f64.sqrt() / 2.0,
    };
    let a2 = mul(a, a);
    [
        scale(add(add(phases[0], phases[1]), phases[2]), 1.0 / 3.0),
        scale(
            add(add(phases[0], mul(a, phases[1])), mul(a2, phases[2])),
            1.0 / 3.0,
        ),
        scale(
            add(add(phases[0], mul(a2, phases[1])), mul(a, phases[2])),
            1.0 / 3.0,
        ),
    ]
}

#[test]
fn dyn11_no_load_applies_ratio_and_clock_without_inventing_losses() {
    let mut network = reference_cases::dyn_transformer();
    network
        .objects
        .retain(|_, object| !matches!(object.equipment, Equipment::Load { .. }));
    let result = unbalanced::solve(&network, &Options::default()).unwrap();
    let secondary = result
        .buses
        .iter()
        .find(|bus| network.objects[&bus.id].name == "Secondaire 400 V")
        .unwrap();
    near(
        secondary.phase_neutral[0].magnitude_v,
        400.0 / 3.0_f64.sqrt(),
        1e-9,
    );
    near(secondary.phase_neutral[0].angle_deg, 30.0, 1e-10);
    near(secondary.phase_neutral[1].angle_deg, -90.0, 1e-10);
    near(result.total_loss_w, 0.0, 1e-10);
    assert!(
        result.branches[0]
            .currents_from_a
            .iter()
            .chain(&result.branches[0].currents_to_a)
            .all(|current| current.magnitude() < 1e-10)
    );
}

#[test]
fn dyn11_fixed_tap_changes_the_no_load_ratio_explicitly() {
    let mut network = reference_cases::dyn_transformer();
    network
        .objects
        .retain(|_, object| !matches!(object.equipment, Equipment::Load { .. }));
    let transformer = network
        .objects
        .values_mut()
        .find(|object| matches!(object.equipment, Equipment::Transformer { .. }))
        .unwrap();
    let Some(ElectricalData::Transformer(data)) = &mut transformer.electrical else {
        unreachable!()
    };
    data.tap.as_mut().unwrap().position = 1;
    let result = unbalanced::solve(&network, &Options::default()).unwrap();
    let secondary = result
        .buses
        .iter()
        .find(|bus| network.objects[&bus.id].name == "Secondaire 400 V")
        .unwrap();
    near(
        secondary.phase_neutral[0].magnitude_v,
        400.0 / (3.0_f64.sqrt() * 1.025),
        1e-9,
    );
    near(secondary.phase_neutral[0].angle_deg, 30.0, 1e-10);
}

#[test]
fn dyn11_unbalanced_case_blocks_homopolar_current_upstream_and_balances_power() {
    let network = reference_cases::dyn_transformer();
    let result = unbalanced::solve(&network, &Options::default()).unwrap();
    let branch = &result.branches[0];
    let upstream_sum = branch.currents_from_a[..3]
        .iter()
        .copied()
        .fold(Phasor { re: 0.0, im: 0.0 }, add);
    near(upstream_sum.magnitude(), 0.0, 1e-10);
    near(branch.currents_from_a[3].magnitude(), 0.0, 1e-12);
    let downstream_sum = branch
        .currents_to_a
        .iter()
        .copied()
        .fold(Phasor { re: 0.0, im: 0.0 }, add);
    near(downstream_sum.magnitude(), 0.0, 1e-10);

    let seq = sequence([
        branch.currents_to_a[0],
        branch.currents_to_a[1],
        branch.currents_to_a[2],
    ]);
    let transformer = network
        .objects
        .values()
        .find(|object| matches!(object.equipment, Equipment::Transformer { .. }))
        .unwrap();
    let Equipment::Transformer {
        rated_va,
        uk_percent,
        ..
    } = transformer.equipment
    else {
        unreachable!()
    };
    let Some(ElectricalData::Transformer(data)) = &transformer.electrical else {
        unreachable!()
    };
    let positive = data.series_pu(rated_va, uk_percent).unwrap();
    let base_to = data.to.rated_voltage_ll_v.powi(2) / rated_va;
    let zero = data.zero_sequence_ohm_referred_from.unwrap();
    let ratio = data.ratio();
    let expected_w = 3.0
        * (positive.r * base_to * (seq[1].magnitude().powi(2) + seq[2].magnitude().powi(2))
            + zero.r / ratio.powi(2) * seq[0].magnitude().powi(2));
    let expected_var = 3.0
        * (positive.x * base_to * (seq[1].magnitude().powi(2) + seq[2].magnitude().powi(2))
            + zero.x / ratio.powi(2) * seq[0].magnitude().powi(2));
    near(result.transformer_loss_w, expected_w, 1e-5);
    near(result.transformer_loss_var, expected_var, 1e-5);
    near(result.line_loss_w, 0.0, 1e-12);
    near(
        result.source_p_w,
        result.net_demand_p_w + result.transformer_loss_w,
        1e-4,
    );
    assert!(result.balance_error_va < 1e-3);
    assert!(branch.loading_percent.is_some_and(|loading| loading > 0.0));
}

#[test]
fn three_wire_hta_line_can_feed_the_dyn_boundary_without_a_fictitious_neutral() {
    let mut network = reference_cases::dyn_transformer();
    let primary = network
        .objects
        .values()
        .find(|object| object.name == "Primaire 20 kV")
        .unwrap()
        .id;
    let mut upstream = Object::new("Amont HTA", Equipment::Bus { voltage_v: 20000.0 });
    upstream.geometry = Geometry::Point(Coordinate(-100.0, 0.0));
    upstream.electrical = Some(ElectricalData::Bus(BusData {
        conductors: vec![Conductor::A, Conductor::B, Conductor::C],
        neutral_grounding: Grounding::NotApplicable,
    }));
    let source = network
        .objects
        .values_mut()
        .find(|object| matches!(object.equipment, Equipment::Source { .. }))
        .unwrap();
    let Equipment::Source { bus, .. } = &mut source.equipment else {
        unreachable!()
    };
    *bus = upstream.id;
    let mut line = Object::new(
        "Ligne HTA ABC",
        Equipment::Line {
            from: upstream.id,
            to: primary,
            length_m: 100.0,
            r_ohm_per_km: 0.2,
            x_ohm_per_km: 0.3,
            max_current_a: 100.0,
        },
    );
    line.geometry = Geometry::Polyline(vec![Coordinate(-100.0, 0.0), Coordinate(0.0, 0.0)]);
    line.electrical = Some(ElectricalData::Line(LineData {
        conductors: vec![Conductor::A, Conductor::B, Conductor::C],
        series_ohm_per_km: vec![
            vec![
                Impedance { r: 0.2, x: 0.3 },
                Impedance::ZERO,
                Impedance::ZERO,
            ],
            vec![
                Impedance::ZERO,
                Impedance { r: 0.2, x: 0.3 },
                Impedance::ZERO,
            ],
            vec![
                Impedance::ZERO,
                Impedance::ZERO,
                Impedance { r: 0.2, x: 0.3 },
            ],
        ],
        shunt_siemens_per_km: None,
    }));
    network.objects.insert(upstream.id, upstream);
    network.objects.insert(line.id, line);
    let result = unbalanced::solve(&network, &Options::default()).unwrap();
    assert_eq!(result.branches.len(), 2);
    assert!(result.line_loss_w > 0.0);
    assert!(result.transformer_loss_w > 0.0);
    assert!(result.balance_error_va < 1e-3);
    let hta = result
        .buses
        .iter()
        .find(|bus| network.objects[&bus.id].name == "Amont HTA")
        .unwrap();
    assert_eq!(
        hta.conductors,
        vec![Conductor::A, Conductor::B, Conductor::C]
    );
}

#[test]
fn unsupported_transformer_models_are_rejected_explicitly() {
    let mut network = reference_cases::dyn_transformer();
    let transformer = network
        .objects
        .values_mut()
        .find(|object| matches!(object.equipment, Equipment::Transformer { .. }))
        .unwrap();
    let Some(ElectricalData::Transformer(data)) = &mut transformer.electrical else {
        unreachable!()
    };
    data.zero_sequence_ohm_referred_from = None;
    assert!(matches!(
        unbalanced::solve(&network, &Options::default()),
        Err(CalculationError::Unsupported(message)) if message.contains("homopolaire")
    ));

    let mut network = reference_cases::dyn_transformer();
    let transformer = network
        .objects
        .values_mut()
        .find(|object| matches!(object.equipment, Equipment::Transformer { .. }))
        .unwrap();
    let Some(ElectricalData::Transformer(data)) = &mut transformer.electrical else {
        unreachable!()
    };
    data.iron_loss_w = Some(10.0);
    data.no_load_current_percent = Some(1.0);
    assert!(matches!(
        unbalanced::solve(&network, &Options::default()),
        Err(CalculationError::Unsupported(message)) if message.contains("pertes fer")
    ));

    let mut network = reference_cases::dyn_transformer();
    let transformer = network
        .objects
        .values_mut()
        .find(|object| matches!(object.equipment, Equipment::Transformer { .. }))
        .unwrap();
    let Some(ElectricalData::Transformer(data)) = &mut transformer.electrical else {
        unreachable!()
    };
    data.zero_sequence_ohm_referred_from = Some(Impedance::ZERO);
    assert!(unbalanced::solve(&network, &Options::default()).is_ok());
}

#[test]
fn downstream_grounding_is_solved_inside_the_dyn_galvanic_zone() {
    let network = reference_cases::dyn_grounded_transformer();
    let secondary = network
        .objects
        .values()
        .find(|object| object.name == "Secondaire 400 V")
        .unwrap()
        .id;
    let downstream_id = network
        .objects
        .values()
        .find(|object| object.name == "Client BT avec terre")
        .unwrap()
        .id;
    let result = unbalanced::solve(&network, &Options::default()).unwrap();
    let electrode = result
        .groundings
        .iter()
        .find(|ground| ground.bus_id == downstream_id)
        .unwrap();
    let anchor = result
        .groundings
        .iter()
        .find(|ground| ground.bus_id == secondary)
        .unwrap();
    near(
        electrode.current_to_earth_a.re + anchor.current_to_earth_a.re,
        0.0,
        1e-9,
    );
    near(
        electrode.current_to_earth_a.im + anchor.current_to_earth_a.im,
        0.0,
        1e-9,
    );
    near(
        electrode.voltage_v.re,
        electrode.current_to_earth_a.re,
        1e-8,
    );
    near(
        electrode.voltage_v.im,
        electrode.current_to_earth_a.im,
        1e-8,
    );
    near(anchor.voltage_v.magnitude(), 0.0, 1e-12);
    assert!(electrode.current_to_earth_a.magnitude() > 0.1);
    assert!(result.grounding_loss_w > 0.0);
    near(
        result.grounding_loss_w,
        electrode.current_to_earth_a.magnitude().powi(2),
        1e-8,
    );
    assert!(result.balance_error_va < 1e-3);
}
