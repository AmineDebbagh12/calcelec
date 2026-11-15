//! Synthetic analytical cases. These values are not an Enedis catalogue.
use crate::{
    electrical::*,
    geometry::{Coordinate, CoordinateReference, Geometry},
    model::{Equipment, Network, Object, Phase},
};
use std::collections::BTreeMap;
fn object(
    name: &str,
    equipment: Equipment,
    electrical: ElectricalData,
    point: Coordinate,
) -> Object {
    let mut object = Object::new(name, equipment);
    object.electrical = Some(electrical);
    object.geometry = Geometry::Point(point);
    object
}
fn bus_data(neutral: bool, grounded: bool) -> ElectricalData {
    let mut conductors = vec![Conductor::A, Conductor::B, Conductor::C];
    if neutral {
        conductors.push(Conductor::N);
    }
    ElectricalData::Bus(BusData {
        conductors,
        neutral_grounding: if !neutral {
            Grounding::NotApplicable
        } else if grounded {
            Grounding::Impedance {
                ohm: Impedance::ZERO,
            }
        } else {
            Grounding::Isolated
        },
    })
}
fn source_data() -> ElectricalData {
    ElectricalData::Source(SourceData {
        positive_sequence_ohm: Some(Impedance { r: 0.1, x: 0.2 }),
        negative_sequence_ohm: Some(Impedance { r: 0.1, x: 0.2 }),
        zero_sequence_ohm: Some(Impedance { r: 0.3, x: 0.4 }),
        neutral_grounding: Grounding::Impedance {
            ohm: Impedance::ZERO,
        },
    })
}
fn load_data() -> ElectricalData {
    ElectricalData::Injection(InjectionData {
        connection: InjectionConnection::ThreePhaseStar { neutral: true },
        fault_contribution: FaultContribution::None,
    })
}
fn network(name: &str, objects: Vec<Object>) -> Network {
    Network {
        name: name.into(),
        frequency_hz: 50.0,
        coordinate_reference: CoordinateReference::LocalMeters,
        objects: objects
            .into_iter()
            .map(|o| (o.id, o))
            .collect::<BTreeMap<_, _>>(),
    }
}

pub fn four_wire() -> Network {
    let a = object(
        "Source BT",
        Equipment::Bus { voltage_v: 400.0 },
        bus_data(true, true),
        Coordinate(0.0, 0.0),
    );
    let b = object(
        "Client BT",
        Equipment::Bus { voltage_v: 400.0 },
        bus_data(true, false),
        Coordinate(100.0, 0.0),
    );
    let source = object(
        "Source idéale en régime permanent",
        Equipment::Source {
            bus: a.id,
            voltage_pu: 1.0,
        },
        source_data(),
        Coordinate(0.0, 0.0),
    );
    let conductors = vec![Conductor::A, Conductor::B, Conductor::C, Conductor::N];
    let mut matrix = vec![vec![Impedance { r: 0.02, x: 0.04 }; 4]; 4];
    for (i, row) in matrix.iter_mut().enumerate() {
        row[i] = Impedance {
            r: if i == 3 { 0.8 } else { 0.4 },
            x: 0.1,
        };
    }
    let mut line = object(
        "Câble ABCN — 100 m",
        Equipment::Line {
            from: a.id,
            to: b.id,
            length_m: 100.0,
            r_ohm_per_km: 0.38,
            x_ohm_per_km: 0.06,
            max_current_a: 150.0,
        },
        ElectricalData::Line(LineData {
            conductors,
            series_ohm_per_km: matrix,
            shunt_siemens_per_km: None,
        }),
        Coordinate(0.0, 0.0),
    );
    line.geometry = Geometry::Polyline(vec![Coordinate(0.0, 0.0), Coordinate(100.0, 0.0)]);
    let load = object(
        "Charge équilibrée 30 kW",
        Equipment::Load {
            bus: b.id,
            p_w: 30000.0,
            q_var: 0.0,
            phase: Phase::Abc,
        },
        load_data(),
        Coordinate(100.0, 10.0),
    );
    network(
        "Référence synthétique — câble quatre conducteurs",
        vec![a, b, source, line, load],
    )
}
pub fn transformer() -> Network {
    let a = object(
        "Primaire 20 kV",
        Equipment::Bus { voltage_v: 20000.0 },
        bus_data(false, false),
        Coordinate(0.0, 0.0),
    );
    let b = object(
        "Secondaire 400 V",
        Equipment::Bus { voltage_v: 400.0 },
        bus_data(true, true),
        Coordinate(0.0, 0.0),
    );
    let source = object(
        "Source HTA",
        Equipment::Source {
            bus: a.id,
            voltage_pu: 1.0,
        },
        source_data(),
        Coordinate(0.0, 0.0),
    );
    let transformer = object(
        "Transformateur analytique Yyn0",
        Equipment::Transformer {
            from: a.id,
            to: b.id,
            rated_va: 100000.0,
            uk_percent: 4.0,
        },
        ElectricalData::Transformer(TransformerData {
            from: Winding {
                connection: WindingConnection::Star,
                rated_voltage_ll_v: 20000.0,
                neutral_terminal: false,
                grounding: Grounding::Isolated,
            },
            to: Winding {
                connection: WindingConnection::Star,
                rated_voltage_ll_v: 400.0,
                neutral_terminal: true,
                grounding: Grounding::Impedance {
                    ohm: Impedance::ZERO,
                },
            },
            clock: 0,
            copper_loss_w: 1000.0,
            iron_loss_w: Some(0.0),
            no_load_current_percent: Some(0.0),
            tap: Some(TapChanger {
                side: Side::From,
                min_position: -2,
                max_position: 2,
                neutral_position: 0,
                position: 0,
                step_percent: 2.5,
            }),
            zero_sequence_ohm_referred_from: Some(Impedance { r: 40.0, x: 150.0 }),
        }),
        Coordinate(0.0, 0.0),
    );
    let load = object(
        "Charge équilibrée 30 kW + j9 kvar",
        Equipment::Load {
            bus: b.id,
            p_w: 30000.0,
            q_var: 9000.0,
            phase: Phase::Abc,
        },
        load_data(),
        Coordinate(20.0, 10.0),
    );
    network(
        "Référence synthétique — transformateur résistif Yyn0",
        vec![a, b, source, transformer, load],
    )
}

/// Synthetic Dyn11 HTA/BT case with unequal phase-to-neutral loads.
pub fn dyn_transformer() -> Network {
    let mut n = transformer();
    n.name = "Référence synthétique — transformateur Dyn11 déséquilibré".into();
    n.objects
        .retain(|_, object| !matches!(object.equipment, Equipment::Load { .. }));
    let secondary = n
        .objects
        .values()
        .find(|object| object.name == "Secondaire 400 V")
        .unwrap()
        .id;
    let transformer = n
        .objects
        .values_mut()
        .find(|object| matches!(object.equipment, Equipment::Transformer { .. }))
        .unwrap();
    transformer.name = "Transformateur analytique Dyn11".into();
    let Some(ElectricalData::Transformer(data)) = &mut transformer.electrical else {
        unreachable!()
    };
    data.from.connection = WindingConnection::Delta;
    data.from.grounding = Grounding::NotApplicable;
    data.clock = 11;
    for (phase, conductor, p_w, q_var, y) in [
        (Phase::A, Conductor::A, 12000.0, 1800.0, 20.0),
        (Phase::B, Conductor::B, 8000.0, 1000.0, 0.0),
        (Phase::C, Conductor::C, 5000.0, 600.0, -20.0),
    ] {
        let load = object(
            &format!("Charge BT phase {phase:?}"),
            Equipment::Load {
                bus: secondary,
                p_w,
                q_var,
                phase,
            },
            ElectricalData::Injection(InjectionData {
                connection: InjectionConnection::PhaseNeutral { phase: conductor },
                fault_contribution: FaultContribution::None,
            }),
            Coordinate(20.0, y),
        );
        n.objects.insert(load.id, load);
    }
    n
}

/// Synthetic Yzn11 HTA/BT case with the same unequal phase-to-neutral loads.
pub fn yzn_transformer() -> Network {
    let mut n = dyn_transformer();
    n.name = "Référence synthétique — transformateur Yzn11 déséquilibré".into();
    let transformer = n
        .objects
        .values_mut()
        .find(|object| matches!(object.equipment, Equipment::Transformer { .. }))
        .unwrap();
    transformer.name = "Transformateur analytique Yzn11".into();
    let Some(ElectricalData::Transformer(data)) = &mut transformer.electrical else {
        unreachable!()
    };
    data.from.connection = WindingConnection::Star;
    data.from.neutral_terminal = false;
    data.from.grounding = Grounding::Isolated;
    data.to.connection = WindingConnection::Zigzag;
    n
}

/// Synthetic Dyn11 case followed by a four-wire BT cable and a terminal earth electrode.
pub fn dyn_grounded_transformer() -> Network {
    let mut n = dyn_transformer();
    n.name = "Référence synthétique — Dyn11 avec terre BT aval".into();
    let secondary = n
        .objects
        .values()
        .find(|object| object.name == "Secondaire 400 V")
        .unwrap()
        .id;
    let client = object(
        "Client BT avec terre",
        Equipment::Bus { voltage_v: 400.0 },
        ElectricalData::Bus(BusData {
            conductors: vec![Conductor::A, Conductor::B, Conductor::C, Conductor::N],
            neutral_grounding: Grounding::Impedance {
                ohm: Impedance { r: 1.0, x: 0.0 },
            },
        }),
        Coordinate(50.0, 0.0),
    );
    let client_id = client.id;
    let line_data = four_wire()
        .objects
        .into_values()
        .find_map(|object| match object.electrical {
            Some(ElectricalData::Line(data)) => Some(data),
            _ => None,
        })
        .unwrap();
    let mut line = object(
        "Câble BT ABCN — 50 m",
        Equipment::Line {
            from: secondary,
            to: client_id,
            length_m: 50.0,
            r_ohm_per_km: 0.38,
            x_ohm_per_km: 0.06,
            max_current_a: 150.0,
        },
        ElectricalData::Line(line_data),
        Coordinate(0.0, 0.0),
    );
    line.geometry = Geometry::Polyline(vec![Coordinate(0.0, 0.0), Coordinate(50.0, 0.0)]);
    for object in n.objects.values_mut() {
        if let Equipment::Load { bus, .. } = &mut object.equipment {
            *bus = client_id;
        }
    }
    n.objects.insert(line.id, line);
    n.objects.insert(client.id, client);
    n
}

/// Synthetic BT feeder with distinct single-phase loads and a single-phase PV injection.
pub fn unbalanced_bt() -> Network {
    let mut n = four_wire();
    n.name = "Référence synthétique — BT déséquilibrée avec neutre et PV".into();
    n.objects
        .retain(|_, o| !matches!(o.equipment, Equipment::Load { .. }));
    let bus = n
        .objects
        .values()
        .find(|o| o.name == "Client BT")
        .unwrap()
        .id;
    for (phase, conductor, p_w, q_var, y) in [
        (Phase::A, Conductor::A, 12000.0, 1800.0, 20.0),
        (Phase::B, Conductor::B, 8000.0, 1000.0, 0.0),
        (Phase::C, Conductor::C, 5000.0, 600.0, -20.0),
    ] {
        let load = object(
            &format!("Clients phase {phase:?}"),
            Equipment::Load {
                bus,
                p_w,
                q_var,
                phase,
            },
            ElectricalData::Injection(InjectionData {
                connection: InjectionConnection::PhaseNeutral { phase: conductor },
                fault_contribution: FaultContribution::None,
            }),
            Coordinate(130.0, y),
        );
        n.objects.insert(load.id, load);
    }
    let pv = object(
        "PV monophasé C — 10 kW",
        Equipment::Generator {
            bus,
            p_w: 10000.0,
            q_var: 0.0,
            phase: Phase::C,
        },
        ElectricalData::Injection(InjectionData {
            connection: InjectionConnection::PhaseNeutral {
                phase: Conductor::C,
            },
            fault_contribution: FaultContribution::CurrentLimited {
                max_current_a: 60.0,
            },
        }),
        Coordinate(160.0, -20.0),
    );
    n.objects.insert(pv.id, pv);
    n
}

/// Synthetic feeder with two distinct downstream neutral earth electrodes.
pub fn grounded_bt() -> Network {
    let mut n = unbalanced_bt();
    n.name = "Référence synthétique — BT avec prises de terre du neutre".into();
    let end = n
        .objects
        .values()
        .find(|o| o.name == "Client BT")
        .unwrap()
        .id;
    let mut middle = n.objects[&end].clone();
    middle.id = uuid::Uuid::new_v4();
    middle.name = "Point de terre intermédiaire".into();
    middle.geometry = Geometry::Point(Coordinate(50.0, 0.0));
    let Some(ElectricalData::Bus(data)) = &mut middle.electrical else {
        unreachable!()
    };
    data.neutral_grounding = Grounding::Impedance {
        ohm: Impedance { r: 0.5, x: 0.0 },
    };
    let mid = middle.id;
    n.objects.insert(mid, middle);
    let Some(ElectricalData::Bus(data)) = &mut n.objects.get_mut(&end).unwrap().electrical else {
        unreachable!()
    };
    data.neutral_grounding = Grounding::Impedance {
        ohm: Impedance { r: 1.0, x: 0.0 },
    };
    let original_id = n
        .objects
        .values()
        .find(|o| matches!(o.equipment, Equipment::Line { .. }))
        .unwrap()
        .id;
    let mut tail = n.objects[&original_id].clone();
    tail.id = uuid::Uuid::new_v4();
    tail.name = "Câble BT — aval".into();
    if let Equipment::Line { from, length_m, .. } = &mut tail.equipment {
        *from = mid;
        *length_m = 50.0;
    }
    tail.geometry = Geometry::Polyline(vec![Coordinate(50.0, 0.0), Coordinate(100.0, 0.0)]);
    let trunk = n.objects.get_mut(&original_id).unwrap();
    trunk.name = "Câble BT — amont".into();
    if let Equipment::Line { to, length_m, .. } = &mut trunk.equipment {
        *to = mid;
        *length_m = 50.0;
    }
    trunk.geometry = Geometry::Polyline(vec![Coordinate(0.0, 0.0), Coordinate(50.0, 0.0)]);
    n.objects.insert(tail.id, tail);
    n
}

/// Synthetic unbalanced feeder with downstream electrodes and a finite source earth impedance.
pub fn source_grounded_bt() -> Network {
    let mut n = grounded_bt();
    n.name = "Référence synthétique — BT avec terre source impédante".into();
    let grounding = Grounding::Impedance {
        ohm: Impedance { r: 0.2, x: 0.0 },
    };
    for object in n.objects.values_mut() {
        match &mut object.electrical {
            Some(ElectricalData::Bus(data)) if object.name == "Source BT" => {
                data.neutral_grounding = grounding.clone();
            }
            Some(ElectricalData::Source(data)) => {
                data.neutral_grounding = grounding.clone();
            }
            _ => {}
        }
    }
    n
}
