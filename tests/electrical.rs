use calcelec::{
    editor::EditHistory,
    electrical::*,
    load_flow::{self, CalculationError, Options},
    model::{Equipment, Phase, rural_example},
    project::{Operation, Project, SCHEMA_VERSION},
    reference_cases,
    storage::Store,
};
fn impedance(r: f64, x: f64) -> Impedance {
    Impedance { r, x }
}
fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{actual} != {expected}"
    );
}
fn analytical_voltage(p: f64, q: f64, r: f64, x: f64) -> (f64, f64, f64) {
    let vs = 400.0 / 3.0_f64.sqrt();
    let p = p / 3.0;
    let q = q / 3.0;
    let b = 2.0 * (r * p + x * q) - vs * vs;
    let c = (r * r + x * x) * (p * p + q * q);
    let squared = (-b + (b * b - 4.0 * c).sqrt()) / 2.0;
    let real = (squared + r * p + x * q) / vs;
    let imag = -(x * p - r * q) / vs;
    (
        squared.sqrt() * 3.0_f64.sqrt(),
        imag.atan2(real).to_degrees(),
        (p * p + q * q).sqrt() / squared.sqrt(),
    )
}
#[test]
fn four_wire_reference_reduces_exactly_and_matches_closed_form() {
    let n = reference_cases::four_wire();
    n.validate().unwrap();
    assert!(missing_data(&n).is_empty());
    let line = n
        .objects
        .values()
        .find(|o| matches!(o.equipment, Equipment::Line { .. }))
        .unwrap();
    let Some(ElectricalData::Line(data)) = &line.electrical else {
        unreachable!()
    };
    assert_eq!(
        data.conductors,
        vec![Conductor::A, Conductor::B, Conductor::C, Conductor::N]
    );
    let z = data.balanced_impedance().unwrap();
    near(z.r, 0.38, 1e-12);
    near(z.x, 0.06, 1e-12);
    let r = load_flow::solve(&n, &Options::default()).unwrap();
    let (v, angle, i) = analytical_voltage(30000.0, 0.0, 0.038, 0.006);
    let bus = r
        .buses
        .iter()
        .find(|b| n.objects[&b.id].name == "Client BT")
        .unwrap();
    near(bus.voltage_ll_v, v, 1e-6);
    near(bus.angle_deg, angle, 1e-7);
    near(r.branches[0].current_to_a, i, 1e-6);
    near(r.total_loss_w, 3.0 * 0.038 * i * i, 1e-5);
}
#[test]
fn transformer_reference_uses_copper_losses_and_matches_closed_form() {
    let n = reference_cases::transformer();
    n.validate().unwrap();
    assert!(missing_data(&n).is_empty());
    let transformer = n
        .objects
        .values()
        .find(|o| matches!(o.equipment, Equipment::Transformer { .. }))
        .unwrap();
    let Some(ElectricalData::Transformer(data)) = &transformer.electrical else {
        unreachable!()
    };
    let z = data.series_pu(100000.0, 4.0).unwrap();
    near(z.r, 0.01, 1e-12);
    near(z.x, (0.04_f64.powi(2) - 0.01_f64.powi(2)).sqrt(), 1e-12);
    near(data.ratio(), 50.0, 1e-12);
    let (v, angle, i) = analytical_voltage(30000.0, 9000.0, z.r * 1.6, z.x * 1.6);
    let r = load_flow::solve(&n, &Options::default()).unwrap();
    let bus = r
        .buses
        .iter()
        .find(|b| n.objects[&b.id].name == "Secondaire 400 V")
        .unwrap();
    near(bus.voltage_ll_v, v, 1e-6);
    near(bus.angle_deg, angle, 1e-7);
    near(r.total_loss_w, 3.0 * z.r * 1.6 * i * i, 1e-5);
    assert!(r.total_loss_w > 0.0);
    near(
        r.branches[0].current_to_a / r.branches[0].current_from_a,
        50.0,
        1e-10,
    );
}
#[test]
fn matrix_validation_rejects_dimensions_nonreciprocity_and_active_resistance() {
    let base = reference_cases::four_wire();
    for failure in 0..5 {
        let mut n = base.clone();
        let line = n
            .objects
            .values_mut()
            .find(|o| matches!(o.equipment, Equipment::Line { .. }))
            .unwrap();
        let Some(ElectricalData::Line(data)) = &mut line.electrical else {
            unreachable!()
        };
        match failure {
            0 => {
                data.series_ohm_per_km.pop();
            }
            1 => data.series_ohm_per_km[0][1].x = 1.0,
            2 => {
                data.series_ohm_per_km[0][1].r = 10.0;
                data.series_ohm_per_km[1][0].r = 10.0;
            }
            3 => data.series_ohm_per_km[0][0].r = f64::NAN,
            _ => data.conductors[0] = Conductor::N,
        }
        assert!(
            n.validate().is_err(),
            "invalid matrix case {failure} accepted"
        );
    }
}
#[test]
fn balanced_solver_refuses_asymmetric_lines_and_nonzero_shunts_even_when_simplified() {
    for shunt in [false, true] {
        let mut n = reference_cases::four_wire();
        let line = n
            .objects
            .values_mut()
            .find(|o| matches!(o.equipment, Equipment::Line { .. }))
            .unwrap();
        let Some(ElectricalData::Line(data)) = &mut line.electrical else {
            unreachable!()
        };
        if shunt {
            data.shunt_siemens_per_km = Some(vec![vec![Admittance { g: 0.0, b: 0.01 }; 4]; 4]);
        } else {
            data.series_ohm_per_km[0][0].r = 0.5;
        }
        n.validate().unwrap();
        assert!(matches!(
            load_flow::solve(&n, &Options::simplified()),
            Err(CalculationError::Unsupported(_))
        ));
    }
}
#[test]
fn conductor_order_is_explicit_and_can_be_permuted_without_changing_physics() {
    let mut n = reference_cases::four_wire();
    let before = load_flow::solve(&n, &Options::default()).unwrap();
    let line = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Line { .. }))
        .unwrap();
    let Some(ElectricalData::Line(data)) = &mut line.electrical else {
        unreachable!()
    };
    let order = [3, 2, 0, 1];
    let old = data.clone();
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
    let after = load_flow::solve(&n, &Options::default()).unwrap();
    near(after.source_p_w, before.source_p_w, 1e-9);
}
#[test]
fn missing_neutral_and_contradictory_injection_connections_are_rejected() {
    let mut n = reference_cases::four_wire();
    let load = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Load { .. }))
        .unwrap();
    let Equipment::Load { bus, .. } = load.equipment else {
        unreachable!()
    };
    load.electrical = Some(ElectricalData::Injection(InjectionData {
        connection: InjectionConnection::PhaseNeutral {
            phase: Conductor::A,
        },
        fault_contribution: FaultContribution::None,
    }));
    assert!(n.validate().is_err());
    let load = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Load { .. }))
        .unwrap();
    if let Equipment::Load { phase, .. } = &mut load.equipment {
        *phase = Phase::A;
    }
    n.validate().unwrap();
    let Some(ElectricalData::Bus(data)) = &mut n.objects.get_mut(&bus).unwrap().electrical else {
        unreachable!()
    };
    data.conductors.pop();
    data.neutral_grounding = Grounding::NotApplicable;
    assert!(n.validate().is_err());
}
#[test]
fn delta_cannot_expose_or_ground_a_star_neutral_and_clock_parity_is_checked() {
    let base = reference_cases::transformer();
    for variant in 0..3 {
        let mut n = base.clone();
        let t = n
            .objects
            .values_mut()
            .find(|o| matches!(o.equipment, Equipment::Transformer { .. }))
            .unwrap();
        let Some(ElectricalData::Transformer(d)) = &mut t.electrical else {
            unreachable!()
        };
        match variant {
            0 => {
                d.to.connection = WindingConnection::Delta;
            }
            1 => {
                d.clock = 11;
            }
            _ => {
                d.clock = 12;
            }
        }
        assert!(n.validate().is_err());
    }
}
#[test]
fn dyn11_and_off_nominal_taps_are_preserved_but_not_silently_ignored_by_solver() {
    let mut n = reference_cases::transformer();
    let t = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Transformer { .. }))
        .unwrap();
    let Some(ElectricalData::Transformer(d)) = &mut t.electrical else {
        unreachable!()
    };
    d.from.connection = WindingConnection::Delta;
    d.from.grounding = Grounding::NotApplicable;
    d.clock = 11;
    near(d.phase_shift_deg(), 30.0, 1e-12);
    d.tap.as_mut().unwrap().position = 1;
    near(d.ratio(), 51.25, 1e-12);
    n.validate().unwrap();
    assert!(matches!(
        load_flow::solve(&n, &Options::simplified()),
        Err(CalculationError::Unsupported(_))
    ));
    let p = Project::new(n).unwrap();
    assert_eq!(Project::from_json(&p.to_json().unwrap()).unwrap(), p);
}
#[test]
fn impossible_test_losses_and_invalid_tap_positions_are_rejected() {
    let base = reference_cases::transformer();
    for variant in 0..4 {
        let mut n = base.clone();
        let t = n
            .objects
            .values_mut()
            .find(|o| matches!(o.equipment, Equipment::Transformer { .. }))
            .unwrap();
        let Some(ElectricalData::Transformer(d)) = &mut t.electrical else {
            unreachable!()
        };
        match variant {
            0 => d.copper_loss_w = 5000.0,
            1 => {
                d.iron_loss_w = Some(1000.0);
                d.no_load_current_percent = Some(0.1);
            }
            2 => d.tap.as_mut().unwrap().position = 3,
            _ => {
                d.tap.as_mut().unwrap().step_percent = 100.0;
            }
        }
        assert!(n.validate().is_err());
    }
}
#[test]
fn detailed_edits_share_undo_and_sqlite_roundtrip_atomically() {
    let mut p = Project::new(reference_cases::four_wire()).unwrap();
    let base = p.clone();
    let mut history = EditHistory::default();
    let source = p
        .root
        .objects
        .values()
        .find(|o| matches!(o.equipment, Equipment::Source { .. }))
        .unwrap();
    let id = source.id;
    let mut detailed = source.electrical.clone();
    let Some(ElectricalData::Source(d)) = &mut detailed else {
        unreachable!()
    };
    d.positive_sequence_ohm = Some(impedance(0.2, 0.3));
    history
        .commit(
            &mut p,
            "Test",
            "Impédance amont",
            vec![Operation::SetElectrical {
                id,
                electrical: detailed,
            }],
        )
        .unwrap();
    let mut remote = base.clone();
    remote.apply(p.changes[0].clone()).unwrap();
    assert_eq!(remote, p);
    history.undo(&mut p, "Test").unwrap();
    assert_eq!(p.network().unwrap(), base.network().unwrap());
    history.redo(&mut p, "Test").unwrap();
    let mut db = Store::open(":memory:").unwrap();
    db.save(&p).unwrap();
    assert_eq!(db.load(p.id).unwrap(), p);
    let before = p.clone();
    let invalid = p.prepare(
        "Test",
        "Mauvais type",
        vec![
            Operation::Rename {
                id,
                name: "À annuler".into(),
            },
            Operation::SetElectrical {
                id,
                electrical: Some(ElectricalData::Switch {
                    conductors: vec![Conductor::A],
                }),
            },
        ],
    );
    assert!(p.apply(invalid).is_err());
    assert_eq!(p, before);
}
#[test]
fn legacy_schema_three_migrates_without_inventing_data_and_new_operation_requires_four() {
    let p = Project::new(rural_example()).unwrap();
    let mut old = serde_json::to_value(&p).unwrap();
    old["schema_version"] = 3.into();
    let mut migrated = Project::from_json(&old.to_string()).unwrap();
    assert_eq!(migrated, p);
    assert_eq!(migrated.schema_version, SCHEMA_VERSION);
    assert!(
        migrated
            .root
            .objects
            .values()
            .all(|o| o.electrical.is_none())
    );
    assert!(!missing_data(&migrated.root).is_empty());
    let id = *p.root.objects.keys().next().unwrap();
    let mut delta = migrated.prepare(
        "Test",
        "Nouveau modèle",
        vec![Operation::SetElectrical {
            id,
            electrical: None,
        }],
    );
    delta.schema_version = 3;
    assert!(migrated.apply(delta).is_err());
    let mut bad =
        serde_json::to_value(Project::new(reference_cases::four_wire()).unwrap()).unwrap();
    bad["schema_version"] = 3.into();
    assert!(Project::from_json(&bad.to_string()).is_err());
}

#[test]
fn old_schema_three_sqlite_history_migrates_without_changing_network_versions() {
    let mut original = Project::new(rural_example()).unwrap();
    let id = *original.root.objects.keys().next().unwrap();
    let change = original.prepare(
        "Ancien poste",
        "Nom",
        vec![Operation::Rename {
            id,
            name: "Nom existant".into(),
        }],
    );
    original.apply(change).unwrap();
    let mut old = serde_json::to_value(&original).unwrap();
    old["schema_version"] = 3.into();
    old["changes"][0]["schema_version"] = 3.into();
    let path = std::env::temp_dir().join(format!(
        "calcelec-electrical-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    {
        let mut db = Store::open(&path).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO projects VALUES (?1,3,?2,?3,?4)",
            rusqlite::params![
                original.id.to_string(),
                original.root_version.to_string(),
                old["root"].to_string(),
                original.head().to_string()
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO changes VALUES (?1,0,?2,?3)",
            rusqlite::params![
                original.id.to_string(),
                original.changes[0].id.to_string(),
                old["changes"][0].to_string()
            ],
        )
        .unwrap();
        let migrated = db.load(original.id).unwrap();
        assert_eq!(migrated, original);
        db.save(&migrated).unwrap();
        assert_eq!(db.load(original.id).unwrap(), original);
        let schema: u32 = conn
            .query_row("SELECT schema_version FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(schema, SCHEMA_VERSION);
    }
    std::fs::remove_file(path).unwrap();
}
