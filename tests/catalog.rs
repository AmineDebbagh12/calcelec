use calcelec::{
    catalog::{TechnologyBinding, TechnologyData},
    editor::EditHistory,
    model::{Equipment, rural_example},
    project::{Operation, Project},
    reference_cases,
};

#[test]
fn technology_resolves_parameters_and_binding_is_persisted() {
    let network = rural_example();
    let technology = network
        .technologies
        .values()
        .find(|value| value.designation.starts_with("150AL"))
        .unwrap()
        .clone();
    let line = network
        .objects
        .values()
        .find(|object| matches!(object.equipment, Equipment::Line { .. }))
        .unwrap()
        .clone();
    let resolved = technology.instantiate(&line.equipment).unwrap();
    let mut project = Project::new(network).unwrap();
    let change = project.prepare(
        "Test",
        "Application 150AL",
        vec![
            Operation::SetEquipment {
                id: line.id,
                equipment: resolved,
            },
            Operation::SetTechnology {
                id: line.id,
                technology: Some(TechnologyBinding {
                    technology_id: technology.id,
                    locally_modified: false,
                }),
            },
        ],
    );
    project.apply(change).unwrap();
    let applied = project.network().unwrap();
    assert_eq!(
        applied.objects[&line.id]
            .technology
            .as_ref()
            .unwrap()
            .technology_id,
        technology.id
    );
    let Equipment::Line {
        r_ohm_per_km,
        max_current_a,
        ..
    } = applied.objects[&line.id].equipment
    else {
        unreachable!()
    };
    assert_eq!((r_ohm_per_km, max_current_a), (0.206, 260.0));
}

#[test]
fn local_variant_must_be_explicit() {
    let mut network = rural_example();
    let technology = network
        .technologies
        .values()
        .find(|value| matches!(value.data, TechnologyData::Line { .. }))
        .unwrap()
        .clone();
    let line_id = network
        .objects
        .values()
        .find(|object| matches!(object.equipment, Equipment::Line { .. }))
        .unwrap()
        .id;
    {
        let object = network.objects.get_mut(&line_id).unwrap();
        object.equipment = technology.instantiate(&object.equipment).unwrap();
        object.technology = Some(TechnologyBinding {
            technology_id: technology.id,
            locally_modified: false,
        });
    }
    network.validate().unwrap();
    match &mut network.objects.get_mut(&line_id).unwrap().equipment {
        Equipment::Line { max_current_a, .. } => *max_current_a += 1.0,
        _ => unreachable!(),
    }
    assert!(network.validate().is_err());
    network
        .objects
        .get_mut(&line_id)
        .unwrap()
        .technology
        .as_mut()
        .unwrap()
        .locally_modified = true;
    network.validate().unwrap();
}

#[test]
fn a_catalog_revision_is_immutable() {
    let network = rural_example();
    let mut changed = network.technologies.values().next().unwrap().clone();
    changed.designation.push_str(" modifiée");
    let mut project = Project::new(network).unwrap();
    let change = project.prepare(
        "Test",
        "Réécriture interdite",
        vec![Operation::PutTechnology {
            technology: changed,
        }],
    );
    assert!(project.apply(change).is_err());
}

#[test]
fn catalog_import_is_versioned_and_undoable_when_unused() {
    let technology = calcelec::catalog::educational_catalog()
        .into_values()
        .next()
        .unwrap();
    let technology_id = technology.id;
    let mut project = Project::new(reference_cases::four_wire()).unwrap();
    let mut history = EditHistory::default();
    history
        .commit(
            &mut project,
            "Test",
            "Import catalogue",
            vec![Operation::PutTechnology { technology }],
        )
        .unwrap();
    assert!(
        project
            .network()
            .unwrap()
            .technologies
            .contains_key(&technology_id)
    );
    history.undo(&mut project, "Test").unwrap();
    assert!(
        !project
            .network()
            .unwrap()
            .technologies
            .contains_key(&technology_id)
    );
    history.redo(&mut project, "Test").unwrap();
    assert!(
        project
            .network()
            .unwrap()
            .technologies
            .contains_key(&technology_id)
    );
}

#[test]
fn documented_catalog_example_is_valid() {
    let technologies: Vec<calcelec::catalog::Technology> =
        serde_json::from_str(include_str!("../examples/catalog-technologies-demo.json")).unwrap();
    assert_eq!(technologies.len(), 2);
    for technology in technologies {
        technology.validate().unwrap();
    }
}
