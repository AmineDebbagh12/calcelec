use calcelec::{
    project::Project,
    reference_cases,
    reporting::{StoredResult, StudyKind, screening_csv, screening_markdown},
    screening::{self, Limits},
    storage::Store,
    unbalanced,
};
use std::sync::atomic::AtomicBool;

fn report() -> (Project, screening::ScreeningReport) {
    let project = Project::new(reference_cases::rural_unbalanced()).unwrap();
    let report = screening::screen_unbalanced(
        &project,
        &[],
        true,
        &unbalanced::Options::default(),
        &Limits::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    (project, report)
}

#[test]
fn screening_exports_are_traceable_and_name_objects() {
    let (project, report) = report();
    let network = project.network().unwrap();
    let csv = screening_csv(&report, &network);
    assert!(csv.starts_with("section;situation;verdict;object_id"));
    assert!(csv.contains("Réseau de base"));
    let markdown = screening_markdown(&report, &network);
    assert!(markdown.contains(&project.id.to_string()));
    assert!(markdown.contains(&project.head().to_string()));
    assert!(markdown.contains("## Situations dimensionnantes"));
}

#[test]
fn sqlite_persists_immutable_results_for_an_exact_network_version() {
    let (project, report) = report();
    let stored = StoredResult::new(
        "Vérification hiver",
        project.id,
        project.head(),
        StudyKind::ScenarioScreening,
        &report.solver_version,
        &report,
    )
    .unwrap();
    let path =
        std::env::temp_dir().join(format!("calcelec-results-{}.sqlite", uuid::Uuid::new_v4()));
    {
        let mut database = Store::open(&path).unwrap();
        database.save(&project).unwrap();
        assert!(database.save_result(&project, &stored).unwrap());
        assert!(!database.save_result(&project, &stored).unwrap());
        let loaded = database.load_results(&project).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, stored.id);
        assert_eq!(loaded[0].network_version, stored.network_version);
        assert_eq!(loaded[0].kind, StudyKind::ScenarioScreening);
        let decoded: screening::ScreeningReport = loaded[0].decode().unwrap();
        assert_eq!(decoded.project_id, project.id);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn result_from_an_unknown_version_is_rejected() {
    let (project, report) = report();
    let stored = StoredResult::new(
        "Version inconnue",
        project.id,
        uuid::Uuid::new_v4(),
        StudyKind::ScenarioScreening,
        &report.solver_version,
        &report,
    )
    .unwrap();
    assert!(stored.validate_for(&project).is_err());
}
