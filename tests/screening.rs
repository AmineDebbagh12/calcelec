use calcelec::{
    load_flow,
    model::rural_example,
    project::Project,
    reference_cases,
    scenario::{EquipmentOverride, InjectionOverride, Scenario, ScenarioKind},
    screening::{self, Limits, MetricKind, ScreeningStatus},
    short_circuit::{
        Options as FaultOptions, ProtectionRequirement, ResidualCriterion, ResidualRequirement,
    },
    unbalanced,
};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};
use uuid::Uuid;

fn scenario(name: &str, load_scale: f64, generation_scale: f64) -> Scenario {
    Scenario {
        id: Uuid::new_v4(),
        name: name.into(),
        kind: ScenarioKind::Custom,
        load_scale,
        generation_scale,
        injections: BTreeMap::new(),
        equipment: BTreeMap::new(),
    }
}

#[test]
fn screens_every_case_and_keeps_the_dimensioning_scenario() {
    let project = Project::new(reference_cases::rural_unbalanced()).unwrap();
    let scenarios = vec![
        scenario("Consommation maximale", 1.25, 0.0),
        scenario("Production maximale", 0.3, 1.2),
    ];
    let limits = Limits {
        max_loading_percent: 0.01,
        ..Limits::default()
    };
    let report = screening::screen_unbalanced(
        &project,
        &scenarios,
        true,
        &unbalanced::Options::default(),
        &limits,
        &AtomicBool::new(false),
    )
    .unwrap();

    assert_eq!(report.status, ScreeningStatus::Constrained);
    assert_eq!(report.scenarios.len(), 3);
    assert!(report.scenarios.iter().all(|case| case.error.is_none()));
    assert!(report.scenarios.iter().any(|case| {
        case.constraints
            .iter()
            .any(|constraint| constraint.metric == MetricKind::MaximumLoading)
    }));
    let loading_cases: Vec<_> = report
        .dimensioning_cases
        .iter()
        .filter(|case| case.metric == MetricKind::MaximumLoading)
        .collect();
    assert!(!loading_cases.is_empty());
    assert!(loading_cases.iter().all(|case| {
        case.scenario_name == "Réseau de base"
            || scenarios
                .iter()
                .any(|scenario| scenario.id == case.scenario_id.unwrap_or_default())
    }));
    assert_eq!(
        serde_json::from_str::<screening::ScreeningReport>(
            &serde_json::to_string(&report).unwrap()
        )
        .unwrap()
        .network_version,
        project.head()
    );
}

#[test]
fn permissive_limits_mark_all_solved_scenarios_compliant() {
    let project = Project::new(reference_cases::rural_unbalanced()).unwrap();
    let report = screening::screen_unbalanced(
        &project,
        &[scenario("Situation", 1.0, 1.0)],
        false,
        &unbalanced::Options::default(),
        &Limits {
            min_voltage_pu: 0.001,
            max_voltage_pu: 100.0,
            max_loading_percent: 1.0e9,
            max_negative_sequence_percent: None,
            max_neutral_displacement_v: None,
        },
        &AtomicBool::new(false),
    )
    .unwrap();

    assert_eq!(report.status, ScreeningStatus::Compliant);
    assert_eq!(report.scenarios[0].status, ScreeningStatus::Compliant);
    assert!(report.scenarios[0].constraints.is_empty());
}

#[test]
fn one_invalid_scenario_makes_the_report_unable_without_hiding_other_cases() {
    let project = Project::new(reference_cases::rural_unbalanced()).unwrap();
    let missing = Uuid::new_v4();
    let mut invalid = scenario("Référence absente", 1.0, 1.0);
    invalid.injections.insert(
        missing,
        InjectionOverride {
            in_service: None,
            p_w: Some(1.0),
            q_var: None,
        },
    );
    let report = screening::screen_unbalanced(
        &project,
        &[scenario("Valide", 1.0, 1.0), invalid],
        false,
        &unbalanced::Options::default(),
        &Limits::default(),
        &AtomicBool::new(false),
    )
    .unwrap();

    assert_eq!(report.status, ScreeningStatus::Unable);
    assert_eq!(report.scenarios.len(), 2);
    assert_ne!(report.scenarios[0].status, ScreeningStatus::Unable);
    assert_eq!(report.scenarios[1].status, ScreeningStatus::Unable);
    assert!(
        report.scenarios[1]
            .error
            .as_deref()
            .unwrap()
            .contains(&missing.to_string())
    );
}

#[test]
fn balanced_screening_uses_explicit_simplified_options_and_honours_cancellation() {
    let project = Project::new(rural_example()).unwrap();
    let report = screening::screen_balanced(
        &project,
        &[scenario("Pointe", 1.1, 0.0)],
        true,
        &load_flow::Options::simplified(),
        &Limits::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(report.scenarios.len(), 2);
    assert!(
        report
            .scenarios
            .iter()
            .all(|case| case.status != ScreeningStatus::Unable)
    );

    let cancelled = AtomicBool::new(true);
    assert!(
        screening::screen_balanced(
            &project,
            &[],
            true,
            &load_flow::Options::simplified(),
            &Limits::default(),
            &cancelled,
        )
        .unwrap_err()
        .to_string()
        .contains("annulé")
    );
    assert!(cancelled.load(Ordering::Relaxed));
}

#[test]
fn fault_requirements_join_the_global_verdict_without_fake_pq_provenance() {
    let project = Project::new(reference_cases::rural_unbalanced()).unwrap();
    let network = project.network().unwrap();
    let hta_bus = network
        .objects
        .values()
        .find(|object| object.name == "Poste rural 3 — HTA")
        .unwrap()
        .id;
    let report = screening::screen_unbalanced(
        &project,
        &[scenario("Production maximale", 0.3, 1.0)],
        false,
        &unbalanced::Options::default(),
        &Limits {
            min_voltage_pu: 0.001,
            max_voltage_pu: 100.0,
            max_loading_percent: 1.0e9,
            max_negative_sequence_percent: None,
            max_neutral_displacement_v: None,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    let report = screening::add_fault_study(
        report,
        &project,
        &FaultOptions {
            protection_requirements: vec![ProtectionRequirement {
                name: "Départ producteur".into(),
                bus_id: hta_bus,
                minimum_icc_b_a: Some(100_000.0),
                residual: Some(ResidualRequirement {
                    criterion: ResidualCriterion::MaximumAdmissible,
                    threshold_a: 1.0,
                }),
            }],
            ..FaultOptions::default()
        },
    );

    assert_eq!(report.status, ScreeningStatus::Constrained);
    assert_eq!(
        report.fault_study.as_ref().unwrap().status,
        ScreeningStatus::Constrained
    );
    assert!(report.dimensioning_cases.iter().any(|case| {
        case.metric == MetricKind::MinimumShortCircuit
            && case.scenario_name == "Réseau de base"
            && case.scenario_id.is_none()
            && case.constrained
    }));
}

#[test]
fn unavailable_three_i0_prevents_a_compliant_fault_verdict() {
    let project = Project::new(reference_cases::rural_unbalanced()).unwrap();
    let terminal = project
        .network()
        .unwrap()
        .objects
        .values()
        .find(|object| object.name == "Extrémité départ BT")
        .unwrap()
        .id;
    let report = screening::screen_unbalanced(
        &project,
        &[],
        true,
        &unbalanced::Options::default(),
        &Limits::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    let report = screening::add_fault_study(
        report,
        &project,
        &FaultOptions {
            protection_requirements: vec![ProtectionRequirement {
                name: "Protection BT".into(),
                bus_id: terminal,
                minimum_icc_b_a: None,
                residual: Some(ResidualRequirement {
                    criterion: ResidualCriterion::MaximumAdmissible,
                    threshold_a: 2_500.0,
                }),
            }],
            ..FaultOptions::default()
        },
    );

    assert_eq!(report.status, ScreeningStatus::Unable);
    assert_eq!(
        report.fault_study.as_ref().unwrap().status,
        ScreeningStatus::Unable
    );
}

#[test]
fn fault_screening_recomputes_operating_variants_but_not_pq_only_scenarios() {
    let project = Project::new(reference_cases::rural_unbalanced()).unwrap();
    let line = project
        .network()
        .unwrap()
        .objects
        .values()
        .find(|object| matches!(object.equipment, calcelec::model::Equipment::Line { .. }))
        .unwrap()
        .id;
    let pq_only = scenario("P/Q uniquement", 1.2, 0.0);
    let mut unavailable = scenario("Tronçon indisponible", 1.0, 1.0);
    unavailable.equipment.insert(
        line,
        EquipmentOverride {
            in_service: Some(false),
            switch_closed: None,
            tap_position: None,
        },
    );
    let scenarios = vec![pq_only, unavailable.clone()];
    let report = screening::screen_unbalanced(
        &project,
        &scenarios,
        true,
        &unbalanced::Options::default(),
        &Limits::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    let report =
        screening::add_fault_studies(report, &project, &scenarios, &FaultOptions::default());
    let fault = report.fault_study.unwrap();

    assert_eq!(fault.cases.len(), 2);
    assert_eq!(fault.cases[0].scenario_id, None);
    assert_eq!(fault.cases[1].scenario_id, Some(unavailable.id));
    assert_eq!(fault.cases[1].status, ScreeningStatus::Unable);
}
