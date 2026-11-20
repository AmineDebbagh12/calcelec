use calcelec::{
    geometry::{Coordinate, CoordinateReference, Geometry},
    model::{Equipment, rural_example},
    project::{Operation, Project, SCHEMA_VERSION},
    storage::Store,
};
use uuid::Uuid;

fn legacy_json(project: &Project) -> String {
    let mut value = serde_json::to_value(project).unwrap();
    value["schema_version"] = 1.into();
    value["root"]
        .as_object_mut()
        .unwrap()
        .remove("coordinate_reference");
    value["root"]
        .as_object_mut()
        .unwrap()
        .remove("technologies");
    for object in value["root"]["objects"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        object.as_object_mut().unwrap().remove("geometry");
    }
    for change in value["changes"].as_array_mut().unwrap() {
        change["schema_version"] = 1.into();
    }
    value.to_string()
}

#[test]
fn example_locates_every_object_and_preserves_polyline_vertices_in_json() {
    let project = Project::new(rural_example()).unwrap();
    assert!(
        project
            .root
            .objects
            .values()
            .all(|o| !matches!(o.geometry, Geometry::Unlocated))
    );
    assert!(
        project
            .root
            .objects
            .values()
            .filter(|o| matches!(o.equipment, Equipment::Line { .. }))
            .all(|o| matches!(&o.geometry,Geometry::Polyline(points) if points.len()>2))
    );
    assert_eq!(
        Project::from_json(&project.to_json().unwrap()).unwrap(),
        project
    );
}

#[test]
fn geographic_delta_survives_sharing_sqlite_and_does_not_alter_electrical_length() {
    let mut alice = Project::new(rural_example()).unwrap();
    let mut bob = alice.clone();
    let line = alice
        .root
        .objects
        .values()
        .find(|o| matches!(o.equipment, Equipment::Line { .. }))
        .unwrap()
        .clone();
    let geometry = Geometry::Polyline(vec![
        Coordinate(700000.0, 6600000.0),
        Coordinate(700030.0, 6600040.0),
        Coordinate(700090.0, 6600040.0),
    ]);
    assert_eq!(geometry.length_m(), Some(110.0));
    let delta = alice.prepare(
        "Géomètre",
        "Rectification du tracé",
        vec![Operation::SetGeometry {
            id: line.id,
            geometry: geometry.clone(),
        }],
    );
    let json = serde_json::to_string(&delta).unwrap();
    alice.apply(delta).unwrap();
    bob.apply(serde_json::from_str(&json).unwrap()).unwrap();
    let network = bob.network().unwrap();
    assert_eq!(network.objects[&line.id].geometry, geometry);
    assert_eq!(network.objects[&line.id].equipment, line.equipment);
    assert_eq!(alice, bob);
    let mut db = Store::open(":memory:").unwrap();
    db.save(&alice).unwrap();
    assert_eq!(db.load(alice.id).unwrap(), alice);
}

#[test]
fn invalid_geometry_rolls_back_other_changes_in_batch() {
    for invalid in [
        Geometry::Polyline(vec![]),
        Geometry::Polyline(vec![Coordinate(0.0, 0.0)]),
        Geometry::Polyline(vec![Coordinate(0.0, 0.0), Coordinate(0.0, 0.0)]),
        Geometry::Polyline(vec![Coordinate(f64::NAN, 0.0), Coordinate(1.0, 0.0)]),
        Geometry::Point(Coordinate(f64::INFINITY, 0.0)),
    ] {
        let mut p = Project::new(rural_example()).unwrap();
        let before = p.clone();
        let id = p
            .root
            .objects
            .values()
            .find(|o| matches!(o.equipment, Equipment::Line { .. }))
            .unwrap()
            .id;
        let delta = p.prepare(
            "Test",
            "Géométrie invalide",
            vec![
                Operation::Rename {
                    id,
                    name: "Renommage à annuler".into(),
                },
                Operation::SetGeometry {
                    id,
                    geometry: invalid,
                },
            ],
        );
        assert!(p.apply(delta).is_err());
        assert_eq!(p, before);
    }
    let mut n = rural_example();
    let point = n
        .objects
        .values_mut()
        .find(|o| matches!(o.equipment, Equipment::Transformer { .. }))
        .unwrap();
    point.geometry = Geometry::Polyline(vec![Coordinate(0.0, 0.0), Coordinate(1.0, 0.0)]);
    assert!(n.validate().is_err());
}

#[test]
fn moving_point_does_not_change_connectivity() {
    let mut p = Project::new(rural_example()).unwrap();
    let before = p.network().unwrap();
    let bus = before
        .objects
        .values()
        .find(|o| matches!(o.equipment, Equipment::Bus { .. }))
        .unwrap()
        .id;
    let delta = p.prepare(
        "Test",
        "Déplacement",
        vec![Operation::SetGeometry {
            id: bus,
            geometry: Geometry::Point(Coordinate(5000.0, 2000.0)),
        }],
    );
    p.apply(delta).unwrap();
    let after = p.network().unwrap();
    assert_eq!(after.unpowered_buses(), before.unpowered_buses());
    for (id, object) in before.objects {
        assert_eq!(after.objects[&id].equipment, object.equipment);
    }
}

#[test]
fn legacy_project_and_old_delta_migrate_without_inventing_locations_or_changing_ids() {
    let original = Project::new(rural_example()).unwrap();
    let legacy = legacy_json(&original);
    let mut migrated = Project::from_json(&legacy).unwrap();
    assert_eq!(migrated.id, original.id);
    assert_eq!(migrated.head(), original.head());
    assert_eq!(migrated.schema_version, SCHEMA_VERSION);
    assert_eq!(
        migrated.root.coordinate_reference,
        CoordinateReference::LocalMeters
    );
    assert!(
        migrated
            .root
            .objects
            .values()
            .all(|o| o.geometry == Geometry::Unlocated)
    );
    let id = *migrated.root.objects.keys().next().unwrap();
    let mut change = migrated.prepare(
        "Ancien poste",
        "Renommage",
        vec![Operation::Rename {
            id,
            name: "Nom partagé".into(),
        }],
    );
    change.schema_version = 1;
    migrated.apply(change.clone()).unwrap();
    assert!(!migrated.apply(change).unwrap());
    assert_eq!(migrated.network().unwrap().objects[&id].name, "Nom partagé");
    assert_eq!(
        Project::from_json(&migrated.to_json().unwrap()).unwrap(),
        migrated
    );
}

#[test]
fn schema_one_cannot_claim_to_contain_geometry() {
    let original = Project::new(rural_example()).unwrap();
    let mut raw = serde_json::to_value(original).unwrap();
    raw["schema_version"] = 1.into();
    assert!(Project::from_json(&raw.to_string()).is_err());
}

#[test]
fn old_sqlite_project_with_history_is_migrated_transactionally_on_save() {
    let mut original = Project::new(rural_example()).unwrap();
    let id = *original.root.objects.keys().next().unwrap();
    let delta = original.prepare(
        "Ancien utilisateur",
        "Nom",
        vec![Operation::Rename {
            id,
            name: "Nouveau nom".into(),
        }],
    );
    original.apply(delta).unwrap();
    let old: serde_json::Value = serde_json::from_str(&legacy_json(&original)).unwrap();
    let path = std::env::temp_dir().join(format!("calcelec-migration-{}.sqlite", Uuid::new_v4()));
    {
        let mut db = Store::open(&path).unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute(
                "INSERT INTO projects VALUES (?1,1,?2,?3,?4)",
                rusqlite::params![
                    original.id.to_string(),
                    original.root_version.to_string(),
                    old["root"].to_string(),
                    original.head().to_string()
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO changes VALUES (?1,0,?2,?3)",
                rusqlite::params![
                    original.id.to_string(),
                    original.changes[0].id.to_string(),
                    old["changes"][0].to_string()
                ],
            )
            .unwrap();
        let migrated = db.load(original.id).unwrap();
        assert_eq!(migrated.schema_version, SCHEMA_VERSION);
        assert_eq!(migrated.head(), original.head());
        assert!(
            migrated
                .root
                .objects
                .values()
                .all(|o| o.geometry == Geometry::Unlocated)
        );
        db.save(&migrated).unwrap();
        db.save(&migrated).unwrap();
        assert_eq!(db.load(original.id).unwrap(), migrated);
        let version: u32 = connection
            .query_row("SELECT schema_version FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }
    std::fs::remove_file(path).unwrap();
}
