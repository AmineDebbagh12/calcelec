//! Four-conductor radial and weakly meshed AC load flow in physical units,
//! with an explicit neutral.
use crate::{
    complex::C,
    electrical::{
        Conductor, ElectricalData, Grounding, InjectionConnection, TransformerData,
        WindingConnection,
    },
    load_flow::{CalculationError, Result},
    model::{Equipment, Id, Network},
    project::Project,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::atomic::{AtomicBool, Ordering},
};
mod grounding;
use grounding::{ComplexLu, GroundSystem};
const CONDUCTORS: [Conductor; 4] = [Conductor::A, Conductor::B, Conductor::C, Conductor::N];
pub const SOLVER_VERSION: &str = "meshed-four-wire-0.7";
type Vector = [C; 4];
type Matrix = [[C; 4]; 4];
type LinearStep = (Vec<Vector>, Vec<Vector>, Vec<Vector>, Vec<C>);
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
    pub conductors: Vec<Conductor>,
    /// Conductor-to-ground phasors, always ordered A B C N.
    pub conductor_voltages_v: [Phasor; 4],
    /// A-N, B-N, C-N; for an ABC bus, phase-to-earth with no N conductor.
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
    /// Tree branches are oriented from the source downstream; cycle closures use
    /// the object's stored `from`/`to` direction. Order A B C N.
    pub currents_a: [Phasor; 4],
    /// Current entering the branch on `from_bus`; order A B C N.
    pub currents_from_a: [Phasor; 4],
    /// Current leaving the branch on `to_bus`; order A B C N.
    pub currents_to_a: [Phasor; 4],
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
    pub transformer_loss_w: f64,
    pub transformer_loss_var: f64,
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
fn phase_conductors(values: &[Conductor]) -> bool {
    values.len() == 3 && CONDUCTORS[..3].iter().all(|c| values.contains(c))
}
fn same_conductors(a: &[Conductor], b: &[Conductor]) -> bool {
    a.len() == b.len() && a.iter().all(|c| b.contains(c))
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
#[derive(Clone, Copy)]
enum EdgeModel {
    Line(Matrix),
    GroundedSecondary {
        ratio: f64,
        shift: C,
        z_positive_to_ohm: C,
        z_zero_to_ohm: C,
    },
}
struct Edge {
    id: Id,
    a: usize,
    b: usize,
    model: EdgeModel,
    current_limits: Option<(f64, f64)>,
}
impl Edge {
    fn upstream_current(&self, downstream: Vector) -> Vector {
        match self.model {
            EdgeModel::Line(_) => downstream,
            EdgeModel::GroundedSecondary { ratio, shift, .. } => {
                let sequence = phase_to_sequence(downstream);
                let upstream_sequence = [
                    C::default(),
                    sequence[1] * shift.conj() * (1.0 / ratio),
                    sequence[2] * shift * (1.0 / ratio),
                ];
                let mut result = sequence_to_phase(upstream_sequence);
                result[3] = C::default();
                result
            }
        }
    }
    fn downstream_voltage(&self, upstream: Vector, downstream_current: Vector) -> Vector {
        match self.model {
            EdgeModel::Line(z) => std::array::from_fn(|k| {
                upstream[k]
                    - (0..4).fold(C::default(), |sum, j| sum + z[k][j] * downstream_current[j])
            }),
            EdgeModel::GroundedSecondary {
                ratio,
                shift,
                z_positive_to_ohm,
                z_zero_to_ohm,
            } => {
                let voltage_sequence = phase_to_sequence(upstream);
                let current_sequence = phase_to_sequence(downstream_current);
                let downstream_sequence = [
                    C::default() - z_zero_to_ohm * current_sequence[0],
                    voltage_sequence[1] * shift * (1.0 / ratio)
                        - z_positive_to_ohm * current_sequence[1],
                    voltage_sequence[2] * shift.conj() * (1.0 / ratio)
                        - z_positive_to_ohm * current_sequence[2],
                ];
                let mut result = sequence_to_phase(downstream_sequence);
                result[3] = C::default();
                result
            }
        }
    }
    fn neutral_impedance(&self) -> Option<C> {
        match self.model {
            EdgeModel::Line(z) => Some(z[3][3]),
            EdgeModel::GroundedSecondary { .. } => None,
        }
    }
    fn transformer(&self) -> bool {
        matches!(self.model, EdgeModel::GroundedSecondary { .. })
    }
    fn loading(&self, upstream: Vector, downstream: Vector) -> Option<f64> {
        self.current_limits.map(|(from, to)| {
            let count = if self.transformer() { 3 } else { 4 };
            let upstream_max = upstream[..count]
                .iter()
                .map(|i| i.abs())
                .fold(0.0, f64::max)
                / from;
            let downstream_max = downstream[..count]
                .iter()
                .map(|i| i.abs())
                .fold(0.0, f64::max)
                / to;
            upstream_max.max(downstream_max) * 100.0
        })
    }
}
fn phase_to_sequence(value: Vector) -> [C; 3] {
    let a = C::new(-0.5, 3.0_f64.sqrt() / 2.0);
    let a2 = a * a;
    [
        (value[0] + value[1] + value[2]) * (1.0 / 3.0),
        (value[0] + a * value[1] + a2 * value[2]) * (1.0 / 3.0),
        (value[0] + a2 * value[1] + a * value[2]) * (1.0 / 3.0),
    ]
}
fn sequence_to_phase(value: [C; 3]) -> Vector {
    let a = C::new(-0.5, 3.0_f64.sqrt() / 2.0);
    let a2 = a * a;
    [
        value[0] + value[1] + value[2],
        value[0] + a2 * value[1] + a * value[2],
        value[0] + a * value[1] + a2 * value[2],
        C::default(),
    ]
}
fn grounded_secondary_transformer_model(
    name: &str,
    data: &TransformerData,
    rated_va: f64,
    uk_percent: f64,
) -> Result<EdgeModel> {
    let dyn_group = data.from.connection == WindingConnection::Delta
        && data.to.connection == WindingConnection::Star;
    let yzn_group = data.from.connection == WindingConnection::Star
        && !data.from.neutral_terminal
        && data.from.grounding == Grounding::Isolated
        && data.to.connection == WindingConnection::Zigzag;
    if (!dyn_group && !yzn_group) || !data.to.neutral_terminal || !grounded(&data.to.grounding) {
        return Err(unsupported(format!(
            "{name} : ce jalon couvre les transformateurs Dyn et Yzn, avec neutre BT exposé et directement mis à la terre ; le primaire Y de Yzn doit rester isolé sans borne N"
        )));
    }
    if data.iron_loss_w != Some(0.0) || data.no_load_current_percent != Some(0.0) {
        return Err(unsupported(format!(
            "{name} : pertes fer et courant à vide doivent être explicitement nuls dans ce jalon"
        )));
    }
    let Some(zero) = data.zero_sequence_ohm_referred_from else {
        return Err(unsupported(format!(
            "{name} : impédance homopolaire ramenée au côté HTA requise"
        )));
    };
    let ratio = data.ratio();
    let series = data.series_pu(rated_va, uk_percent)?;
    let base_to = data.to.rated_voltage_ll_v.powi(2) / rated_va;
    let angle = data.phase_shift_deg().to_radians();
    Ok(EdgeModel::GroundedSecondary {
        ratio,
        shift: C::new(angle.cos(), angle.sin()),
        z_positive_to_ohm: C::new(series.r * base_to, series.x * base_to),
        z_zero_to_ohm: C::new(zero.r, zero.x) * (1.0 / ratio.powi(2)),
    })
}
struct Injection {
    bus: usize,
    power: C,
    connection: InjectionConnection,
}
struct Circuit {
    buses: Vec<Id>,
    conductors: Vec<Vec<Conductor>>,
    nominal: Vec<f64>,
    edges: Vec<Edge>,
    injections: Vec<Injection>,
    order: Vec<usize>,
    parent: Vec<Option<(usize, usize)>>,
    chords: Vec<usize>,
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
        let mut bus_data = Vec::with_capacity(buses.len());
        for id in &buses {
            let o = &network.objects[id];
            let Some(ElectricalData::Bus(data)) = &o.electrical else {
                return Err(unsupported(format!(
                    "{} : déclaration des conducteurs et du neutre requise",
                    o.name
                )));
            };
            if !full_conductors(&data.conductors) && !phase_conductors(&data.conductors) {
                return Err(unsupported(format!(
                    "{} : ce moteur exige A/B/C ou A/B/C/N sur chaque bus",
                    o.name
                )));
            }
            bus_data.push(data);
        }
        let source_anchor_impedance = if full_conductors(&bus_data[root].conductors) {
            let (Grounding::Impedance { ohm: bus_ohm }, Grounding::Impedance { ohm: source_ohm }) =
                (&bus_data[root].neutral_grounding, &data.neutral_grounding)
            else {
                return Err(unsupported(
                    "une source raccordée en ABCN exige une impédance de terre renseignée dans les données du bus et de la source",
                ));
            };
            if bus_ohm != source_ohm {
                return Err(unsupported(
                    "la mise à la terre du bus source et celle de la source doivent décrire la même impédance",
                ));
            }
            Some(C::new(source_ohm.r, source_ohm.x))
        } else if phase_conductors(&bus_data[root].conductors)
            && !matches!(data.neutral_grounding, Grounding::NotApplicable)
            && !grounded(&data.neutral_grounding)
        {
            return Err(unsupported(
                "une source raccordée en ABC doit déclarer son neutre non applicable ou directement mis à la terre hors du modèle de régime permanent",
            ));
        } else {
            None
        };
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
                    if !full_conductors(&data.conductors) && !phase_conductors(&data.conductors) {
                        return Err(unsupported(format!(
                            "{} : conducteurs A/B/C ou A/B/C/N requis",
                            o.name
                        )));
                    }
                    if !same_conductors(&data.conductors, &bus_data[a].conductors)
                        || !same_conductors(&data.conductors, &bus_data[b].conductors)
                    {
                        return Err(unsupported(format!(
                            "{} : les conducteurs du tronçon doivent correspondre exactement à ceux de ses deux bus",
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
                    for (i, c) in data.conductors.iter().enumerate() {
                        for (j, d) in data.conductors.iter().enumerate() {
                            let value = data.series_ohm_per_km[i][j];
                            z[phase_index(*c)][phase_index(*d)] =
                                C::new(value.r, value.x) * (length_m / 1000.0);
                        }
                    }
                    if z.iter().flatten().any(|v| !v.finite()) {
                        return Err(CalculationError::Numerical(buses[a]));
                    }
                    edges.push(Edge {
                        id: o.id,
                        a,
                        b,
                        model: EdgeModel::Line(z),
                        current_limits: Some((max_current_a, max_current_a)),
                    });
                }
                Equipment::Switch {
                    from,
                    to,
                    closed: true,
                    ..
                } => {
                    let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) else {
                        continue;
                    };
                    let Some(ElectricalData::Switch { conductors }) = &o.electrical else {
                        return Err(unsupported(format!(
                            "{} : interrupteur ABC ou ABCN explicite requis",
                            o.name
                        )));
                    };
                    if (!full_conductors(conductors) && !phase_conductors(conductors))
                        || !same_conductors(conductors, &bus_data[a].conductors)
                        || !same_conductors(conductors, &bus_data[b].conductors)
                    {
                        return Err(unsupported(format!(
                            "{} : les conducteurs de l'interrupteur doivent correspondre exactement à ceux de ses deux bus",
                            o.name
                        )));
                    }
                    edges.push(Edge {
                        id: o.id,
                        a,
                        b,
                        model: EdgeModel::Line([[C::default(); 4]; 4]),
                        current_limits: None,
                    });
                }
                Equipment::Transformer {
                    from,
                    to,
                    rated_va,
                    uk_percent,
                } if index.contains_key(&from) && index.contains_key(&to) => {
                    let Some(ElectricalData::Transformer(data)) = &o.electrical else {
                        return Err(unsupported(format!(
                            "{} : données détaillées de transformateur requises",
                            o.name
                        )));
                    };
                    let a = index[&from];
                    let b = index[&to];
                    if !phase_conductors(&bus_data[a].conductors)
                        || !full_conductors(&bus_data[b].conductors)
                    {
                        return Err(unsupported(format!(
                            "{} : le côté HTA delta exige un bus ABC et le côté BT étoile un bus ABCN",
                            o.name
                        )));
                    }
                    if (data.from.rated_voltage_ll_v / (nominal[a] * 3.0_f64.sqrt()) - 1.0).abs()
                        > 1e-10
                        || (data.to.rated_voltage_ll_v / (nominal[b] * 3.0_f64.sqrt()) - 1.0).abs()
                            > 1e-10
                    {
                        return Err(unsupported(format!(
                            "{} : les tensions assignées des enroulements doivent correspondre aux tensions nominales des bus",
                            o.name
                        )));
                    }
                    let model =
                        grounded_secondary_transformer_model(&o.name, data, rated_va, uk_percent)?;
                    let from_limit = rated_va / (3.0_f64.sqrt() * data.from.rated_voltage_ll_v);
                    let to_limit = rated_va / (3.0_f64.sqrt() * data.to.rated_voltage_ll_v);
                    edges.push(Edge {
                        id: o.id,
                        a,
                        b,
                        model,
                        current_limits: Some((from_limit, to_limit)),
                    });
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
        let mut edge_seen = vec![false; edges.len()];
        let mut chords = vec![];
        seen[root] = true;
        let mut queue = VecDeque::from([root]);
        while let Some(a) = queue.pop_front() {
            check_cancel(cancel)?;
            order.push(a);
            for &(b, e) in &adjacency[a] {
                if edge_seen[e] {
                    continue;
                }
                edge_seen[e] = true;
                if seen[b] {
                    if edges[e].transformer() {
                        return Err(unsupported(format!(
                            "{} : un transformateur ne peut pas être la branche de fermeture d'une boucle",
                            network.objects[&edges[e].id].name
                        )));
                    }
                    chords.push(e);
                    continue;
                }
                seen[b] = true;
                parent[b] = Some((a, e));
                queue.push_back(b);
            }
        }
        if order.len() != buses.len() {
            return Err(unsupported(
                "jeux de barres en service non alimentés : le réseau ABCN doit être connecté à la source active",
            ));
        }
        let mut transformer_boundaries = BTreeSet::new();
        for (node, parent_edge) in parent.iter().enumerate() {
            let Some((parent_node, edge_index)) = parent_edge else {
                continue;
            };
            let edge = &edges[*edge_index];
            if edge.transformer() {
                if edge.a != *parent_node || edge.b != node {
                    return Err(unsupported(format!(
                        "{} : le transformateur Dyn/Yzn doit être orienté du réseau HTA amont vers le réseau BT aval",
                        network.objects[&edge.id].name
                    )));
                }
                transformer_boundaries.insert(node);
            }
        }
        let mut zones = vec![root; buses.len()];
        for &node in &order {
            if let Some((parent_node, edge)) = parent[node] {
                zones[node] = if edges[edge].transformer() {
                    node
                } else {
                    zones[parent_node]
                };
            }
        }
        for &edge in &chords {
            if zones[edges[edge].a] != zones[edges[edge].b] {
                return Err(unsupported(format!(
                    "{} : une boucle ne peut pas relier deux zones galvaniques séparées par un transformateur",
                    network.objects[&edges[edge].id].name
                )));
            }
        }
        let mesh_variables: usize = chords
            .iter()
            .map(|&edge| bus_data[edges[edge].a].conductors.len())
            .sum();
        if mesh_variables > 64 {
            return Err(unsupported(
                "ce jalon accepte au plus 64 courants de compensation de boucle par calcul",
            ));
        }
        let mut anchor_impedances: BTreeMap<_, _> = transformer_boundaries
            .iter()
            .map(|&node| (node, C::default()))
            .collect();
        if let Some(impedance) = source_anchor_impedance {
            anchor_impedances.insert(root, impedance);
        }
        let mut earth = vec![];
        for (node, data) in bus_data.iter().enumerate() {
            let name = &network.objects[&buses[node]].name;
            if phase_conductors(&data.conductors) {
                if data.neutral_grounding != Grounding::NotApplicable {
                    return Err(unsupported(format!(
                        "{name} : un bus sans conducteur N doit déclarer la mise à la terre non applicable"
                    )));
                }
                continue;
            }
            if node == root {
                continue;
            }
            if transformer_boundaries.contains(&node) {
                if !grounded(&data.neutral_grounding) {
                    return Err(unsupported(format!(
                        "{name} : le neutre au secondaire Dyn/Yzn doit être directement mis à la terre"
                    )));
                }
                continue;
            }
            match &data.neutral_grounding {
                Grounding::Isolated => {}
                Grounding::Impedance { ohm } => earth.push((node, C::new(ohm.r, ohm.x))),
                _ => {
                    return Err(unsupported(format!(
                        "{name} : préciser l'impédance de terre du neutre ou déclarer l'absence de prise de terre locale"
                    )));
                }
            }
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
        let earth = GroundSystem::build(earth, anchor_impedances, &edges, &parent, cancel)?;
        let conductors = bus_data
            .iter()
            .map(|data| data.conductors.clone())
            .collect();
        Ok(Self {
            earth,
            buses,
            conductors,
            nominal,
            edges,
            injections,
            order,
            parent,
            chords,
            source,
        })
    }
    fn injection_currents(&self, voltage: &[Vector], cancel: &AtomicBool) -> Result<Vec<Vector>> {
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
        if currents
            .iter()
            .flatten()
            .any(|v| !v.finite() || !v.abs().is_finite())
        {
            return Err(CalculationError::Numerical(self.buses[self.order[0]]));
        }
        Ok(currents)
    }

    fn radial_step(
        &self,
        nodal_currents: &[Vector],
        cancel: &AtomicBool,
    ) -> Result<(Vec<Vector>, Vec<Vector>, Vec<C>)> {
        let mut currents = nodal_currents.to_vec();
        for &i in self.order.iter().rev() {
            check_cancel(cancel)?;
            if let Some((p, edge)) = self.parent[i] {
                let upstream = self.edges[edge].upstream_current(currents[i]);
                for (target, value) in currents[p].iter_mut().zip(upstream) {
                    *target = *target + value;
                }
            }
        }
        let base_voltage = self.sweep(&currents, C::default(), cancel)?;
        let rhs: Vec<_> = self
            .earth
            .nodes
            .iter()
            .map(|i| base_voltage[*i][3])
            .collect();
        let earth_currents = self.earth.currents(&rhs, cancel)?;
        for ((&node, &anchor), &current) in self
            .earth
            .nodes
            .iter()
            .zip(&self.earth.electrode_anchors)
            .zip(&earth_currents)
        {
            let mut cursor = node;
            loop {
                check_cancel(cancel)?;
                currents[cursor][3] = currents[cursor][3] + current;
                if cursor == anchor {
                    break;
                }
                let Some((parent, _)) = self.parent[cursor] else {
                    return Err(CalculationError::Numerical(self.buses[node]));
                };
                cursor = parent;
            }
        }
        if currents
            .iter()
            .flatten()
            .any(|i| !i.finite() || !i.abs().is_finite())
        {
            return Err(CalculationError::Numerical(self.buses[self.order[0]]));
        }
        let anchor_states = self.earth.anchor_states(&earth_currents);
        let root_neutral = anchor_states
            .get(&self.order[0])
            .map_or(C::default(), |(impedance, current)| *impedance * *current);
        let voltage = self.sweep(&currents, root_neutral, cancel)?;
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
        for (&anchor, &(impedance, current)) in &anchor_states {
            let residual = (voltage[anchor][3] - impedance * current).abs();
            if !residual.is_finite() || residual > 1e-9 * self.nominal[anchor] {
                return Err(CalculationError::Numerical(self.buses[anchor]));
            }
        }
        Ok((voltage, currents, earth_currents))
    }

    fn linear_step(&self, nodal_currents: Vec<Vector>, cancel: &AtomicBool) -> Result<LinearStep> {
        if self.chords.is_empty() {
            let (voltage, currents, earth) = self.radial_step(&nodal_currents, cancel)?;
            return Ok((voltage, currents, vec![], earth));
        }
        let zero = vec![[C::default(); 4]; self.buses.len()];
        let (zero_voltage, _, _) = self.radial_step(&zero, cancel)?;
        let (base_voltage, _, _) = self.radial_step(&nodal_currents, cancel)?;
        let variables: Vec<_> = self
            .chords
            .iter()
            .enumerate()
            .flat_map(|(chord, &edge)| {
                self.conductors[self.edges[edge].a]
                    .iter()
                    .map(move |conductor| (chord, phase_index(*conductor)))
            })
            .collect();
        let mut matrix = vec![vec![C::default(); variables.len()]; variables.len()];
        let mut rhs = vec![C::default(); variables.len()];
        for (row, &(chord, conductor)) in variables.iter().enumerate() {
            let edge = &self.edges[self.chords[chord]];
            rhs[row] = base_voltage[edge.a][conductor] - base_voltage[edge.b][conductor];
        }
        for (column, &(basis_chord, basis_conductor)) in variables.iter().enumerate() {
            check_cancel(cancel)?;
            let basis_edge = &self.edges[self.chords[basis_chord]];
            let mut basis = zero.clone();
            basis[basis_edge.a][basis_conductor] = C::new(1.0, 0.0);
            basis[basis_edge.b][basis_conductor] = C::new(-1.0, 0.0);
            let (response_voltage, _, _) = self.radial_step(&basis, cancel)?;
            for (row, &(chord, conductor)) in variables.iter().enumerate() {
                let edge = &self.edges[self.chords[chord]];
                let response = response_voltage[edge.a][conductor]
                    - response_voltage[edge.b][conductor]
                    - zero_voltage[edge.a][conductor]
                    + zero_voltage[edge.b][conductor];
                let series = if chord == basis_chord {
                    match edge.model {
                        EdgeModel::Line(z) => z[conductor][basis_conductor],
                        EdgeModel::GroundedSecondary { .. } => unreachable!("validated chord"),
                    }
                } else {
                    C::default()
                };
                matrix[row][column] = series - response;
            }
        }
        let compensation = ComplexLu::factor(matrix, cancel)?.solve(&rhs, cancel)?;
        let mut adjusted = nodal_currents;
        let mut chord_currents = vec![[C::default(); 4]; self.chords.len()];
        for (&(chord, conductor), &current) in variables.iter().zip(&compensation) {
            let edge = &self.edges[self.chords[chord]];
            chord_currents[chord][conductor] = current;
            adjusted[edge.a][conductor] = adjusted[edge.a][conductor] + current;
            adjusted[edge.b][conductor] = adjusted[edge.b][conductor] - current;
        }
        let (voltage, currents, earth) = self.radial_step(&adjusted, cancel)?;
        for (chord, &edge_index) in self.chords.iter().enumerate() {
            let edge = &self.edges[edge_index];
            let EdgeModel::Line(z) = edge.model else {
                unreachable!("validated chord")
            };
            for conductor in self.conductors[edge.a].iter().map(|c| phase_index(*c)) {
                let drop = (0..4).fold(C::default(), |sum, other| {
                    sum + z[conductor][other] * chord_currents[chord][other]
                });
                let residual =
                    (voltage[edge.a][conductor] - voltage[edge.b][conductor] - drop).abs();
                if !residual.is_finite() || residual > 1e-9 * self.nominal[edge.a] {
                    return Err(CalculationError::Numerical(self.buses[edge.a]));
                }
            }
        }
        Ok((voltage, currents, chord_currents, earth))
    }
    fn sweep(
        &self,
        currents: &[Vector],
        source_neutral: C,
        cancel: &AtomicBool,
    ) -> Result<Vec<Vector>> {
        let shifted_source = self.source.map(|value| value + source_neutral);
        let mut voltage = vec![shifted_source; self.buses.len()];
        for &i in &self.order {
            check_cancel(cancel)?;
            if let Some((p, e)) = self.parent[i] {
                let upstream = voltage[p];
                voltage[i] = self.edges[e].downstream_voltage(upstream, currents[i]);
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
    let zero_currents = vec![[C::default(); 4]; circuit.buses.len()];
    let mut voltage = circuit.sweep(&zero_currents, C::default(), cancel)?;
    let mut residual = f64::INFINITY;
    for iteration in 1..=options.max_iterations {
        check_cancel(cancel)?;
        (voltage, _, _, _) =
            circuit.linear_step(circuit.injection_currents(&voltage, cancel)?, cancel)?;
        let (corrected, currents, chord_currents, earth_currents) =
            circuit.linear_step(circuit.injection_currents(&voltage, cancel)?, cancel)?;
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
        let mut line_loss = C::default();
        let mut transformer_loss = C::default();
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
                conductors: circuit.conductors[i].clone(),
                conductor_voltages_v: voltage[i].map(Into::into),
                phase_neutral,
                phase_phase_v,
                neutral_displacement_v: voltage[i][3].abs(),
                negative_sequence_percent: unbalance,
            });
            if let Some((p, e)) = circuit.parent[i] {
                let edge = &circuit.edges[e];
                let upstream_current = edge.upstream_current(currents[i]);
                let from = power(voltage[p], upstream_current);
                let to = power(voltage[i], currents[i]);
                let branch_loss = from - to;
                if edge.transformer() {
                    transformer_loss = transformer_loss + branch_loss;
                } else {
                    line_loss = line_loss + branch_loss;
                }
                let loading = edge.loading(upstream_current, currents[i]);
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
                    currents_from_a: upstream_current.map(Into::into),
                    currents_to_a: currents[i].map(Into::into),
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
        for (chord, &edge_index) in circuit.chords.iter().enumerate() {
            check_cancel(cancel)?;
            let edge = &circuit.edges[edge_index];
            let current = chord_currents[chord];
            let from = power(voltage[edge.a], current);
            let to = power(voltage[edge.b], current);
            let branch_loss = from - to;
            let loading = edge.loading(current, current);
            if !from.finite()
                || !to.finite()
                || !branch_loss.finite()
                || loading.is_some_and(|value| !value.is_finite())
            {
                return Err(CalculationError::Numerical(circuit.buses[edge.a]));
            }
            line_loss = line_loss + branch_loss;
            branches.push(BranchResult {
                id: edge.id,
                from_bus: circuit.buses[edge.a],
                to_bus: circuit.buses[edge.b],
                currents_a: current.map(Into::into),
                currents_from_a: current.map(Into::into),
                currents_to_a: current.map(Into::into),
                p_from_w: from.re,
                q_from_var: from.im,
                p_to_w: to.re,
                q_to_var: to.im,
                loss_w: branch_loss.re,
                loss_var: branch_loss.im,
                loading_percent: loading,
            });
        }
        let mut grounding_loss = C::default();
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
        for (anchor, (impedance, current)) in circuit.earth.anchor_states(&earth_currents) {
            let ground_loss = impedance * current.abs().powi(2);
            grounding_loss = grounding_loss + ground_loss;
            if !current.finite() || !current.abs().is_finite() || !ground_loss.finite() {
                return Err(CalculationError::Numerical(circuit.buses[anchor]));
            }
            groundings.push(GroundingResult {
                bus_id: circuit.buses[anchor],
                current_to_earth_a: current.into(),
                voltage_v: (impedance * current).into(),
                loss_w: ground_loss.re,
                loss_var: ground_loss.im,
            });
        }
        let loss = line_loss + transformer_loss + grounding_loss;
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
            transformer_loss_w:transformer_loss.re,transformer_loss_var:transformer_loss.im,
            grounding_loss_w:grounding_loss.re,grounding_loss_var:grounding_loss.im,groundings,buses,branches,
            assumptions:vec![
                if circuit.chords.is_empty() {
                    "Réseau radial ABC/ABCN, matrices primitives complètes et source de tension équilibrée idéale.".into()
                } else {
                    format!("Réseau ABC/ABCN faiblement maillé avec {} branche(s) de fermeture, matrices primitives complètes et source de tension équilibrée idéale.", circuit.chords.len())
                },
                "Prises de terre résolues par zones galvaniques indépendantes, ancrées au neutre source avec son impédance déclarée ou au neutre BT Dyn/Yzn idéal ; chaque zone utilise une référence équipotentielle, sans couplage de sol entre zones ou électrodes.".into(),
                "Transformateurs couverts : Dyn et Yzn HTA/BT, secondaire à neutre directement mis à la terre, excitation nulle et impédance homopolaire déclarée ; le primaire étoile Yzn est isolé sans borne N.".into(),
                "Aucun shunt de ligne ni point étoile flottant. Impédances de source de défaut non utilisées en régime permanent.".into(),
                "Taux de charge : limite de ligne appliquée aux conducteurs et courants assignés des deux enroulements ; aucun calcul thermique ni harmonique.".into(),
            ],
        });
    }
    Err(CalculationError::NonConvergent {
        iterations: options.max_iterations,
        residual_pu: residual,
    })
}
