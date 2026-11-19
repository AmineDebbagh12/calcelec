use calcelec::{
    editor::{self, BranchKind, EditHistory, Placement},
    geometry::Coordinate,
    model::{Equipment, Network, SwitchInstallation, rural_example},
    project::{Operation, Project, SCHEMA_VERSION},
    storage::Store,
};

fn bus(network: &Network, voltage: f64) -> uuid::Uuid {
    network
        .objects
        .values()
        .find(|o| matches!(o.equipment,Equipment::Bus {voltage_v} if voltage_v==voltage))
        .unwrap()
        .id
}
#[test]
fn station_is_one_atomic_edit_and_undo_redo_preserve_shared_history() {
    let mut p = Project::new(rural_example()).unwrap();
    let base = p.clone();
    let before = p.network().unwrap();
    let mut history = EditHistory::default();
    let operations = editor::placement(
        &before,
        Placement::Station,
        Coordinate(5000.0, 1000.0),
        None,
    )
    .unwrap();
    history
        .commit(&mut p, "Alice", "Nouveau poste", operations)
        .unwrap();
    let created = p.network().unwrap();
    assert_eq!(created.objects.len(), before.objects.len() + 3);
    assert_eq!(p.changes.len(), 1);
    assert!(history.can_undo(&p));
    history.undo(&mut p, "Alice").unwrap();
    assert_eq!(p.network().unwrap(), before);
    assert_eq!(p.changes.len(), 2);
    history.redo(&mut p, "Alice").unwrap();
    assert_eq!(p.network().unwrap(), created);
    assert_eq!(p.changes.len(), 3);
    let mut remote = base;
    for delta in &p.changes {
        remote.apply(delta.clone()).unwrap();
    }
    assert_eq!(remote, p);
}
#[test]
fn electrical_reconnection_and_geography_are_independent_and_reversible() {
    let mut p = Project::new(rural_example()).unwrap();
    let mut history = EditHistory::default();
    let n = p.network().unwrap();
    let load = n
        .objects
        .values()
        .find(|o| matches!(o.equipment, Equipment::Load { .. }))
        .unwrap()
        .clone();
    let mut equipment = load.equipment.clone();
    let target = bus(&n, 400.0);
    if let Equipment::Load { bus, p_w, .. } = &mut equipment {
        *bus = target;
        *p_w = 16000.0;
    }
    history
        .commit(
            &mut p,
            "Test",
            "Raccordement",
            vec![Operation::SetEquipment {
                id: load.id,
                equipment: equipment.clone(),
            }],
        )
        .unwrap();
    let current = p.network().unwrap();
    assert_eq!(current.objects[&load.id].geometry, load.geometry);
    assert_eq!(current.objects[&load.id].equipment, equipment);
    history.undo(&mut p, "Test").unwrap();
    assert_eq!(p.network().unwrap(), n);
}
#[test]
fn traced_branch_has_explicit_endpoints_and_initial_electrical_length() {
    let n = rural_example();
    let mut nodes = n
        .objects
        .values()
        .filter(|o| matches!(o.equipment,Equipment::Bus {voltage_v} if voltage_v==20000.0));
    let from = nodes.next().unwrap().id;
    let to = nodes.next().unwrap().id;
    let operation = editor::branch(
        &n,
        BranchKind::Line,
        Some(from),
        Some(to),
        &[Coordinate(1900.0, 800.0)],
    )
    .unwrap();
    let Operation::Create { object } = operation else {
        unreachable!()
    };
    let Equipment::Line {
        from: a,
        to: b,
        length_m,
        ..
    } = object.equipment
    else {
        unreachable!()
    };
    assert_eq!((a, b), (from, to));
    assert_eq!(object.geometry.length_m(), Some(length_m));
    assert_eq!(
        object.geometry.coordinates().first(),
        Some(&editor::bus_point(&n, Some(from)).unwrap())
    );
    assert_eq!(
        object.geometry.coordinates().last(),
        Some(&editor::bus_point(&n, Some(to)).unwrap())
    );
}
#[test]
fn invalid_routing_and_incomplete_attachments_are_rejected() {
    let n = rural_example();
    let hta = bus(&n, 20000.0);
    let bt = bus(&n, 400.0);
    let other_bt = n
        .objects
        .values()
        .find(|o| {
            o.id != bt && matches!(o.equipment, Equipment::Bus { voltage_v } if voltage_v == 400.0)
        })
        .unwrap()
        .id;
    assert!(editor::branch(&n, BranchKind::Line, Some(hta), Some(bt), &[]).is_err());
    assert!(editor::branch(&n, BranchKind::Switch, Some(hta), Some(hta), &[]).is_err());
    assert!(editor::branch(&n, BranchKind::Switch, Some(bt), Some(other_bt), &[]).is_err());
    assert!(editor::placement(&n, Placement::Load, Coordinate(1.0, 1.0), None).is_err());
    assert!(
        editor::placement(
            &n,
            Placement::Generator,
            Coordinate(f64::NAN, 1.0),
            Some(bt)
        )
        .is_err()
    );
}

#[test]
fn aerial_switch_is_an_explicit_hta_geographic_asset() {
    let n = rural_example();
    let buses: Vec<_> = n
        .objects
        .values()
        .filter(|o| matches!(o.equipment, Equipment::Bus { voltage_v } if voltage_v == 20_000.0))
        .map(|o| o.id)
        .collect();
    let operation =
        editor::branch(&n, BranchKind::Switch, Some(buses[0]), Some(buses[1]), &[]).unwrap();
    let Operation::Create { object } = operation else {
        unreachable!()
    };
    assert_eq!(object.kind(), "Interrupteur aérien HTA");
    assert!(matches!(
        object.equipment,
        Equipment::Switch {
            installation: SwitchInstallation::AerialHta,
            closed: true,
            ..
        }
    ));
    assert!(matches!(
        object.geometry,
        calcelec::geometry::Geometry::Point(_)
    ));
}

#[test]
fn switch_without_installation_keeps_legacy_json_compatibility() {
    let equipment: Equipment = serde_json::from_value(serde_json::json!({
        "type": "switch",
        "from": "10000000-0000-4000-8000-000000000001",
        "to": "10000000-0000-4000-8000-000000000002",
        "closed": false
    }))
    .unwrap();
    assert!(matches!(
        equipment,
        Equipment::Switch {
            installation: SwitchInstallation::Generic,
            ..
        }
    ));
}
#[test]
fn deletion_requires_explicit_dependents_and_undo_restores_them() {
    let mut p = Project::new(rural_example()).unwrap();
    let n = p.network().unwrap();
    let id = bus(&n, 400.0);
    let linked = editor::dependents(&n, id);
    assert!(!linked.is_empty());
    assert!(editor::deletion(&n, id, false).is_err());
    let operations = editor::deletion(&n, id, true).unwrap();
    let mut history = EditHistory::default();
    history
        .commit(&mut p, "Test", "Suppression", operations)
        .unwrap();
    assert!(!p.network().unwrap().objects.contains_key(&id));
    for other in linked {
        assert!(!p.network().unwrap().objects.contains_key(&other));
    }
    history.undo(&mut p, "Test").unwrap();
    assert_eq!(p.network().unwrap(), n);
}
#[test]
fn failed_edit_or_noop_does_not_append_history_or_destroy_redo() {
    let mut p = Project::new(rural_example()).unwrap();
    let mut history = EditHistory::default();
    let id = *p.root.objects.keys().next().unwrap();
    history
        .commit(
            &mut p,
            "Test",
            "Nom",
            vec![Operation::Rename {
                id,
                name: "Autre nom".into(),
            }],
        )
        .unwrap();
    history.undo(&mut p, "Test").unwrap();
    let before = p.clone();
    assert!(
        history
            .commit(
                &mut p,
                "Test",
                "Invalide",
                vec![Operation::Rename {
                    id,
                    name: "".into()
                }]
            )
            .is_err()
    );
    assert_eq!(p, before);
    assert!(history.can_redo(&p));
    let existing_name = p.network().unwrap().objects[&id].name.clone();
    assert!(
        history
            .commit(
                &mut p,
                "Test",
                "Sans effet",
                vec![Operation::Rename {
                    id,
                    name: existing_name
                }]
            )
            .is_err()
    );
    assert_eq!(p, before);
    assert!(history.can_redo(&p));
}
#[test]
fn new_edit_clears_redo_and_external_changes_prevent_stale_undo() {
    let mut p = Project::new(rural_example()).unwrap();
    let mut history = EditHistory::default();
    let id = *p.root.objects.keys().next().unwrap();
    history
        .commit(
            &mut p,
            "Test",
            "A",
            vec![Operation::Rename {
                id,
                name: "A".into(),
            }],
        )
        .unwrap();
    history.undo(&mut p, "Test").unwrap();
    history
        .commit(
            &mut p,
            "Test",
            "B",
            vec![Operation::Rename {
                id,
                name: "B".into(),
            }],
        )
        .unwrap();
    assert!(!history.can_redo(&p));
    let change = p.prepare(
        "Autre poste",
        "C",
        vec![Operation::Rename {
            id,
            name: "C".into(),
        }],
    );
    p.apply(change).unwrap();
    assert!(!history.can_undo(&p));
    assert!(history.undo(&mut p, "Test").is_err());
    history.sync(&p);
    assert!(!history.can_undo(&p));
}
#[test]
fn object_kind_cannot_be_changed_and_invalid_equipment_batch_is_atomic() {
    let mut p = Project::new(rural_example()).unwrap();
    let before = p.clone();
    let id = bus(&p.root, 400.0);
    let delta = p.prepare(
        "Test",
        "Mauvais type",
        vec![Operation::SetEquipment {
            id,
            equipment: Equipment::Bus { voltage_v: -1.0 },
        }],
    );
    assert!(p.apply(delta).is_err());
    assert_eq!(p, before);
    let delta = p.prepare(
        "Test",
        "Mauvais type",
        vec![Operation::SetEquipment {
            id,
            equipment: Equipment::Load {
                bus: id,
                p_w: 1.0,
                q_var: 0.0,
                phase: calcelec::model::Phase::Abc,
            },
        }],
    );
    assert!(p.apply(delta).is_err());
    assert_eq!(p, before);
}
#[test]
fn schema_two_projects_and_sqlite_migrate_to_three_with_ids_preserved() {
    let mut original = Project::new(rural_example()).unwrap();
    let id = bus(&original.root, 400.0);
    let change = original.prepare(
        "Ancien poste",
        "Position",
        vec![Operation::SetGeometry {
            id,
            geometry: calcelec::geometry::Geometry::Point(Coordinate(20.0, 30.0)),
        }],
    );
    original.apply(change).unwrap();
    let mut value = serde_json::to_value(&original).unwrap();
    value["schema_version"] = 2.into();
    value["changes"][0]["schema_version"] = 2.into();
    let migrated = Project::from_json(&value.to_string()).unwrap();
    assert_eq!(migrated, original);
    assert_eq!(migrated.schema_version, SCHEMA_VERSION);
    let path =
        std::env::temp_dir().join(format!("calcelec-editor-{}.sqlite", uuid::Uuid::new_v4()));
    {
        let mut db = Store::open(&path).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO projects VALUES (?1,2,?2,?3,?4)",
            rusqlite::params![
                original.id.to_string(),
                original.root_version.to_string(),
                value["root"].to_string(),
                original.head().to_string()
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO changes VALUES (?1,0,?2,?3)",
            rusqlite::params![
                original.id.to_string(),
                original.changes[0].id.to_string(),
                value["changes"][0].to_string()
            ],
        )
        .unwrap();
        assert_eq!(db.load(original.id).unwrap(), original);
        db.save(&migrated).unwrap();
        assert_eq!(db.load(original.id).unwrap(), migrated);
        let version: u32 = conn
            .query_row("SELECT schema_version FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
fn new_equipment_operation_cannot_be_mislabeled_as_schema_two() {
    let mut p = Project::new(rural_example()).unwrap();
    let id = bus(&p.root, 400.0);
    let before = p.clone();
    let mut change = p.prepare(
        "Test",
        "Modification",
        vec![Operation::SetEquipment {
            id,
            equipment: Equipment::Bus { voltage_v: 400.0 },
        }],
    );
    change.schema_version = 2;
    assert!(p.apply(change).is_err());
    assert_eq!(p, before);
}
