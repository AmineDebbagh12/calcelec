use calcelec::{
    electrical::*,
    load_flow::{self, CalculationError},
    model::{Equipment, Network, Phase},
    project::Project,
    reference_cases,
    unbalanced::{self, Options},
};
use std::sync::atomic::AtomicBool;
fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{actual} != {expected} (ε={tolerance})"
    );
}
fn single() -> Network {
    let mut n = reference_cases::four_wire();
    let load = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Load { .. }))
        .unwrap();
    if let Equipment::Load {
        p_w, q_var, phase, ..
    } = &mut load.equipment
    {
        *p_w = 10000.0;
        *q_var = 2000.0;
        *phase = Phase::A;
    }
    load.electrical = Some(ElectricalData::Injection(InjectionData {
        connection: InjectionConnection::PhaseNeutral {
            phase: Conductor::A,
        },
        fault_contribution: FaultContribution::None,
    }));
    n
}
fn analytic(vs: f64, p: f64, q: f64, r: f64, x: f64) -> (f64, f64, f64, f64) {
    let b = 2.0 * (r * p + x * q) - vs * vs;
    let c = (r * r + x * x) * (p * p + q * q);
    let squared = (-b + (b * b - 4.0 * c).sqrt()) / 2.0;
    let re = (squared + r * p + x * q) / vs;
    let im = -(x * p - r * q) / vs;
    (
        re,
        im,
        (p * re + q * im) / squared,
        (p * im - q * re) / squared,
    )
}
#[test]
fn phase_neutral_loop_matches_closed_form_including_mutual_coupling_and_neutral_drop() {
    let n = single();
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    let (vr, vi, ir, ii) = analytic(400.0 / 3.0_f64.sqrt(), 10000.0, 2000.0, 0.116, 0.012);
    let bus = r
        .buses
        .iter()
        .find(|b| n.objects[&b.id].name == "Client BT")
        .unwrap();
    let line = &r.branches[0];
    near(bus.phase_neutral[0].magnitude_v, vr.hypot(vi), 1e-6);
    near(
        bus.phase_neutral[0].angle_deg,
        vi.atan2(vr).to_degrees(),
        1e-6,
    );
    near(line.currents_a[0].re, ir, 1e-6);
    near(line.currents_a[0].im, ii, 1e-6);
    near(line.currents_a[3].re, -ir, 1e-6);
    near(line.currents_a[3].im, -ii, 1e-6);
    near(line.currents_a[1].magnitude(), 0.0, 1e-10);
    near(line.currents_a[2].magnitude(), 0.0, 1e-10);
    let nr = 0.078 * ir - 0.006 * ii;
    let ni = 0.078 * ii + 0.006 * ir;
    near(bus.conductor_voltages_v[3].re, nr, 1e-6);
    near(bus.conductor_voltages_v[3].im, ni, 1e-6);
    assert!(bus.neutral_displacement_v > 1.0);
    near(r.total_loss_w, 0.116 * (ir * ir + ii * ii), 1e-5);
    near(r.total_loss_var, 0.012 * (ir * ir + ii * ii), 1e-5);
    assert!(r.balance_error_va < 1e-3);
    assert!(bus.negative_sequence_percent > 0.0);
}
#[test]
fn balanced_wye_case_agrees_with_balanced_solver_and_cancels_neutral_current() {
    let n = reference_cases::four_wire();
    let a = unbalanced::solve(&n, &Options::default()).unwrap();
    let b = load_flow::solve(&n, &load_flow::Options::default()).unwrap();
    near(a.source_p_w, b.source_p_w, 1e-4);
    near(a.total_loss_w, b.total_loss_w, 1e-4);
    for bus in &a.buses {
        let balanced = b.buses.iter().find(|v| v.id == bus.id).unwrap();
        for phase in &bus.phase_neutral {
            near(phase.pu, balanced.voltage_pu, 1e-8);
        }
        near(bus.negative_sequence_percent, 0.0, 1e-9);
        near(bus.neutral_displacement_v, 0.0, 1e-9);
    }
    near(a.branches[0].currents_a[3].magnitude(), 0.0, 1e-9);
}
#[test]
fn phase_phase_load_returns_through_the_other_phase_without_neutral_current() {
    let mut n = single();
    let load = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Load { .. }))
        .unwrap();
    if let Equipment::Load { phase, .. } = &mut load.equipment {
        *phase = Phase::Abc;
    }
    let Some(ElectricalData::Injection(data)) = &mut load.electrical else {
        unreachable!()
    };
    data.connection = InjectionConnection::PhasePhase {
        from: Conductor::A,
        to: Conductor::B,
    };
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    let (vr, vi, ir, ii) = analytic(400.0, 10000.0, 2000.0, 0.076, 0.012);
    let bus = r
        .buses
        .iter()
        .find(|b| n.objects[&b.id].name == "Client BT")
        .unwrap();
    near(bus.phase_phase_v[0], vr.hypot(vi), 1e-6);
    let line = &r.branches[0];
    near(line.currents_a[0].magnitude(), ir.hypot(ii), 1e-6);
    near(line.currents_a[0].re, -line.currents_a[1].re, 1e-9);
    near(line.currents_a[3].magnitude(), 0.0, 1e-9);
    near(bus.neutral_displacement_v, 0.0, 1e-9);
    near(r.total_loss_w, 0.076 * (ir * ir + ii * ii), 1e-5);
}
#[test]
fn balanced_delta_matches_wye_total_power_and_voltages() {
    let mut n = reference_cases::four_wire();
    let before = unbalanced::solve(&n, &Options::default()).unwrap();
    let load = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Load { .. }))
        .unwrap();
    let Some(ElectricalData::Injection(d)) = &mut load.electrical else {
        unreachable!()
    };
    d.connection = InjectionConnection::ThreePhaseDelta;
    let after = unbalanced::solve(&n, &Options::default()).unwrap();
    near(after.source_p_w, before.source_p_w, 1e-5);
    near(after.branches[0].currents_a[3].magnitude(), 0.0, 1e-9);
    for (a, b) in after.buses.iter().zip(&before.buses) {
        for k in 0..3 {
            near(
                a.phase_neutral[k].magnitude_v,
                b.phase_neutral[k].magnitude_v,
                1e-6,
            );
        }
    }
}
#[test]
fn asymmetric_matrix_and_unbalanced_generation_are_computed_without_aggregation() {
    let mut n = single();
    let load = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Load { .. }))
        .unwrap();
    let Equipment::Load { bus, phase, .. } = load.equipment else {
        unreachable!()
    };
    load.equipment = Equipment::Generator {
        bus,
        p_w: 10000.0,
        q_var: 0.0,
        phase,
    };
    let line = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Line { .. }))
        .unwrap();
    let Some(ElectricalData::Line(data)) = &mut line.electrical else {
        unreachable!()
    };
    data.series_ohm_per_km[0][0].r = 0.5;
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    assert!(r.source_p_w < 0.0);
    assert!(r.total_loss_w > 0.0);
    assert!(r.branches[0].currents_a[3].magnitude() > 1.0);
    assert!(r.buses.iter().any(|b| b.phase_neutral[0].pu > 1.0));
    assert!(r.balance_error_va < 1e-3);
}
#[test]
fn permuting_conductor_order_or_stored_branch_direction_preserves_the_solution() {
    let mut n = single();
    let before = unbalanced::solve(&n, &Options::default()).unwrap();
    let line = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Line { .. }))
        .unwrap();
    if let Equipment::Line { from, to, .. } = &mut line.equipment {
        std::mem::swap(from, to);
    }
    let Some(ElectricalData::Line(data)) = &mut line.electrical else {
        unreachable!()
    };
    let old = data.clone();
    let order = [3, 2, 0, 1];
    data.conductors = order.iter().map(|i| old.conductors[*i]).collect();
    data.series_ohm_per_km = order
        .iter()
        .map(|i| {
            order
                .iter()
                .map(|j| old.series_ohm_per_km[*i][*j])
                .collect()
        })
        .collect();
    let after = unbalanced::solve(&n, &Options::default()).unwrap();
    near(after.source_p_w, before.source_p_w, 1e-9);
    near(after.total_loss_w, before.total_loss_w, 1e-9);
    assert_eq!(after.branches[0].from_bus, before.branches[0].from_bus);
}
#[test]
fn zero_impedance_neutral_is_valid_and_still_carries_return_current() {
    let mut n = single();
    let line = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Line { .. }))
        .unwrap();
    let Some(ElectricalData::Line(data)) = &mut line.electrical else {
        unreachable!()
    };
    for i in 0..4 {
        data.series_ohm_per_km[i][3] = Impedance::ZERO;
        data.series_ohm_per_km[3][i] = Impedance::ZERO;
    }
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    assert!(r.branches[0].currents_a[3].magnitude() > 1.0);
    assert!(r.buses.iter().all(|b| b.neutral_displacement_v == 0.0));
}
#[test]
fn unsupported_earthing_shunts_and_floating_star_are_not_silently_ignored() {
    for variant in 0..4 {
        let mut n = reference_cases::four_wire();
        match variant {
            0 => {
                let bus = n
                    .objects
                    .values_mut()
                    .find(|o| o.name == "Client BT")
                    .unwrap();
                let Some(ElectricalData::Bus(data)) = &mut bus.electrical else {
                    unreachable!()
                };
                data.neutral_grounding = Grounding::Impedance {
                    ohm: Impedance { r: 10.0, x: 0.0 },
                };
            }
            1 => {
                let line = n
                    .objects
                    .values_mut()
                    .find(|o| matches!(o.equipment, Equipment::Line { .. }))
                    .unwrap();
                let Some(ElectricalData::Line(data)) = &mut line.electrical else {
                    unreachable!()
                };
                data.shunt_siemens_per_km = Some(vec![vec![Admittance { g: 0.0, b: 0.01 }; 4]; 4]);
            }
            2 => {
                let load = n
                    .objects
                    .values_mut()
                    .find(|o| matches!(o.equipment, Equipment::Load { .. }))
                    .unwrap();
                let Some(ElectricalData::Injection(data)) = &mut load.electrical else {
                    unreachable!()
                };
                data.connection = InjectionConnection::ThreePhaseStar { neutral: false };
            }
            _ => {
                let line = n
                    .objects
                    .values_mut()
                    .find(|o| matches!(o.equipment, Equipment::Line { .. }))
                    .unwrap();
                line.electrical = None;
            }
        }
        n.validate().unwrap();
        assert!(matches!(
            unbalanced::solve(&n, &Options::default()),
            Err(CalculationError::Unsupported(_))
        ));
    }
    assert!(unbalanced::solve(&reference_cases::transformer(), &Options::default()).is_err());
}
#[test]
fn nonconvergence_cancellation_and_invalid_options_return_no_partial_results() {
    let n = single();
    assert!(matches!(
        unbalanced::solve(
            &n,
            &Options {
                max_iterations: 1,
                ..Options::default()
            }
        ),
        Err(CalculationError::NonConvergent { .. })
    ));
    assert!(matches!(
        unbalanced::solve_cancellable(&n, &Options::default(), &AtomicBool::new(true)),
        Err(CalculationError::Cancelled)
    ));
    assert!(matches!(
        unbalanced::solve(
            &n,
            &Options {
                tolerance_pu: f64::NAN,
                ..Options::default()
            }
        ),
        Err(CalculationError::InvalidOptions)
    ));
}
#[test]
fn study_records_version_engine_options_and_finite_phasors() {
    let p = Project::new(single()).unwrap();
    let r = unbalanced::study(&p, &Options::default(), &AtomicBool::new(false)).unwrap();
    assert_eq!(r.network_version, p.head());
    assert_eq!(r.project_id, p.id);
    assert_eq!(r.solver_version, unbalanced::SOLVER_VERSION);
    let json = serde_json::to_string(&r).unwrap();
    let decoded: unbalanced::StudyResult = serde_json::from_str(&json).unwrap();
    near(decoded.result.source_p_w, r.result.source_p_w, 1e-9);
    assert!(
        r.result
            .buses
            .iter()
            .flat_map(|b| b.conductor_voltages_v)
            .all(|v| v.re.is_finite() && v.im.is_finite())
    );
}

#[test]
fn downstream_branch_currents_sum_at_the_intermediate_node_with_neutral_kcl() {
    let mut n = single();
    let original_line = n
        .objects
        .values()
        .find(|o| matches!(o.equipment, Equipment::Line { .. }))
        .unwrap()
        .clone();
    let Equipment::Line {
        from: root,
        to: middle,
        ..
    } = original_line.equipment
    else {
        unreachable!()
    };
    let mut bus = n.objects[&middle].clone();
    bus.id = uuid::Uuid::new_v4();
    bus.name = "Aval".into();
    let end = bus.id;
    n.objects.insert(end, bus);
    let mut tail = original_line.clone();
    tail.id = uuid::Uuid::new_v4();
    tail.name = "Branche aval".into();
    if let Equipment::Line { from, to, .. } = &mut tail.equipment {
        *from = middle;
        *to = end;
    }
    let tail_id = tail.id;
    n.objects.insert(tail_id, tail);
    let mut load = n
        .objects
        .values()
        .find(|o| matches!(o.equipment, Equipment::Load { .. }))
        .unwrap()
        .clone();
    load.id = uuid::Uuid::new_v4();
    load.name = "Charge aval phase B".into();
    load.equipment = Equipment::Load {
        bus: end,
        p_w: 6000.0,
        q_var: 0.0,
        phase: Phase::B,
    };
    let Some(ElectricalData::Injection(d)) = &mut load.electrical else {
        unreachable!()
    };
    d.connection = InjectionConnection::PhaseNeutral {
        phase: Conductor::B,
    };
    n.objects.insert(load.id, load);
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    let trunk = r
        .branches
        .iter()
        .find(|b| b.id == original_line.id)
        .unwrap();
    let tail = r.branches.iter().find(|b| b.id == tail_id).unwrap();
    let v = &r
        .buses
        .iter()
        .find(|b| b.id == middle)
        .unwrap()
        .conductor_voltages_v;
    let re = v[0].re - v[3].re;
    let im = v[0].im - v[3].im;
    let norm = re * re + im * im;
    let ir = (10000.0 * re + 2000.0 * im) / norm;
    let ii = (10000.0 * im - 2000.0 * re) / norm;
    for k in 0..4 {
        let sign = if k == 0 {
            1.0
        } else if k == 3 {
            -1.0
        } else {
            0.0
        };
        near(
            trunk.currents_a[k].re - tail.currents_a[k].re,
            sign * ir,
            1e-7,
        );
        near(
            trunk.currents_a[k].im - tail.currents_a[k].im,
            sign * ii,
            1e-7,
        );
    }
    assert_eq!(trunk.from_bus, root);
    near(r.net_demand_p_w, 16000.0, 1e-9);
    assert!(r.balance_error_va < 1e-3);
}
#[test]
fn topology_loops_islands_and_multiple_sources_are_rejected() {
    for variant in 0..3 {
        let mut n = single();
        match variant {
            0 => {
                let mut line = n
                    .objects
                    .values()
                    .find(|o| matches!(o.equipment, Equipment::Line { .. }))
                    .unwrap()
                    .clone();
                line.id = uuid::Uuid::new_v4();
                n.objects.insert(line.id, line);
            }
            1 => {
                let mut bus = n
                    .objects
                    .values()
                    .find(|o| o.name == "Client BT")
                    .unwrap()
                    .clone();
                bus.id = uuid::Uuid::new_v4();
                n.objects.insert(bus.id, bus);
            }
            _ => {
                let mut source = n
                    .objects
                    .values()
                    .find(|o| matches!(o.equipment, Equipment::Source { .. }))
                    .unwrap()
                    .clone();
                source.id = uuid::Uuid::new_v4();
                n.objects.insert(source.id, source);
            }
        }
        assert!(matches!(
            unbalanced::solve(&n, &Options::default()),
            Err(CalculationError::Unsupported(_))
        ));
    }
}
#[test]
fn unbalanced_demo_has_phase_differences_return_current_and_conserved_power() {
    let n = reference_cases::unbalanced_bt();
    let r = unbalanced::solve(&n, &Options::default()).unwrap();
    near(r.net_demand_p_w, 15000.0, 1e-8);
    near(r.net_demand_q_var, 3400.0, 1e-8);
    let bus = r
        .buses
        .iter()
        .find(|b| n.objects[&b.id].name == "Client BT")
        .unwrap();
    assert!((bus.phase_neutral[0].magnitude_v - bus.phase_neutral[2].magnitude_v).abs() > 1.0);
    assert!(bus.neutral_displacement_v > 1.0);
    assert!(r.branches[0].currents_a[3].magnitude() > 10.0);
    assert!(r.balance_error_va < 1e-3);
}
