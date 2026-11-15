//! Four-conductor radial AC load flow in physical units, with an explicit neutral.
use crate::{
    complex::C,
    electrical::{Conductor, ElectricalData, Grounding, InjectionConnection},
    load_flow::{CalculationError, Result},
    model::{Equipment, Id, Network},
    project::Project,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::atomic::{AtomicBool, Ordering},
};
mod grounding;
use grounding::GroundSystem;
const CONDUCTORS: [Conductor; 4] = [Conductor::A, Conductor::B, Conductor::C, Conductor::N];
pub const SOLVER_VERSION: &str = "radial-four-wire-0.2";
type Vector = [C; 4];
type Matrix = [[C; 4]; 4];
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub max_iterations: usize,
    pub tolerance_pu: f64,
    pub min_voltage_pu: f64,
    pub max_voltage_pu: f64,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            max_iterations: 200,
            tolerance_pu: 1e-9,
            min_voltage_pu: 0.9,
            max_voltage_pu: 1.1,
        }
    }
}
impl Options {
    fn validate(&self) -> Result<()> {
        if self.max_iterations == 0
            || self.max_iterations > 100000
            || !self.tolerance_pu.is_finite()
            || self.tolerance_pu <= 0.0
            || self.tolerance_pu > 1e-3
            || !self.min_voltage_pu.is_finite()
            || self.min_voltage_pu <= 0.0
            || !self.max_voltage_pu.is_finite()
            || self.max_voltage_pu <= self.min_voltage_pu
        {
            return Err(CalculationError::InvalidOptions);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Phasor {
    pub re: f64,
    pub im: f64,
}
impl Phasor {
    pub fn magnitude(&self) -> f64 {
        self.re.hypot(self.im)
    }
}
impl From<C> for Phasor {
    fn from(c: C) -> Self {
        Self { re: c.re, im: c.im }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoltageResult {
    pub magnitude_v: f64,
    pub angle_deg: f64,
    pub pu: f64,
    pub outside_voltage_limits: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusResult {
    pub id: Id,
    /// Conductor-to-ground phasors, always ordered A B C N.
    pub conductor_voltages_v: [Phasor; 4],
    /// A-N, B-N, C-N, relative to the source phase A reference.
    pub phase_neutral: [VoltageResult; 3],
    /// A-B, B-C, C-A magnitudes.
    pub phase_phase_v: [f64; 3],
    pub neutral_displacement_v: f64,
    pub negative_sequence_percent: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BranchResult {
    pub id: Id,
    pub from_bus: Id,
    pub to_bus: Id,
    /// Oriented from the source downstream; order A B C N.
    pub currents_a: [Phasor; 4],
    pub p_from_w: f64,
    pub q_from_var: f64,
    pub p_to_w: f64,
    pub q_to_var: f64,
    pub loss_w: f64,
    pub loss_var: f64,
    pub loading_percent: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroundingResult {
    pub bus_id: Id,
    /// Positive from the neutral towards the common earth reference.
    pub current_to_earth_a: Phasor,
    pub voltage_v: Phasor,
    pub loss_w: f64,
    pub loss_var: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoadFlow {
    pub iterations: usize,
    pub residual_pu: f64,
    pub source_p_w: f64,
    pub source_q_var: f64,
    pub net_demand_p_w: f64,
    pub net_demand_q_var: f64,
    pub total_loss_w: f64,
    pub total_loss_var: f64,
    pub balance_error_va: f64,
    pub line_loss_w: f64,
    pub line_loss_var: f64,
    pub grounding_loss_w: f64,
    pub grounding_loss_var: f64,
    pub groundings: Vec<GroundingResult>,
    pub buses: Vec<BusResult>,
    pub branches: Vec<BranchResult>,
    pub assumptions: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StudyResult {
    pub project_id: Id,
    pub network_version: Id,
    pub solver_version: String,
    pub options: Options,
    pub result: LoadFlow,
}
fn unsupported(message: impl Into<String>) -> CalculationError {
    CalculationError::Unsupported(message.into())
}
fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(CalculationError::Cancelled)
    } else {
        Ok(())
    }
}
fn full_conductors(values: &[Conductor]) -> bool {
    values.len() == 4 && CONDUCTORS.iter().all(|c| values.contains(c))
}
fn grounded(g: &Grounding) -> bool {
    matches!(g,Grounding::Impedance {ohm} if ohm.r==0.0 && ohm.x==0.0)
}
fn phase_index(phase: Conductor) -> usize {
    match phase {
        Conductor::A => 0,
        Conductor::B => 1,
        Conductor::C => 2,
        Conductor::N => 3,
    }
}
struct Edge {
    id: Id,
    a: usize,
    b: usize,
    z: Matrix,
    limit: Option<f64>,
}
struct Injection {
    bus: usize,
    power: C,
    connection: InjectionConnection,
}
struct Circuit {
    buses: Vec<Id>,
    nominal: Vec<f64>,
    edges: Vec<Edge>,
    injections: Vec<Injection>,
    order: Vec<usize>,
    parent: Vec<Option<(usize, usize)>>,
    source: Vector,
    earth: GroundSystem,
}
impl Circuit {
    fn build(network: &Network, cancel: &AtomicBool) -> Result<Self> {
        network.validate()?;
        check_cancel(cancel)?;
        let buses: Vec<_> = network
            .objects
            .values()
            .filter(|o| o.in_service && matches!(o.equipment, Equipment::Bus { .. }))
            .map(|o| o.id)
            .collect();
        let index: BTreeMap<_, _> = buses.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let nominal: Vec<_> = buses
            .iter()
            .map(|id| match network.objects[id].equipment {
                Equipment::Bus { voltage_v } => voltage_v / 3.0_f64.sqrt(),
                _ => unreachable!(),
            })
            .collect();
        let sources: Vec<_> = network
            .objects
            .values()
            .filter(|o| o.in_service)
            .filter_map(|o| {
                if let Equipment::Source { bus, voltage_pu } = o.equipment {
                    index.get(&bus).map(|i| (*i, voltage_pu, o))
                } else {
                    None
                }
            })
            .collect();
        if sources.len() != 1 {
            return Err(unsupported(format!(
                "le calcul ABCN exige une source active unique ; {} trouvées",
                sources.len()
            )));
        }
        let (root, voltage_pu, source_object) = sources[0];
        let Some(ElectricalData::Source(data)) = &source_object.electrical else {
            return Err(unsupported(format!(
                "{} : données détaillées de source requises",
                source_object.name
            )));
        };
        if !grounded(&data.neutral_grounding) {
            return Err(unsupported(
                "ce jalon ABCN exige une source idéale avec neutre directement à la terre",
            ));
        }
        let mut earth = vec![];
        for (i, id) in buses.iter().enumerate() {
            let o = &network.objects[id];
            let Some(ElectricalData::Bus(data)) = &o.electrical else {
                return Err(unsupported(format!(
                    "{} : déclaration des conducteurs et du neutre requise",
                    o.name
                )));
            };
            if !full_conductors(&data.conductors) {
                return Err(unsupported(format!(
                    "{} : ce moteur exige A/B/C/N sur chaque bus",
                    o.name
                )));
            }
            if i == root {
                if !grounded(&data.neutral_grounding) {
                    return Err(unsupported(format!(
                        "{} : le neutre source doit rester directement à la terre",
                        o.name
                    )));
                }
            } else {
                match &data.neutral_grounding {
                    Grounding::Isolated => {}
                    Grounding::Impedance { ohm } => earth.push((i, C::new(ohm.r, ohm.x))),
                    _ => {
                        return Err(unsupported(format!(
                            "{} : préciser l'impédance de terre du neutre ou déclarer l'absence de prise de terre locale",
                            o.name
                        )));
                    }
                }
            }
            if (nominal[i] / nominal[root] - 1.0).abs() > 1e-10 {
                return Err(unsupported(
                    "le premier moteur ABCN couvre un seul niveau de tension, en aval d'un poste",
                ));
            }
        }
        let mut edges = vec![];
        let mut injections = vec![];
        for o in network.objects.values().filter(|o| o.in_service) {
            check_cancel(cancel)?;
            match o.equipment {
                Equipment::Line {
                    from,
                    to,
                    length_m,
                    max_current_a,
                    ..
                } => {
                    let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) else {
                        continue;
                    };
                    let Some(ElectricalData::Line(data)) = &o.electrical else {
                        return Err(unsupported(format!(
                            "{} : matrice primitive ABCN requise, aucune reconstitution depuis R/X simplifiés",
                            o.name
                        )));
                    };
                    if !full_conductors(&data.conductors) {
                        return Err(unsupported(format!(
                            "{} : quatre conducteurs A/B/C/N requis",
                            o.name
                        )));
                    }
                    if data
                        .shunt_siemens_per_km
                        .as_ref()
                        .is_some_and(|m| m.iter().flatten().any(|y| y.g != 0.0 || y.b != 0.0))
                    {
                        return Err(unsupported(format!(
                            "{} : shunts non nuls non encore supportés",
                            o.name
                        )));
                    }
                    let mut z = [[C::default(); 4]; 4];
                    for (row, c) in CONDUCTORS.iter().enumerate() {
                        for (col, d) in CONDUCTORS.iter().enumerate() {
                            let i = data.conductors.iter().position(|v| v == c).unwrap();
                            let j = data.conductors.iter().position(|v| v == d).unwrap();
                            let value = data.series_ohm_per_km[i][j];
                            z[row][col] = C::new(value.r, value.x) * (length_m / 1000.0);
                        }
                    }
                    if z.iter().flatten().any(|v| !v.finite()) {
                        return Err(CalculationError::Numerical(buses[a]));
                    }
                    edges.push(Edge {
                        id: o.id,
                        a,
                        b,
                        z,
                        limit: Some(max_current_a),
                    });
                }
                Equipment::Switch {
                    from,
                    to,
                    closed: true,
                } => {
                    let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) else {
                        continue;
                    };
                    if !matches!(&o.electrical,Some(ElectricalData::Switch {conductors}) if full_conductors(conductors))
                    {
                        return Err(unsupported(format!(
                            "{} : interrupteur ABCN explicite requis",
                            o.name
                        )));
                    }
                    edges.push(Edge {
                        id: o.id,
                        a,
                        b,
                        z: [[C::default(); 4]; 4],
                        limit: None,
                    });
                }
                Equipment::Transformer { from, to, .. }
                    if index.contains_key(&from) && index.contains_key(&to) =>
                {
                    return Err(unsupported(format!(
                        "{} : transformateurs non encore couverts par le moteur déséquilibré ; définir le réseau BT depuis son équivalent source secondaire",
                        o.name
                    )));
                }
                Equipment::Load {
                    bus, p_w, q_var, ..
                }
                | Equipment::Generator {
                    bus, p_w, q_var, ..
                } => {
                    let Some(&bus) = index.get(&bus) else {
                        continue;
                    };
                    let Some(ElectricalData::Injection(data)) = &o.electrical else {
                        return Err(unsupported(format!(
                            "{} : connexion électrique détaillée requise",
                            o.name
                        )));
                    };
                    if matches!(
                        data.connection,
                        InjectionConnection::ThreePhaseStar { neutral: false }
                    ) {
                        return Err(unsupported(format!(
                            "{} : point étoile flottant non encore couvert",
                            o.name
                        )));
                    }
                    let sign = if matches!(o.equipment, Equipment::Load { .. }) {
                        1.0
                    } else {
                        -1.0
                    };
                    injections.push(Injection {
                        bus,
                        power: C::new(p_w * sign, q_var * sign),
                        connection: data.connection.clone(),
                    });
                }
                _ => {}
            }
        }
        let mut adjacency = vec![vec![]; buses.len()];
        for (e, edge) in edges.iter().enumerate() {
            adjacency[edge.a].push((edge.b, e));
            adjacency[edge.b].push((edge.a, e));
        }
        let mut order = vec![];
        let mut parent = vec![None; buses.len()];
        let mut seen = vec![false; buses.len()];
        seen[root] = true;
        let mut queue = VecDeque::from([root]);
        while let Some(a) = queue.pop_front() {
            check_cancel(cancel)?;
            order.push(a);
            for &(b, e) in &adjacency[a] {
                if parent[a].is_some_and(|(_, p)| p == e) {
                    continue;
                }
                if seen[b] {
                    return Err(unsupported(format!(
                        "boucle ou branches parallèles : {}",
                        network.objects[&edges[e].id].name
                    )));
                }
                seen[b] = true;
                parent[b] = Some((a, e));
                queue.push_back(b);
            }
        }
        if order.len() != buses.len() {
            return Err(unsupported(
                "jeux de barres en service non alimentés : le réseau ABCN doit être radial et connecté",
            ));
        }
        let magnitude = nominal[root] * voltage_pu;
        let source = std::array::from_fn(|i| {
            if i == 3 {
                C::default()
            } else {
                let angle = [0.0, -120.0_f64, 120.0][i].to_radians();
                C::new(magnitude * angle.cos(), magnitude * angle.sin())
            }
        });
        if source.iter().any(|v| !v.finite()) {
            return Err(CalculationError::Numerical(buses[root]));
        }
        let earth = GroundSystem::build(earth, &edges, &parent, cancel)?;
        Ok(Self {
            earth,
            buses,
            nominal,
            edges,
            injections,
            order,
            parent,
            source,
        })
    }
    fn currents(&self, voltage: &[Vector], cancel: &AtomicBool) -> Result<Vec<Vector>> {
        let mut currents = vec![[C::default(); 4]; self.buses.len()];
        for injection in &self.injections {
            check_cancel(cancel)?;
            let bus = injection.bus;
            let mut pair = |a: usize, b: usize, power: C| -> Result<()> {
                let v = voltage[bus][a] - voltage[bus][b];
                if !v.finite() || v.abs() < self.nominal[bus] * 1e-8 {
                    return Err(CalculationError::Numerical(self.buses[bus]));
                }
                let current = (power / v).conj();
                if !current.finite() {
                    return Err(CalculationError::Numerical(self.buses[bus]));
                }
                currents[bus][a] = currents[bus][a] + current;
                currents[bus][b] = currents[bus][b] - current;
                Ok(())
            };
            match injection.connection {
                InjectionConnection::PhaseNeutral { phase } => {
                    pair(phase_index(phase), 3, injection.power)?
                }
                InjectionConnection::PhasePhase { from, to } => {
                    pair(phase_index(from), phase_index(to), injection.power)?
                }
                InjectionConnection::ThreePhaseStar { neutral: true } => {
                    for phase in 0..3 {
                        pair(phase, 3, injection.power * (1.0 / 3.0))?;
                    }
                }
                InjectionConnection::ThreePhaseDelta => {
                    for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                        pair(a, b, injection.power * (1.0 / 3.0))?;
                    }
                }
                _ => unreachable!("validated supported connection"),
            }
        }
        for &i in self.order.iter().rev() {
            check_cancel(cancel)?;
            if let Some((p, _)) = self.parent[i] {
                let child = currents[i];
                for (target, value) in currents[p].iter_mut().zip(child) {
                    *target = *target + value;
                }
            }
        }
        if currents
            .iter()
            .flatten()
            .any(|v| !v.finite() || !v.abs().is_finite())
        {
            return Err(CalculationError::Numerical(self.buses[self.order[0]]));
        }
        Ok(currents)
    }
    fn linear_step(
        &self,
        mut currents: Vec<Vector>,
        cancel: &AtomicBool,
    ) -> Result<(Vec<Vector>, Vec<Vector>, Vec<C>)> {
        let base_voltage = self.sweep(&currents, cancel)?;
        let rhs: Vec<_> = self
            .earth
            .nodes
            .iter()
            .map(|i| base_voltage[*i][3])
            .collect();
        let earth_currents = self.earth.currents(&rhs, cancel)?;
        for (&node, &current) in self.earth.nodes.iter().zip(&earth_currents) {
            let mut cursor = node;
            loop {
                check_cancel(cancel)?;
                currents[cursor][3] = currents[cursor][3] + current;
                match self.parent[cursor] {
                    Some((parent, _)) => cursor = parent,
                    None => break,
                }
            }
        }
        if currents
            .iter()
            .flatten()
            .any(|i| !i.finite() || !i.abs().is_finite())
        {
            return Err(CalculationError::Numerical(self.buses[self.order[0]]));
        }
        let voltage = self.sweep(&currents, cancel)?;
        for ((&node, &z), &current) in self
            .earth
            .nodes
            .iter()
            .zip(&self.earth.impedances)
            .zip(&earth_currents)
        {
            let residual = (voltage[node][3] - z * current).abs();
            if !residual.is_finite() || residual > 1e-9 * self.nominal[node] {
                return Err(CalculationError::Numerical(self.buses[node]));
            }
        }
        Ok((voltage, currents, earth_currents))
    }
    fn sweep(&self, currents: &[Vector], cancel: &AtomicBool) -> Result<Vec<Vector>> {
        let mut voltage = vec![self.source; self.buses.len()];
        for &i in &self.order {
            check_cancel(cancel)?;
            if let Some((p, e)) = self.parent[i] {
                let upstream = voltage[p];
                for (k, value) in voltage[i].iter_mut().enumerate() {
                    let drop = (0..4).fold(C::default(), |sum, j| {
                        sum + self.edges[e].z[k][j] * currents[i][j]
                    });
                    *value = upstream[k] - drop;
                }
                if voltage[i].iter().any(|v| !v.finite()) {
                    return Err(CalculationError::Numerical(self.buses[i]));
                }
            }
        }
        Ok(voltage)
    }
}
fn power(voltage: Vector, current: Vector) -> C {
    (0..4).fold(C::default(), |sum, k| sum + voltage[k] * current[k].conj())
}
pub fn study(project: &Project, options: &Options, cancel: &AtomicBool) -> Result<StudyResult> {
    check_cancel(cancel)?;
    let result = solve_cancellable(&project.network()?, options, cancel)?;
    Ok(StudyResult {
        project_id: project.id,
        network_version: project.head(),
        solver_version: SOLVER_VERSION.into(),
        options: options.clone(),
        result,
    })
}
pub fn solve(network: &Network, options: &Options) -> Result<LoadFlow> {
    solve_cancellable(network, options, &AtomicBool::new(false))
}
pub fn solve_cancellable(
    network: &Network,
    options: &Options,
    cancel: &AtomicBool,
) -> Result<LoadFlow> {
    options.validate()?;
    let circuit = Circuit::build(network, cancel)?;
    let mut voltage = vec![circuit.source; circuit.buses.len()];
    let mut residual = f64::INFINITY;
    for iteration in 1..=options.max_iterations {
        check_cancel(cancel)?;
        (voltage, _, _) = circuit.linear_step(circuit.currents(&voltage, cancel)?, cancel)?;
        let (corrected, currents, earth_currents) =
            circuit.linear_step(circuit.currents(&voltage, cancel)?, cancel)?;
        residual = voltage
            .iter()
            .zip(&corrected)
            .enumerate()
            .flat_map(|(i, (a, b))| {
                let base = circuit.nominal[i];
                (0..4).map(move |k| (a[k] - b[k]).abs() / base)
            })
            .fold(0.0, f64::max);
        if !residual.is_finite() {
            return Err(CalculationError::Numerical(circuit.buses[circuit.order[0]]));
        }
        if residual > options.tolerance_pu {
            continue;
        }
        // Publish the last linear state: Ohm's law and earth boundary conditions hold together.
        voltage = corrected;
        let mut buses = vec![];
        let mut branches = vec![];
        let mut loss = C::default();
        for (i, &id) in circuit.buses.iter().enumerate() {
            check_cancel(cancel)?;
            let pn: [C; 3] = std::array::from_fn(|k| voltage[i][k] - voltage[i][3]);
            let phase_neutral = std::array::from_fn(|k| {
                let magnitude = pn[k].abs();
                let pu = magnitude / circuit.nominal[i];
                VoltageResult {
                    magnitude_v: magnitude,
                    angle_deg: pn[k].im.atan2(pn[k].re).to_degrees(),
                    pu,
                    outside_voltage_limits: pu < options.min_voltage_pu
                        || pu > options.max_voltage_pu,
                }
            });
            let phase_phase_v =
                std::array::from_fn(|k| (voltage[i][k] - voltage[i][(k + 1) % 3]).abs());
            let a = C::new(-0.5, 3.0_f64.sqrt() / 2.0);
            let a2 = a * a;
            let positive = (pn[0] + a * pn[1] + a2 * pn[2]) * (1.0 / 3.0);
            let negative = (pn[0] + a2 * pn[1] + a * pn[2]) * (1.0 / 3.0);
            if positive.abs() < circuit.nominal[i] * 1e-8 {
                return Err(CalculationError::Numerical(id));
            }
            let unbalance = negative.abs() / positive.abs() * 100.0;
            if !unbalance.is_finite()
                || phase_phase_v.iter().any(|v| !v.is_finite())
                || phase_neutral
                    .iter()
                    .any(|v| !v.magnitude_v.is_finite() || !v.pu.is_finite())
            {
                return Err(CalculationError::Numerical(id));
            }
            buses.push(BusResult {
                id,
                conductor_voltages_v: voltage[i].map(Into::into),
                phase_neutral,
                phase_phase_v,
                neutral_displacement_v: voltage[i][3].abs(),
                negative_sequence_percent: unbalance,
            });
            if let Some((p, e)) = circuit.parent[i] {
                let edge = &circuit.edges[e];
                let from = power(voltage[p], currents[i]);
                let to = power(voltage[i], currents[i]);
                let drop: Vector = std::array::from_fn(|k| {
                    (0..4).fold(C::default(), |sum, j| sum + edge.z[k][j] * currents[i][j])
                });
                let branch_loss = power(drop, currents[i]);
                loss = loss + branch_loss;
                let loading = edge.limit.map(|limit| {
                    currents[i].iter().map(|v| v.abs()).fold(0.0, f64::max) / limit * 100.0
                });
                if !from.finite()
                    || !to.finite()
                    || !branch_loss.finite()
                    || loading.is_some_and(|v| !v.is_finite())
                {
                    return Err(CalculationError::Numerical(id));
                }
                branches.push(BranchResult {
                    id: edge.id,
                    from_bus: circuit.buses[p],
                    to_bus: id,
                    currents_a: currents[i].map(Into::into),
                    p_from_w: from.re,
                    q_from_var: from.im,
                    p_to_w: to.re,
                    q_to_var: to.im,
                    loss_w: branch_loss.re,
                    loss_var: branch_loss.im,
                    loading_percent: loading,
                });
            }
        }
        let line_loss = loss;
        let mut grounding_loss = C::default();
        let mut earth_sum = C::default();
        let mut groundings = vec![];
        for ((&node, &z), &current) in circuit
            .earth
            .nodes
            .iter()
            .zip(&circuit.earth.impedances)
            .zip(&earth_currents)
        {
            check_cancel(cancel)?;
            let ground_loss = z * current.abs().powi(2);
            grounding_loss = grounding_loss + ground_loss;
            earth_sum = earth_sum + current;
            if !ground_loss.finite() {
                return Err(CalculationError::Numerical(circuit.buses[node]));
            }
            groundings.push(GroundingResult {
                bus_id: circuit.buses[node],
                current_to_earth_a: current.into(),
                voltage_v: voltage[node][3].into(),
                loss_w: ground_loss.re,
                loss_var: ground_loss.im,
            });
        }
        if !earth_sum.finite() || !earth_sum.abs().is_finite() {
            return Err(CalculationError::Numerical(circuit.buses[circuit.order[0]]));
        }
        groundings.push(GroundingResult {
            bus_id: circuit.buses[circuit.order[0]],
            current_to_earth_a: (earth_sum * -1.0).into(),
            voltage_v: C::default().into(),
            loss_w: 0.0,
            loss_var: 0.0,
        });
        loss = loss + grounding_loss;
        let source = power(circuit.source, currents[circuit.order[0]]);
        let net = circuit
            .injections
            .iter()
            .fold(C::default(), |sum, i| sum + i.power);
        let balance = (source - net - loss).abs();
        if !source.finite() || !net.finite() || !loss.finite() || !balance.is_finite() {
            return Err(CalculationError::Numerical(circuit.buses[circuit.order[0]]));
        }
        check_cancel(cancel)?;
        return Ok(LoadFlow {
            iterations:iteration,residual_pu:residual,source_p_w:source.re,source_q_var:source.im,
            net_demand_p_w:net.re,net_demand_q_var:net.im,total_loss_w:loss.re,total_loss_var:loss.im,
            balance_error_va:balance,line_loss_w:line_loss.re,line_loss_var:line_loss.im,
            grounding_loss_w:grounding_loss.re,grounding_loss_var:grounding_loss.im,groundings,buses,branches,
            assumptions:vec![
                "Réseau radial ABCN, matrice primitive complète, source de tension équilibrée idéale avec neutre fixé à la terre.".into(),
                "Prises de terre du neutre résolues vers une référence de terre équipotentielle commune ; couplages entre électrodes et potentiels locaux du sol non modélisés.".into(),
                "Aucun shunt de ligne, transformateur ou point étoile flottant. Impédances de source de défaut non utilisées en régime permanent.".into(),
                "Taux de charge : limite de courant renseignée appliquée uniformément aux quatre conducteurs ; aucun calcul thermique ni harmonique.".into(),
            ],
        });
    }
    Err(CalculationError::NonConvergent {
        iterations: options.max_iterations,
        residual_pu: residual,
    })
}
