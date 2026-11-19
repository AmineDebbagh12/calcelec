use calcelec::{
    electrical::{Conductor, ElectricalData},
    geometry::{Coordinate, Geometry},
    model::{Equipment, Object},
    project::Project,
    reference_cases,
    scenario::{EquipmentOverride, InjectionOverride, Scenario, ScenarioCollection, ScenarioKind},
};
use std::collections::BTreeMap;
use uuid::Uuid;

#[test]
fn compact_scenario_scales_all_injections_then_applies_sparse_overrides() {
    let network = reference_cases::rural_unbalanced();
    let load = network
        .objects
        .values()
        .find(|object| object.name == "Clients BT — A")
        .unwrap();
    let generator = network
        .objects
        .values()
        .find(|object| matches!(object.equipment, Equipment::Generator { .. }))
        .unwrap();
    let scenario = Scenario {
        id: Uuid::new_v4(),
        name: "Pointe hiver".into(),
        kind: ScenarioKind::MaximumConsumption,
        load_scale: 1.2,
        generation_scale: 0.0,
        injections: BTreeMap::from([
            (
                load.id,
                InjectionOverride {
                    in_service: None,
                    p_w: Some(15_000.0),
                    q_var: Some(3_000.0),
                },
            ),
            (
                generator.id,
                InjectionOverride {
                    in_service: Some(false),
                    p_w: None,
                    q_var: None,
                },
            ),
        ]),
        equipment: BTreeMap::new(),
    };

    let applied = scenario.apply(&network).unwrap();
    assert_eq!(network.objects[&load.id].power(), Some((12_000.0, 2_000.0)));
    assert_eq!(applied.objects[&load.id].power(), Some((15_000.0, 3_000.0)));
    assert!(!applied.objects[&generator.id].in_service);
    let aggregate = applied
        .objects
        .values()
        .find(|object| object.name == "Consommation agrégée 2")
        .unwrap();
    assert_eq!(aggregate.power(), Some((108_000.0, 24_000.0)));

    let encoded = serde_json::to_string(&scenario).unwrap();
    assert_eq!(
        serde_json::from_str::<Scenario>(&encoded).unwrap(),
        scenario
    );
}

#[test]
fn collection_is_bound_to_one_project_version_and_rejects_duplicate_ids() {
    let project = Project::new(reference_cases::rural_unbalanced()).unwrap();
    let scenario = Scenario {
        id: Uuid::new_v4(),
        name: "Pointe".into(),
        kind: ScenarioKind::MaximumConsumption,
        load_scale: 1.1,
        generation_scale: 0.2,
        injections: BTreeMap::new(),
        equipment: BTreeMap::new(),
    };
    let collection = ScenarioCollection::new(&project, vec![scenario.clone()]).unwrap();
    let json = collection.to_json(&project).unwrap();
    assert_eq!(
        ScenarioCollection::from_json(&project, &json).unwrap(),
        collection
    );
    assert!(ScenarioCollection::new(&project, vec![scenario.clone(), scenario]).is_err());

    let other = Project::new(reference_cases::rural_unbalanced()).unwrap();
    assert!(ScenarioCollection::from_json(&other, &json).is_err());
}

#[test]
fn scenario_refuses_invalid_coefficients_and_non_injection_overrides() {
    let network = reference_cases::rural_unbalanced();
    let bus = network
        .objects
        .values()
        .find(|object| matches!(object.equipment, Equipment::Bus { .. }))
        .unwrap();
    let mut scenario = Scenario {
        id: Uuid::new_v4(),
        name: "Invalide".into(),
        kind: ScenarioKind::Custom,
        load_scale: -1.0,
        generation_scale: 1.0,
        injections: BTreeMap::new(),
        equipment: BTreeMap::new(),
    };
    assert!(scenario.apply(&network).is_err());
    scenario.load_scale = 1.0;
    scenario.injections.insert(
        bus.id,
        InjectionOverride {
            in_service: Some(false),
            p_w: None,
            q_var: None,
        },
    );
    assert!(scenario.apply(&network).is_err());
}

#[test]
fn operating_overrides_change_service_and_tap_only_in_the_study_copy() {
    let network = reference_cases::rural_meshed();
    let line = network
        .objects
        .values()
        .find(|object| matches!(object.equipment, Equipment::Line { .. }))
        .unwrap();
    let mut operating = Scenario {
        id: Uuid::new_v4(),
        name: "Départ indisponible".into(),
        kind: ScenarioKind::Custom,
        load_scale: 1.0,
        generation_scale: 1.0,
        injections: BTreeMap::new(),
        equipment: BTreeMap::from([(
            line.id,
            EquipmentOverride {
                in_service: Some(false),
                switch_closed: None,
                tap_position: None,
            },
        )]),
    };
    let applied = operating.apply(&network).unwrap();
    assert!(network.objects[&line.id].in_service);
    assert!(!applied.objects[&line.id].in_service);

    let transformer_network = reference_cases::dyn_transformer();
    let transformer = transformer_network
        .objects
        .values()
        .find(|object| matches!(object.equipment, Equipment::Transformer { .. }))
        .unwrap();
    let calcelec::electrical::ElectricalData::Transformer(data) =
        transformer.electrical.as_ref().unwrap()
    else {
        unreachable!()
    };
    let tap = data.tap.as_ref().unwrap();
    let target = if tap.position < tap.max_position {
        tap.position + 1
    } else {
        tap.position - 1
    };
    operating.equipment = BTreeMap::from([(
        transformer.id,
        EquipmentOverride {
            in_service: None,
            switch_closed: None,
            tap_position: Some(target),
        },
    )]);
    let applied = operating.apply(&transformer_network).unwrap();
    let Some(calcelec::electrical::ElectricalData::Transformer(applied_data)) =
        &applied.objects[&transformer.id].electrical
    else {
        unreachable!()
    };
    assert_eq!(applied_data.tap.as_ref().unwrap().position, target);
    assert_eq!(data.tap.as_ref().unwrap().position, tap.position);
}

#[test]
fn operating_override_rejects_fields_for_the_wrong_equipment_kind() {
    let network = reference_cases::rural_unbalanced();
    let line = network
        .objects
        .values()
        .find(|object| matches!(object.equipment, Equipment::Line { .. }))
        .unwrap();
    let scenario = Scenario {
        id: Uuid::new_v4(),
        name: "Invalide".into(),
        kind: ScenarioKind::Custom,
        load_scale: 1.0,
        generation_scale: 1.0,
        injections: BTreeMap::new(),
        equipment: BTreeMap::from([(
            line.id,
            EquipmentOverride {
                in_service: None,
                switch_closed: Some(false),
                tap_position: None,
            },
        )]),
    };
    assert!(
        scenario
            .apply(&network)
            .unwrap_err()
            .to_string()
            .contains("interrupteur")
    );
}

#[test]
fn operating_override_opens_and_closes_a_switch_without_changing_the_base() {
    let mut network = reference_cases::rural_unbalanced();
    let buses: Vec<_> = network
        .objects
        .values()
        .filter(|object| {
            matches!(object.equipment, Equipment::Bus { voltage_v } if voltage_v == 20_000.0)
        })
        .map(|object| object.id)
        .take(2)
        .collect();
    let mut switch = Object::new(
        "Interrupteur de scénario",
        Equipment::Switch {
            from: buses[0],
            to: buses[1],
            closed: false,
        },
    );
    switch.geometry = Geometry::Point(Coordinate(0.0, 0.0));
    switch.electrical = Some(ElectricalData::Switch {
        conductors: vec![Conductor::A, Conductor::B, Conductor::C],
    });
    let switch_id = switch.id;
    network.objects.insert(switch_id, switch);
    network.validate().unwrap();
    let scenario = Scenario {
        id: Uuid::new_v4(),
        name: "Bouclage fermé".into(),
        kind: ScenarioKind::Custom,
        load_scale: 1.0,
        generation_scale: 1.0,
        injections: BTreeMap::new(),
        equipment: BTreeMap::from([(
            switch_id,
            EquipmentOverride {
                in_service: None,
                switch_closed: Some(true),
                tap_position: None,
            },
        )]),
    };
    let applied = scenario.apply(&network).unwrap();
    assert!(matches!(
        network.objects[&switch_id].equipment,
        Equipment::Switch { closed: false, .. }
    ));
    assert!(matches!(
        applied.objects[&switch_id].equipment,
        Equipment::Switch { closed: true, .. }
    ));
}
