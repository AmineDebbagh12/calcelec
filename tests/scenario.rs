use calcelec::{
    model::Equipment,
    reference_cases,
    scenario::{InjectionOverride, Scenario, ScenarioKind},
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
