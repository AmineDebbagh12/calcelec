use calcelec::{
    model::{Equipment, Object, SwitchInstallation},
    reference_cases, topology,
};

#[test]
fn topology_report_tracks_open_ties_cycles_and_unenergized_components() {
    let mut network = reference_cases::rural_unbalanced();
    let root = network
        .objects
        .values()
        .find(|object| object.name == "Poste source — HTA")
        .unwrap()
        .id;
    let remote = network
        .objects
        .values()
        .find(|object| object.name == "Poste rural 3 — HTA")
        .unwrap()
        .id;
    let tie = Object::new(
        "Interrupteur de bouclage HTA",
        Equipment::Switch {
            from: root,
            to: remote,
            closed: false,
            installation: SwitchInstallation::AerialHta,
        },
    );
    let tie_id = tie.id;
    network.objects.insert(tie_id, tie);

    let open = topology::analyze(&network).unwrap();
    assert!(open.radial);
    assert_eq!(open.components.len(), 1);
    assert_eq!(open.active_sources.len(), 1);
    assert_eq!(open.open_switches, vec![tie_id]);
    assert!(open.unenergized_buses.is_empty());

    let Equipment::Switch { closed, .. } = &mut network.objects.get_mut(&tie_id).unwrap().equipment
    else {
        unreachable!()
    };
    *closed = true;
    let closed = topology::analyze(&network).unwrap();
    assert!(!closed.radial);
    assert_eq!(closed.closed_switches, vec![tie_id]);
    assert_eq!(closed.cycle_closures.len(), 1);

    let first_line = network
        .objects
        .values_mut()
        .find(|object| object.name == "Départ aérien — tronçon 1")
        .unwrap();
    first_line.in_service = false;
    let Equipment::Switch { closed, .. } = &mut network.objects.get_mut(&tie_id).unwrap().equipment
    else {
        unreachable!()
    };
    *closed = false;
    let isolated = topology::analyze(&network).unwrap();
    assert_eq!(isolated.components.len(), 2);
    assert!(!isolated.unenergized_buses.is_empty());
    assert!(
        isolated
            .components
            .iter()
            .any(|component| !component.energized)
    );
}
