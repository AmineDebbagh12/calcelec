use calcelec::{
    geometry::CoordinateReference,
    load_flow::{self, CalculationError, Options},
    model::{Equipment, Network, Object, Phase, rural_example},
    project::{Operation, Project},
};
use std::{collections::BTreeMap, sync::atomic::AtomicBool};
use uuid::Uuid;

fn feeder(p: f64, q: f64, r: f64, x: f64) -> (Network, Uuid, Uuid, Uuid) {
    let source_bus = Object::new("Source", Equipment::Bus { voltage_v: 400.0 });
    let root = source_bus.id;
    let load_bus = Object::new("Client", Equipment::Bus { voltage_v: 400.0 });
    let end = load_bus.id;
    let source = Object::new(
        "Amont",
        Equipment::Source {
            bus: root,
            voltage_pu: 1.0,
        },
    );
    let line = Object::new(
        "Ligne",
        Equipment::Line {
            from: root,
            to: end,
            length_m: 1000.0,
            r_ohm_per_km: r,
            x_ohm_per_km: x,
            max_current_a: 100.0,
        },
    );
    let branch = line.id;
    let load = Object::new(
        "Charge",
        Equipment::Load {
            bus: end,
            p_w: p,
            q_var: q,
            phase: Phase::Abc,
        },
    );
    (
        Network {
            name: "Référence analytique".into(),
            frequency_hz: 50.0,
            coordinate_reference: CoordinateReference::LocalMeters,
            objects: [source_bus, load_bus, source, line, load]
                .into_iter()
                .map(|o| (o.id, o))
                .collect::<BTreeMap<_, _>>(),
        },
        root,
        end,
        branch,
    )
}
fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} != {expected} (ε={tolerance})"
    );
}

#[test]
fn resistive_two_bus_matches_closed_form_high_voltage_solution() {
    let (n, _, end, line) = feeder(30000.0, 0.0, 0.1, 0.0);
    let result = load_flow::solve(&n, &Options::default()).unwrap();
    let vs = 400.0 / 3.0_f64.sqrt();
    let vr = (vs + (vs * vs - 4.0 * 0.1 * 10000.0).sqrt()) / 2.0;
    let expected_i = 10000.0 / vr;
    near(
        result
            .buses
            .iter()
            .find(|b| b.id == end)
            .unwrap()
            .voltage_ll_v,
        vr * 3.0_f64.sqrt(),
        1e-6,
    );
    let branch = result.branches.iter().find(|b| b.id == line).unwrap();
    near(branch.current_from_a, expected_i, 1e-6);
    near(result.total_loss_w, 3.0 * 0.1 * expected_i.powi(2), 1e-5);
    near(result.source_p_w, 30000.0 + result.total_loss_w, 1e-4);
    assert!(result.residual_pu <= 1e-9);
}

#[test]
fn reactive_two_bus_matches_independent_complex_closed_form() {
    let (n, _, end, _) = feeder(30000.0, 9000.0, 0.1, 0.05);
    let result = load_flow::solve(&n, &Options::default()).unwrap();
    let (p, q, r, x) = (10000.0_f64, 3000.0_f64, 0.1_f64, 0.05_f64);
    let vs = 400.0 / 3.0_f64.sqrt();
    let b = 2.0 * (r * p + x * q) - vs * vs;
    let c = (r * r + x * x) * (p * p + q * q);
    let v_squared = (-b + (b * b - 4.0 * c).sqrt()) / 2.0;
    let real = (v_squared + r * p + x * q) / vs;
    let imag = -(x * p - r * q) / vs;
    let bus = result.buses.iter().find(|b| b.id == end).unwrap();
    near(bus.voltage_ll_v, v_squared.sqrt() * 3.0_f64.sqrt(), 1e-6);
    near(bus.angle_deg, imag.atan2(real).to_degrees(), 1e-7);
    assert!(result.balance_error_va < 1e-3);
}

#[test]
fn no_load_flat_voltage_and_zero_losses() {
    let (n, _, _, _) = feeder(0.0, 0.0, 0.1, 0.1);
    let result = load_flow::solve(&n, &Options::default()).unwrap();
    assert_eq!(result.iterations, 1);
    for b in result.buses {
        near(b.voltage_pu, 1.0, 1e-12);
    }
    near(result.total_loss_w, 0.0, 1e-12);
    near(result.source_p_w, 0.0, 1e-12);
}

#[test]
fn generation_exports_power_and_raises_receiving_voltage() {
    let (mut n, _, end, _) = feeder(0.0, 0.0, 0.1, 0.0);
    let pv = Object::new(
        "PV",
        Equipment::Generator {
            bus: end,
            p_w: 30000.0,
            q_var: 0.0,
            phase: Phase::Abc,
        },
    );
    n.objects.insert(pv.id, pv);
    let result = load_flow::solve(&n, &Options::default()).unwrap();
    assert!(result.source_p_w < 0.0);
    assert!(
        result
            .buses
            .iter()
            .find(|b| b.id == end)
            .unwrap()
            .voltage_pu
            > 1.0
    );
    near(result.source_p_w, -30000.0 + result.total_loss_w, 1e-4);
}

#[test]
fn reversing_stored_line_orientation_does_not_change_solution() {
    let (mut n, _, _, line) = feeder(30000.0, 9000.0, 0.1, 0.05);
    let before = load_flow::solve(&n, &Options::default()).unwrap();
    if let Equipment::Line { from, to, .. } = &mut n.objects.get_mut(&line).unwrap().equipment {
        std::mem::swap(from, to);
    }
    let after = load_flow::solve(&n, &Options::default()).unwrap();
    near(after.source_p_w, before.source_p_w, 1e-9);
    near(
        after.branches[0].current_from_a,
        before.branches[0].current_from_a,
        1e-9,
    );
    assert_eq!(after.branches[0].from_bus, before.branches[0].from_bus);
}

#[test]
fn rural_example_requires_explicit_assumptions_then_balances_power() {
    let n = rural_example();
    assert!(matches!(
        load_flow::solve(&n, &Options::default()),
        Err(CalculationError::Unsupported(_))
    ));
    let result = load_flow::solve(&n, &Options::simplified()).unwrap();
    assert_eq!(result.buses.len(), 8);
    assert_eq!(result.branches.len(), 7);
    near(result.net_demand_p_w, 192000.0, 1e-6);
    near(result.net_demand_q_var, 46000.0, 1e-6);
    assert!(result.total_loss_w > 0.0);
    assert!(result.balance_error_va < 0.01);
    assert_eq!(result.assumptions.len(), 3);
    for branch in result.branches {
        near(branch.p_from_w - branch.p_to_w, branch.loss_w, 1e-3);
        near(branch.q_from_var - branch.q_to_var, branch.loss_var, 1e-3);
    }
}

#[test]
fn simplified_transformer_respects_ratio_current_and_reactive_drop() {
    let (mut n, root, end, line) = feeder(30000.0, 9000.0, 0.1, 0.0);
    n.objects.get_mut(&root).unwrap().equipment = Equipment::Bus { voltage_v: 20000.0 };
    n.objects.get_mut(&line).unwrap().equipment = Equipment::Transformer {
        from: root,
        to: end,
        rated_va: 100000.0,
        uk_percent: 4.0,
    };
    let result = load_flow::solve(&n, &Options::simplified()).unwrap();
    let branch = &result.branches[0];
    near(branch.current_to_a / branch.current_from_a, 50.0, 1e-10);
    near(branch.loss_w, 0.0, 1e-12);
    assert!(branch.loss_var > 0.0);
    assert!(
        result
            .buses
            .iter()
            .find(|b| b.id == end)
            .unwrap()
            .voltage_pu
            < 1.0
    );
}

#[test]
fn loops_parallel_lines_disconnected_buses_and_multiple_sources_are_refused() {
    let (base, root, end, _) = feeder(30000.0, 0.0, 0.1, 0.0);
    let mut parallel = base.clone();
    let switch = Object::new(
        "Boucle",
        Equipment::Switch {
            from: root,
            to: end,
            closed: true,
        },
    );
    parallel.objects.insert(switch.id, switch);
    assert!(matches!(
        load_flow::solve(&parallel, &Options::default()),
        Err(CalculationError::Unsupported(_))
    ));
    let mut disconnected = base.clone();
    for o in disconnected.objects.values_mut() {
        if matches!(o.equipment, Equipment::Line { .. }) {
            o.in_service = false;
        }
    }
    assert!(matches!(
        load_flow::solve(&disconnected, &Options::default()),
        Err(CalculationError::Unsupported(_))
    ));
    let mut multiple = base;
    let source = Object::new(
        "Deuxième source",
        Equipment::Source {
            bus: end,
            voltage_pu: 1.0,
        },
    );
    multiple.objects.insert(source.id, source);
    assert!(matches!(
        load_flow::solve(&multiple, &Options::default()),
        Err(CalculationError::Unsupported(_))
    ));
}

#[test]
fn open_switch_is_excluded_and_closed_switch_has_no_voltage_drop() {
    let (mut n, root, end, line) = feeder(30000.0, 0.0, 0.1, 0.0);
    n.objects.get_mut(&line).unwrap().equipment = Equipment::Switch {
        from: root,
        to: end,
        closed: true,
    };
    let result = load_flow::solve(&n, &Options::default()).unwrap();
    assert!(result.buses.iter().all(|b| b.voltage_pu == 1.0));
    assert_eq!(result.total_loss_w, 0.0);
    n.objects.get_mut(&line).unwrap().equipment = Equipment::Switch {
        from: root,
        to: end,
        closed: false,
    };
    assert!(load_flow::solve(&n, &Options::default()).is_err());
}

#[test]
fn failed_or_cancelled_calculation_returns_no_results() {
    let (n, _, _, _) = feeder(30000.0, 9000.0, 0.1, 0.05);
    let options = Options {
        max_iterations: 1,
        tolerance_pu: 1e-14,
        ..Options::default()
    };
    assert!(matches!(
        load_flow::solve(&n, &options),
        Err(CalculationError::NonConvergent { .. })
    ));
    assert!(matches!(
        load_flow::solve_cancellable(&n, &Options::default(), &AtomicBool::new(true)),
        Err(CalculationError::Cancelled)
    ));
    assert!(matches!(
        load_flow::solve(
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
fn voltage_and_loading_alerts_follow_configured_limits() {
    let (mut n, _, end, line) = feeder(30000.0, 0.0, 0.1, 0.0);
    if let Equipment::Line { max_current_a, .. } = &mut n.objects.get_mut(&line).unwrap().equipment
    {
        *max_current_a = 10.0;
    }
    let options = Options {
        min_voltage_pu: 0.99,
        ..Options::default()
    };
    let result = load_flow::solve(&n, &options).unwrap();
    assert!(
        result
            .buses
            .iter()
            .find(|b| b.id == end)
            .unwrap()
            .outside_voltage_limits
    );
    assert!(result.branches[0].loading_percent.unwrap() > 100.0);
}

#[test]
fn study_is_bound_to_exact_project_version_and_serializes_for_export() {
    let (n, _, _, _) = feeder(30000.0, 0.0, 0.1, 0.0);
    let mut p = Project::new(n).unwrap();
    let report = load_flow::study(&p, &Options::default(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.project_id, p.id);
    assert_eq!(report.network_version, p.head());
    assert_eq!(report.solver_version, load_flow::SOLVER_VERSION);
    let _: load_flow::StudyResult =
        serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
    let id = *p.root.objects.keys().next().unwrap();
    let change = p.prepare(
        "Test",
        "Renommage",
        vec![Operation::Rename {
            id,
            name: "Nouveau nom".into(),
        }],
    );
    p.apply(change).unwrap();
    assert_ne!(report.network_version, p.head());
}

#[test]
fn non_finite_derived_outputs_are_rejected_instead_of_exporting_null() {
    let (mut n, root, _, line) = feeder(0.0, 0.0, 0.1, 0.0);
    let source = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Source { .. }))
        .unwrap();
    source.equipment = Equipment::Source {
        bus: root,
        voltage_pu: 2.0,
    };
    for o in n.objects.values_mut() {
        if matches!(o.equipment, Equipment::Bus { .. }) {
            o.equipment = Equipment::Bus { voltage_v: 1e308 };
        }
    }
    n.objects.get_mut(&line).unwrap().equipment = Equipment::Switch {
        from: root,
        to: n.objects[&line].buses()[1],
        closed: true,
    };
    assert!(matches!(
        load_flow::solve(&n, &Options::default()),
        Err(CalculationError::Numerical(_))
    ));
}
