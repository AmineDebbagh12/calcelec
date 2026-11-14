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
