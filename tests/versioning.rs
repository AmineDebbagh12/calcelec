use calcelec::{
    model::{self, Equipment, Error, Object},
    project::{Operation, Project},
    storage::{self, Store},
};
use uuid::Uuid;

fn project() -> Project {
    Project::new(model::rural_example()).unwrap()
}
fn load_id(project: &Project) -> Uuid {
    project
        .root
        .objects
        .values()
        .find(|o| matches!(o.equipment, Equipment::Load { .. }))
        .unwrap()
        .id
}
fn power_change(project: &Project, p_w: f64) -> calcelec::project::ChangeSet {
    project.prepare(
        "Test",
        "Nouvelle consommation",
        vec![Operation::SetPower {
            id: load_id(project),
            p_w,
            q_var: 1200.0,
        }],
    )
}

#[test]
fn rural_network_is_valid_and_connected() {
    let p = project();
    let n = p.network().unwrap();
    assert_eq!(n.objects.len(), 22);
    assert!(n.unpowered_buses().is_empty());
    assert_eq!(
        n.objects
            .values()
            .filter(|o| matches!(o.equipment, Equipment::Transformer { .. }))
            .count(),
        3
    );
}

#[test]
fn compact_delta_reconstructs_identical_version_on_second_workstation() {
    let mut alice = project();
    let base = alice.to_json().unwrap();
    let bus = match alice.root.objects[&load_id(&alice)].equipment {
        Equipment::Load { bus, .. } => bus,
        _ => unreachable!(),
    };
    let generator = Object::new(
        "Nouveau PV",
        Equipment::Generator {
            bus,
            p_w: 3000.0,
            q_var: 0.0,
            phase: model::Phase::A,
        },
    );
    let change = alice.prepare(
        "Alice",
        "Raccordement",
        vec![
            Operation::SetPower {
                id: load_id(&alice),
                p_w: 8000.0,
                q_var: 2000.0,
            },
            Operation::Create { object: generator },
        ],
    );
    let delta = serde_json::to_string(&change).unwrap();
    alice.apply(change).unwrap();
    let mut bob = Project::from_json(&base).unwrap();
    assert!(bob.apply(serde_json::from_str(&delta).unwrap()).unwrap());
    assert_eq!(alice, bob);
    assert_eq!(alice.network().unwrap(), bob.network().unwrap());
    assert!(delta.len() < base.len());
}

#[test]
fn duplicate_is_idempotent_but_different_content_is_rejected() {
    let mut p = project();
    let change = power_change(&p, 9000.0);
    assert!(p.apply(change.clone()).unwrap());
    assert!(!p.apply(change.clone()).unwrap());
    let before = p.clone();
    let mut tampered = change;
    tampered.message = "Other content".into();
    assert!(matches!(p.apply(tampered), Err(Error::Conflict(_))));
    assert_eq!(p, before);
}

#[test]
fn concurrent_branches_conflict_without_losing_local_work() {
    let mut p = project();
    let first = power_change(&p, 8000.0);
    let concurrent = power_change(&p, 9000.0);
    p.apply(first).unwrap();
    let before = p.clone();
    assert!(matches!(p.apply(concurrent), Err(Error::Conflict(_))));
    assert_eq!(p, before);
}

#[test]
fn invalid_late_operation_rolls_back_entire_batch() {
    let mut p = project();
    let before = p.clone();
    let change = p.prepare(
        "Test",
        "Lot invalide",
        vec![
            Operation::Rename {
                id: load_id(&p),
                name: "Nouveau nom".into(),
            },
            Operation::Delete { id: Uuid::new_v4() },
        ],
    );
    assert!(p.apply(change).is_err());
    assert_eq!(p, before);
}

#[test]
fn cannot_delete_referenced_bus_but_can_delete_dependents_in_same_batch() {
    let mut p = project();
    let original = p.clone();
    let bus = p
        .root
        .objects
        .values()
        .find(|o| matches!(o.equipment, Equipment::Bus { .. }))
        .unwrap()
        .id;
    let invalid = p.prepare(
        "Test",
        "Suppression invalide",
        vec![Operation::Delete { id: bus }],
    );
    assert!(p.apply(invalid).is_err());
    assert_eq!(p, original);
    let operations = p
        .root
        .objects
        .values()
        .filter(|o| o.id == bus || o.buses().contains(&bus))
        .map(|o| Operation::Delete { id: o.id })
        .collect();
    let valid = p.prepare("Test", "Suppression explicite avec dépendances", operations);
    p.apply(valid).unwrap();
    assert!(!p.network().unwrap().objects.contains_key(&bus));
}

#[test]
fn non_finite_power_and_unsupported_schema_are_rejected() {
    let mut p = project();
    let change = power_change(&p, f64::NAN);
    assert!(p.apply(change).is_err());
    let mut encoded = serde_json::to_value(&p).unwrap();
    encoded["schema_version"] = 99.into();
    assert!(Project::from_json(&encoded.to_string()).is_err());
    encoded["schema_version"] = 1.into();
    encoded["unexpected"] = true.into();
    assert!(Project::from_json(&encoded.to_string()).is_err());
}

#[test]
fn serialized_invalid_history_is_rejected() {
    let mut p = project();
    let change = power_change(&p, 8000.0);
    p.apply(change).unwrap();
    p.changes[0].base_version = Uuid::new_v4();
    assert!(Project::from_json(&serde_json::to_string(&p).unwrap()).is_err());
}

#[test]
fn switching_off_transformer_disconnects_its_downstream_buses() {
    let mut p = project();
    let id = p
        .root
        .objects
        .values()
        .find(|o| o.name == "Transformateur 1")
        .unwrap()
        .id;
    let change = p.prepare(
        "Test",
        "Coupure",
        vec![Operation::SetService {
            id,
            in_service: false,
        }],
    );
    p.apply(change).unwrap();
    assert_eq!(p.network().unwrap().unpowered_buses().len(), 2);
}

#[test]
fn sqlite_preserves_history_and_rejects_stale_and_divergent_writers() {
    let path = std::env::temp_dir().join(format!("calcelec-{}.sqlite", Uuid::new_v4()));
    {
        let mut db = Store::open(&path).unwrap();
        let mut other = Store::open(&path).unwrap();
        let base = project();
        db.save(&base).unwrap();
        let mut alice = other.load(base.id).unwrap();
        let mut bob = db.load(base.id).unwrap();
        let a = power_change(&alice, 1000.0);
        let b = power_change(&bob, 2000.0);
        alice.apply(a).unwrap();
        bob.apply(b).unwrap();
        db.save(&alice).unwrap();
        db.save(&alice).unwrap();
        assert!(matches!(other.save(&base), Err(Error::Conflict(_))));
        assert!(matches!(other.save(&bob), Err(Error::Conflict(_))));
        assert_eq!(other.load(alice.id).unwrap(), alice);
    }
    let db = Store::open(&path).unwrap();
    // Reopening the file also preserves the complete history (project ID found via a separate connection).
    let connection = rusqlite::Connection::open(&path).unwrap();
    let id: String = connection
        .query_row("SELECT id FROM projects", [], |r| r.get(0))
        .unwrap();
    assert_eq!(db.load(id.parse().unwrap()).unwrap().changes.len(), 1);
    drop(connection);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn file_export_roundtrip_refuses_to_overwrite_existing_file() {
    let path = std::env::temp_dir().join(format!("calcelec-{}.json", Uuid::new_v4()));
    let p = project();
    let json = p.to_json().unwrap();
    storage::export_new(&path, &json).unwrap();
    assert!(storage::export_new(&path, "replacement").is_err());
    assert_eq!(storage::read_project(&path).unwrap(), p);
    std::fs::remove_file(path).unwrap();
}
