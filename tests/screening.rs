use calcelec::{
    load_flow,
    model::rural_example,
    project::Project,
    reference_cases,
    scenario::{InjectionOverride, Scenario, ScenarioKind},
    screening::{self, Limits, MetricKind, ScreeningStatus},
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
