//! Read-only operating-topology diagnostics shared by the CLI and future study cases.
use crate::model::{Equipment, Id, Network, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentReport {
    pub buses: Vec<Id>,
    pub sources: Vec<Id>,
    pub energized: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TopologyReport {
    pub radial: bool,
    pub active_sources: Vec<Id>,
    pub energized_buses: Vec<Id>,
    pub unenergized_buses: Vec<Id>,
    /// Active branches that close a cycle in a deterministic spanning forest.
    pub cycle_closures: Vec<Id>,
    pub open_switches: Vec<Id>,
    pub closed_switches: Vec<Id>,
    pub components: Vec<ComponentReport>,
}

pub fn analyze(network: &Network) -> Result<TopologyReport> {
    network.validate()?;
    let buses: Vec<_> = network
        .objects
        .values()
        .filter(|object| object.in_service && matches!(object.equipment, Equipment::Bus { .. }))
        .map(|object| object.id)
        .collect();
    let index: BTreeMap<_, _> = buses.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut adjacency = vec![vec![]; buses.len()];
    let mut open_switches = vec![];
    let mut closed_switches = vec![];
    let mut active_edges = vec![];
    for object in network.objects.values().filter(|object| object.in_service) {
        let endpoints = match object.equipment {
            Equipment::Line { from, to, .. } | Equipment::Transformer { from, to, .. } => {
                Some((from, to))
            }
            Equipment::Switch {
                from, to, closed, ..
            } => {
                if closed {
                    closed_switches.push(object.id);
                    Some((from, to))
                } else {
                    open_switches.push(object.id);
                    None
                }
            }
            _ => None,
        };
        let Some((from, to)) = endpoints else {
            continue;
        };
        let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) else {
            continue;
        };
        let edge = active_edges.len();
        active_edges.push(object.id);
        adjacency[a].push((b, edge));
        adjacency[b].push((a, edge));
    }
    let active_sources: Vec<_> = network
        .objects
        .values()
        .filter(|object| object.in_service && matches!(object.equipment, Equipment::Source { .. }))
        .map(|object| object.id)
        .collect();
    let source_buses: BTreeMap<_, Vec<_>> = network
        .objects
        .values()
        .filter(|object| object.in_service)
        .filter_map(|object| match object.equipment {
            Equipment::Source { bus, .. } if index.contains_key(&bus) => Some((bus, object.id)),
            _ => None,
        })
        .fold(BTreeMap::new(), |mut grouped, (bus, source)| {
            grouped.entry(bus).or_default().push(source);
            grouped
        });
    let mut seen = vec![false; buses.len()];
    let mut edge_seen = vec![false; active_edges.len()];
    let mut cycle_closures = vec![];
    let mut components = vec![];
    for start in 0..buses.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut queue = VecDeque::from([start]);
        let mut component_buses = vec![];
        let mut component_sources = vec![];
        while let Some(node) = queue.pop_front() {
            component_buses.push(buses[node]);
            if let Some(sources) = source_buses.get(&buses[node]) {
                component_sources.extend(sources);
            }
            for &(next, edge) in &adjacency[node] {
                if edge_seen[edge] {
                    continue;
                }
                edge_seen[edge] = true;
                if seen[next] {
                    cycle_closures.push(active_edges[edge]);
                } else {
                    seen[next] = true;
                    queue.push_back(next);
                }
            }
        }
        component_buses.sort();
        component_sources.sort();
        components.push(ComponentReport {
            energized: !component_sources.is_empty(),
            buses: component_buses,
            sources: component_sources,
        });
    }
    let energized: BTreeSet<_> = components
        .iter()
        .filter(|component| component.energized)
        .flat_map(|component| component.buses.iter().copied())
        .collect();
    let mut energized_buses: Vec<_> = energized.iter().copied().collect();
    let mut unenergized_buses: Vec<_> = buses
        .iter()
        .filter(|bus| !energized.contains(bus))
        .copied()
        .collect();
    energized_buses.sort();
    unenergized_buses.sort();
    cycle_closures.sort();
    open_switches.sort();
    closed_switches.sort();
    Ok(TopologyReport {
        radial: cycle_closures.is_empty(),
        active_sources,
        energized_buses,
        unenergized_buses,
        cycle_closures,
        open_switches,
        closed_switches,
        components,
    })
}
