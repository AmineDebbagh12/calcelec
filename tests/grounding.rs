use calcelec::{
    electrical::*,
    geometry::{Coordinate, Geometry},
    load_flow::CalculationError,
    model::{Equipment, Network, Object, Phase},
    reference_cases,
    unbalanced::{self, Options},
};
use uuid::Uuid;
fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{actual} != {expected} (ε={tolerance})"
    );
}
fn single(ground: Option<Impedance>) -> Network {
    let mut n = reference_cases::four_wire();
    for o in n.objects.values_mut() {
        match &mut o.equipment {
            Equipment::Bus { .. } if o.name == "Client BT" => {
                let Some(ElectricalData::Bus(d)) = &mut o.electrical else {
                    unreachable!()
                };
                d.neutral_grounding =
                    ground.map_or(Grounding::Isolated, |ohm| Grounding::Impedance { ohm });
            }
            Equipment::Line { .. } => {
                let Some(ElectricalData::Line(d)) = &mut o.electrical else {
                    unreachable!()
                };
                d.series_ohm_per_km = vec![vec![Impedance::ZERO; 4]; 4];
                for (i, row) in d.series_ohm_per_km.iter_mut().enumerate() {
                    row[i] = Impedance {
                        r: if i == 3 { 0.8 } else { 0.4 },
                        x: 0.1,
                    };
                }
            }
            Equipment::Load {
                p_w, q_var, phase, ..
            } => {
                *p_w = 10000.0;
                *q_var = 2000.0;
                *phase = Phase::A;
                o.electrical = Some(ElectricalData::Injection(InjectionData {
                    connection: InjectionConnection::PhaseNeutral {
                        phase: Conductor::A,
                    },
                    fault_contribution: FaultContribution::None,
                }));
            }
            _ => {}
        }
    }
    n
}
fn analytic(vs: f64, p: f64, q: f64, r: f64, x: f64) -> (f64, f64, f64, f64) {
    let b = 2.0 * (r * p + x * q) - vs * vs;
    let c = (r * r + x * x) * (p * p + q * q);
    let squared = (-b + (b * b - 4.0 * c).sqrt()) / 2.0;
    let vr = (squared + r * p + x * q) / vs;
    let vi = -(x * p - r * q) / vs;
    (
        vr,
        vi,
        (p * vr + q * vi) / squared,
        (p * vi - q * vr) / squared,
    )
}
fn multiply(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}
fn divide(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let norm = b.0 * b.0 + b.1 * b.1;
    (
        (a.0 * b.0 + a.1 * b.1) / norm,
        (a.1 * b.0 - a.0 * b.1) / norm,
    )
}
fn set_source_grounding(network: &mut Network, impedance: Impedance) {
    for object in network.objects.values_mut() {
        match &mut object.electrical {
            Some(ElectricalData::Bus(data)) if object.name == "Source BT" => {
                data.neutral_grounding = Grounding::Impedance { ohm: impedance };
            }
            Some(ElectricalData::Source(data)) => {
                data.neutral_grounding = Grounding::Impedance { ohm: impedance };
            }
            _ => {}
        }
    }
}
#[test]
fn finite_grounding_matches_analytical_phase_and_parallel_return_circuit() {
    // Zphase=.04+j.01; Zn=.08+j.01; Zg=2*Zn => Zequivalent return=2/3*Zn.
    let n = single(Some(Impedance { r: 0.16, x: 0.02 }));
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    let end = n
        .objects
        .values()
        .find(|o| o.name == "Client BT")
        .unwrap()
        .id;
    let (vr, vi, ir, ii) = analytic(
        400.0 / 3.0_f64.sqrt(),
        10000.0,
        2000.0,
        0.04 + 0.08 * 2.0 / 3.0,
        0.01 + 0.01 * 2.0 / 3.0,
    );
    let bus = r.buses.iter().find(|b| b.id == end).unwrap();
    let g = r.groundings.iter().find(|g| g.bus_id == end).unwrap();
    let line = &r.branches[0];
    near(bus.phase_neutral[0].magnitude_v, vr.hypot(vi), 1e-6);
    near(
        bus.phase_neutral[0].angle_deg,
        vi.atan2(vr).to_degrees(),
        1e-7,
    );
    near(line.currents_a[0].re, ir, 1e-6);
    near(line.currents_a[0].im, ii, 1e-6);
    near(line.currents_a[3].re, -ir * 2.0 / 3.0, 1e-6);
    near(line.currents_a[3].im, -ii * 2.0 / 3.0, 1e-6);
    near(g.current_to_earth_a.re, ir / 3.0, 1e-6);
    near(g.current_to_earth_a.im, ii / 3.0, 1e-6);
    near(
        g.voltage_v.re,
        0.16 * g.current_to_earth_a.re - 0.02 * g.current_to_earth_a.im,
        1e-8,
    );
    near(
        g.voltage_v.im,
        0.16 * g.current_to_earth_a.im + 0.02 * g.current_to_earth_a.re,
        1e-8,
    );
    let norm = ir * ir + ii * ii;
    near(r.line_loss_w, (0.04 + 0.08 * 4.0 / 9.0) * norm, 1e-5);
    near(r.grounding_loss_w, 0.16 / 9.0 * norm, 1e-5);
    near(r.grounding_loss_var, 0.02 / 9.0 * norm, 1e-5);
    near(r.total_loss_w, r.line_loss_w + r.grounding_loss_w, 1e-9);
    assert!(r.balance_error_va < 1e-3);
    let source = r.groundings.iter().find(|g| g.bus_id != end).unwrap();
    near(source.current_to_earth_a.re, -g.current_to_earth_a.re, 1e-9);
    near(source.current_to_earth_a.im, -g.current_to_earth_a.im, 1e-9);
}

#[test]
fn finite_source_grounding_matches_the_complete_parallel_return_circuit() {
    let downstream = Impedance { r: 0.16, x: 0.02 };
    let source_impedance = Impedance { r: 0.04, x: 0.01 };
    let mut network = single(Some(downstream));
    set_source_grounding(&mut network, source_impedance);

    // Zn=.08+j.01 in parallel with Zg+Zsource=.20+j.03.
    let neutral = (0.08, 0.01);
    let earth_path = (
        downstream.r + source_impedance.r,
        downstream.x + source_impedance.x,
    );
    let parallel = divide(multiply(neutral, earth_path), (0.28, 0.04));
    let (vr, vi, ir, ii) = analytic(
        400.0 / 3.0_f64.sqrt(),
        10000.0,
        2000.0,
        0.04 + parallel.0,
        0.01 + parallel.1,
    );
    let earth_current = multiply((ir, ii), divide(neutral, (0.28, 0.04)));
    let result = unbalanced::solve(&network, &Options::default()).unwrap();
    let source_id = network
        .objects
        .values()
        .find(|object| object.name == "Source BT")
        .unwrap()
        .id;
    let end_id = network
        .objects
        .values()
        .find(|object| object.name == "Client BT")
        .unwrap()
        .id;
    let bus = result.buses.iter().find(|bus| bus.id == end_id).unwrap();
    let source = result
        .groundings
        .iter()
        .find(|ground| ground.bus_id == source_id)
        .unwrap();
    let electrode = result
        .groundings
        .iter()
        .find(|ground| ground.bus_id == end_id)
        .unwrap();

    near(bus.phase_neutral[0].magnitude_v, vr.hypot(vi), 1e-6);
    near(electrode.current_to_earth_a.re, earth_current.0, 1e-6);
    near(electrode.current_to_earth_a.im, earth_current.1, 1e-6);
    near(source.current_to_earth_a.re, -earth_current.0, 1e-9);
    near(source.current_to_earth_a.im, -earth_current.1, 1e-9);
    let source_voltage = multiply(
        (source_impedance.r, source_impedance.x),
        (-earth_current.0, -earth_current.1),
    );
    near(source.voltage_v.re, source_voltage.0, 1e-8);
    near(source.voltage_v.im, source_voltage.1, 1e-8);
    let current_squared = earth_current.0.powi(2) + earth_current.1.powi(2);
    near(
        result.grounding_loss_w,
        (downstream.r + source_impedance.r) * current_squared,
        1e-6,
    );
    near(
        result.grounding_loss_var,
        (downstream.x + source_impedance.x) * current_squared,
        1e-6,
    );
    assert!(result.balance_error_va < 1e-3);

    let mut inconsistent = network;
    let root = inconsistent.objects.get_mut(&source_id).unwrap();
    let Some(ElectricalData::Bus(data)) = &mut root.electrical else {
        unreachable!()
    };
    data.neutral_grounding = Grounding::Impedance {
        ohm: Impedance::ZERO,
    };
    let error = unbalanced::solve(&inconsistent, &Options::default()).unwrap_err();
    assert!(matches!(error, CalculationError::Unsupported(_)));
    assert!(error.to_string().contains("même impédance"));
}
#[test]
fn ideal_downstream_grounding_fixes_neutral_voltage_and_can_take_the_whole_return() {
    let n = single(Some(Impedance::ZERO));
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    let (vr, vi, ir, ii) = analytic(400.0 / 3.0_f64.sqrt(), 10000.0, 2000.0, 0.04, 0.01);
    let bus = r
        .buses
        .iter()
        .find(|b| n.objects[&b.id].name == "Client BT")
        .unwrap();
    near(bus.neutral_displacement_v, 0.0, 1e-9);
    near(bus.phase_neutral[0].magnitude_v, vr.hypot(vi), 1e-6);
    near(r.branches[0].currents_a[3].magnitude(), 0.0, 1e-8);
    near(r.branches[0].currents_a[0].magnitude(), ir.hypot(ii), 1e-6);
    assert_eq!(r.grounding_loss_w, 0.0);
    near(r.total_loss_w, 0.04 * (ir * ir + ii * ii), 1e-5);
    assert!(r.balance_error_va < 1e-3);
}
fn two_grounds(ideal: bool) -> (Network, Uuid, Uuid) {
    // One resistive feeder followed by a perfect four-pole switch. Two earth resistors
    // at its receiving end are in parallel, providing an independent closed form.
    let mut n = single(Some(Impedance {
        r: if ideal { 0.0 } else { 0.2 },
        x: 0.0,
    }));
    let middle = n
        .objects
        .values()
        .find(|o| o.name == "Client BT")
        .unwrap()
        .id;
    let mut bus = n.objects[&middle].clone();
    bus.id = Uuid::new_v4();
    bus.name = "Aval interrupteur".into();
    bus.geometry = Geometry::Point(Coordinate(110.0, 0.0));
    let end = bus.id;
    let Some(ElectricalData::Bus(d)) = &mut bus.electrical else {
        unreachable!()
    };
    d.neutral_grounding = Grounding::Impedance {
        ohm: Impedance {
            r: if ideal { 0.0 } else { 0.3 },
            x: 0.0,
        },
    };
    n.objects.insert(end, bus);
    for o in n.objects.values_mut() {
        match &mut o.equipment {
            Equipment::Load { bus, q_var, .. } => {
                *bus = end;
                *q_var = 0.0;
            }
            Equipment::Line { .. } => {
                let Some(ElectricalData::Line(d)) = &mut o.electrical else {
                    unreachable!()
                };
                for row in &mut d.series_ohm_per_km {
                    for z in row {
                        z.x = 0.0;
                    }
                }
            }
            _ => {}
        }
    }
    let mut switch = Object::new(
        "Interrupteur idéal",
        Equipment::Switch {
            from: middle,
            to: end,
            closed: true,
        },
    );
    switch.electrical = Some(ElectricalData::Switch {
        conductors: vec![Conductor::A, Conductor::B, Conductor::C, Conductor::N],
    });
    switch.geometry = Geometry::Point(Coordinate(105.0, 0.0));
    n.objects.insert(switch.id, switch);
    (n, middle, end)
}
#[test]
fn two_groundings_share_current_and_match_the_resistive_parallel_solution() {
    let (n, middle, end) = two_grounds(false);
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    // Rg1||Rg2=.12; .08||.12=.048; phase .04 => loop .088 ohm.
    let (vr, _, ir, _) = analytic(400.0 / 3.0_f64.sqrt(), 10000.0, 0.0, 0.088, 0.0);
    let bus = r.buses.iter().find(|b| b.id == end).unwrap();
    near(bus.phase_neutral[0].magnitude_v, vr, 1e-6);
    near(bus.neutral_displacement_v, 0.048 * ir, 1e-6);
    let g1 = r.groundings.iter().find(|g| g.bus_id == middle).unwrap();
    let g2 = r.groundings.iter().find(|g| g.bus_id == end).unwrap();
    near(g1.current_to_earth_a.re, 0.24 * ir, 1e-6);
    near(g2.current_to_earth_a.re, 0.16 * ir, 1e-6);
    let trunk = r.branches.iter().find(|b| b.to_bus == middle).unwrap();
    let tail = r.branches.iter().find(|b| b.to_bus == end).unwrap();
    near(trunk.currents_a[3].re, -0.6 * ir, 1e-6);
    near(tail.currents_a[3].re, -0.84 * ir, 1e-6);
    near(
        trunk.currents_a[3].re - tail.currents_a[3].re,
        g1.current_to_earth_a.re,
        1e-8,
    );
    near(r.total_loss_w, 0.088 * ir * ir, 1e-5);
    near(
        r.grounding_loss_w,
        (0.2 * 0.24_f64.powi(2) + 0.3 * 0.16_f64.powi(2)) * ir * ir,
        1e-5,
    );
    assert!(r.balance_error_va < 1e-3);
}
#[test]
fn redundant_ideal_groundings_return_an_explicit_error_instead_of_arbitrary_currents() {
    let (n, _, _) = two_grounds(true);
    n.validate().unwrap();
    let error = unbalanced::solve(&n, &Options::default()).unwrap_err();
    assert!(matches!(error, CalculationError::Unsupported(_)));
    assert!(error.to_string().contains("singulier"));
}
#[test]
fn grounding_reduces_neutral_displacement_without_losing_earth_losses_in_the_balance() {
    let isolated = unbalanced::solve(&single(None), &Options::default()).unwrap();
    let grounded = unbalanced::solve(
        &single(Some(Impedance { r: 0.16, x: 0.02 })),
        &Options::default(),
    )
    .unwrap();
    let vn = |r: &unbalanced::LoadFlow| {
        r.buses
            .iter()
            .map(|b| b.neutral_displacement_v)
            .fold(0.0, f64::max)
    };
    assert!(vn(&grounded) < vn(&isolated));
    assert!(
        grounded.branches[0].currents_a[3].magnitude()
            < isolated.branches[0].currents_a[3].magnitude()
    );
    assert!(grounded.grounding_loss_w > 0.0);
    assert_eq!(isolated.grounding_loss_w, 0.0);
    near(
        grounded.source_p_w - grounded.net_demand_p_w,
        grounded.line_loss_w + grounded.grounding_loss_w,
        1e-3,
    );
}
#[test]
fn capacitive_grounding_is_preserved_and_reactive_power_is_signed() {
    let n = single(Some(Impedance { r: 0.16, x: -0.03 }));
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    assert!(r.grounding_loss_w > 0.0);
    assert!(r.grounding_loss_var < 0.0);
    assert!(r.balance_error_va < 1e-3);
    let g = r
        .groundings
        .iter()
        .find(|g| n.objects[&g.bus_id].name == "Client BT")
        .unwrap();
    near(
        g.voltage_v.re,
        0.16 * g.current_to_earth_a.re + 0.03 * g.current_to_earth_a.im,
        1e-8,
    );
    near(
        g.voltage_v.im,
        0.16 * g.current_to_earth_a.im - 0.03 * g.current_to_earth_a.re,
        1e-8,
    );
}
#[test]
fn no_grounding_case_retains_a_zero_earth_return_and_separates_losses() {
    let n = reference_cases::unbalanced_bt();
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    assert_eq!(r.groundings.len(), 1);
    assert_eq!(r.groundings[0].current_to_earth_a.magnitude(), 0.0);
    assert_eq!(r.grounding_loss_w, 0.0);
    near(r.line_loss_w, r.total_loss_w, 1e-12);
    assert!(r.balance_error_va < 1e-3);
}

#[test]
fn multi_node_demo_satisfies_neutral_kcl_and_power_balance_with_mutual_couplings() {
    let n = reference_cases::grounded_bt();
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    assert_eq!(r.groundings.len(), 3);
    assert!(r.grounding_loss_w > 0.0);
    assert!(r.balance_error_va < 1e-3);
    for bus in &r.buses {
        let Some(incoming) = r.branches.iter().find(|b| b.to_bus == bus.id) else {
            continue;
        };
        let outgoing: (f64, f64) = r
            .branches
            .iter()
            .filter(|b| b.from_bus == bus.id)
            .fold((0.0, 0.0), |(re, im), b| {
                (re + b.currents_a[3].re, im + b.currents_a[3].im)
            });
        let mut local = (0.0, 0.0);
        for object in n.objects.values() {
            let (target, p, q, sign) = match object.equipment {
                Equipment::Load {
                    bus, p_w, q_var, ..
                } => (bus, p_w, q_var, 1.0),
                Equipment::Generator {
                    bus, p_w, q_var, ..
                } => (bus, p_w, q_var, -1.0),
                _ => continue,
            };
            if target != bus.id {
                continue;
            }
            let Some(ElectricalData::Injection(data)) = &object.electrical else {
                unreachable!()
            };
            let InjectionConnection::PhaseNeutral { phase } = data.connection else {
                unreachable!()
            };
            let i = match phase {
                Conductor::A => 0,
                Conductor::B => 1,
                Conductor::C => 2,
                Conductor::N => unreachable!(),
            };
            let v = &bus.conductor_voltages_v;
            let re = v[i].re - v[3].re;
            let im = v[i].im - v[3].im;
            let norm = re * re + im * im;
            local.0 -= sign * (p * re + q * im) / norm;
            local.1 -= sign * (p * im - q * re) / norm;
        }
        let ground = r.groundings.iter().find(|g| g.bus_id == bus.id).unwrap();
        near(
            incoming.currents_a[3].re - outgoing.0 - local.0,
            ground.current_to_earth_a.re,
            3e-6,
        );
        near(
            incoming.currents_a[3].im - outgoing.1 - local.1,
            ground.current_to_earth_a.im,
            3e-6,
        );
    }
    near(
        r.line_loss_w,
        r.branches.iter().map(|b| b.loss_w).sum(),
        1e-8,
    );
    near(
        r.grounding_loss_w,
        r.groundings.iter().map(|g| g.loss_w).sum(),
        1e-8,
    );
    let json = serde_json::to_string(&r).unwrap();
    let decoded: unbalanced::LoadFlow = serde_json::from_str(&json).unwrap();
    near(decoded.grounding_loss_w, r.grounding_loss_w, 1e-10);
}
