use calcelec::{
    electrical,
    model::Equipment,
    reference_cases, topology,
    unbalanced::{self, Options, Phasor},
};

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{actual} != {expected} (ε={tolerance})"
    );
}

#[test]
fn complete_rural_case_connects_three_grounded_bt_zones_and_balances_power() {
    let network = reference_cases::rural_unbalanced();
    network.validate().unwrap();
    assert!(electrical::missing_data(&network).is_empty());

    let result = unbalanced::solve(&network, &Options::default()).unwrap();
    assert_eq!(result.buses.len(), 8);
    assert_eq!(result.branches.len(), 7);
    assert_eq!(result.groundings.len(), 4);
    assert_eq!(result.iterations, 5);
    near(result.net_demand_p_w, 192_000.0, 1e-8);
    near(result.net_demand_q_var, 46_000.0, 1e-8);
    assert!(result.line_loss_w > 0.0);
    assert!(result.transformer_loss_w > 0.0);
    assert!(result.grounding_loss_w > 0.0);
    near(
        result.total_loss_w,
        result.line_loss_w + result.transformer_loss_w + result.grounding_loss_w,
        1e-9,
    );
    assert!(result.balance_error_va < 1e-3);

    let mut transformer_count = 0;
    let mut hta_line_count = 0;
    for branch in &result.branches {
        let object = &network.objects[&branch.id];
        match object.equipment {
            Equipment::Transformer { .. } => {
                transformer_count += 1;
                let sum = branch.currents_from_a[..3].iter().copied().fold(
                    Phasor { re: 0.0, im: 0.0 },
                    |sum, current| Phasor {
                        re: sum.re + current.re,
                        im: sum.im + current.im,
                    },
                );
                near(sum.magnitude(), 0.0, 1e-9);
                near(branch.currents_from_a[3].magnitude(), 0.0, 1e-12);
            }
            Equipment::Line { from, .. } if matches!(network.objects[&from].equipment, Equipment::Bus { voltage_v } if voltage_v > 1000.0) =>
            {
                hta_line_count += 1;
                near(branch.currents_a[3].magnitude(), 0.0, 1e-12);
            }
            _ => {}
        }
    }
    assert_eq!(transformer_count, 3);
    assert_eq!(hta_line_count, 3);

    let earth_sum = result
        .groundings
        .iter()
        .fold(Phasor { re: 0.0, im: 0.0 }, |sum, grounding| Phasor {
            re: sum.re + grounding.current_to_earth_a.re,
            im: sum.im + grounding.current_to_earth_a.im,
        });
    near(earth_sum.magnitude(), 0.0, 1e-10);
    assert_eq!(
        result
            .groundings
            .iter()
            .filter(|grounding| grounding.current_to_earth_a.magnitude() > 1e-6)
            .count(),
        2
    );

    let minimum_bt_voltage = result
        .buses
        .iter()
        .filter(|bus| bus.conductors.len() == 4)
        .flat_map(|bus| bus.phase_neutral.iter().map(|voltage| voltage.pu))
        .fold(f64::INFINITY, f64::min);
    assert!(minimum_bt_voltage > 0.97);
}

#[test]
fn rural_loop_is_detected_solved_and_reduces_line_losses() {
    let radial = reference_cases::rural_unbalanced();
    let meshed = reference_cases::rural_meshed();

    let topology = topology::analyze(&meshed).unwrap();
    assert!(!topology.radial);
    assert_eq!(topology.components.len(), 1);
    assert_eq!(topology.cycle_closures.len(), 1);
    assert!(topology.unenergized_buses.is_empty());

    let radial_result = unbalanced::solve(&radial, &Options::default()).unwrap();
    let meshed_result = unbalanced::solve(&meshed, &Options::default()).unwrap();
    assert_eq!(meshed_result.buses.len(), 8);
    assert_eq!(meshed_result.branches.len(), 8);
    assert_eq!(meshed_result.net_demand_p_w, radial_result.net_demand_p_w);
    assert_eq!(
        meshed_result.net_demand_q_var,
        radial_result.net_demand_q_var
    );
    assert!(meshed_result.line_loss_w < radial_result.line_loss_w);
    assert!(meshed_result.balance_error_va < 1e-3);
    assert!(meshed_result.assumptions[0].contains("faiblement maillé"));
}
