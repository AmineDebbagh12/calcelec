//! Balanced radial AC power flow, current-summation backward/forward sweep.
//! No shunts, tap control, transformer phase shift, neutral or zero-sequence model.
use crate::{
    model::{Equipment, Id, Network, Phase},
    project::Project,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::atomic::{AtomicBool, Ordering},
};

const BASE_VA: f64 = 1_000_000.0;
pub const SOLVER_VERSION: &str = "radial-balanced-0.1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub max_iterations: usize,
    pub tolerance_pu: f64,
    pub allow_balanced_equivalent: bool,
    pub allow_reactive_transformers: bool,
    pub min_voltage_pu: f64,
    pub max_voltage_pu: f64,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            max_iterations: 200,
            tolerance_pu: 1e-9,
            allow_balanced_equivalent: false,
            allow_reactive_transformers: false,
            min_voltage_pu: 0.9,
            max_voltage_pu: 1.1,
        }
    }
}
impl Options {
    pub fn simplified() -> Self {
        Self {
            allow_balanced_equivalent: true,
            allow_reactive_transformers: true,
            ..Self::default()
        }
    }
    fn validate(&self) -> Result<()> {
        if self.max_iterations == 0
            || self.max_iterations > 100_000
            || !self.tolerance_pu.is_finite()
            || self.tolerance_pu <= 0.0
            || self.tolerance_pu > 1e-3
            || !self.min_voltage_pu.is_finite()
            || !self.max_voltage_pu.is_finite()
            || self.min_voltage_pu <= 0.0
            || self.max_voltage_pu <= self.min_voltage_pu
        {
            return Err(CalculationError::InvalidOptions);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CalculationError {
    #[error("Modèle invalide : {0}")]
    Model(#[from] crate::model::Error),
    #[error("Configuration non supportée : {0}")]
    Unsupported(String),
    #[error(
        "Options invalides : vérifier itérations, tolérance (0 < ε ≤ 10⁻³) et seuils de tension"
    )]
    InvalidOptions,
    #[error(
        "Calcul non convergé après {iterations} itérations (résidu de tension : {residual_pu:.3e} pu)"
    )]
    NonConvergent { iterations: usize, residual_pu: f64 },
    #[error("Instabilité numérique ou effondrement de tension au jeu de barres {0}")]
    Numerical(Id),
    #[error("Calcul annulé")]
    Cancelled,
}
pub type Result<T> = std::result::Result<T, CalculationError>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusResult {
    pub id: Id,
    pub voltage_pu: f64,
    pub voltage_ll_v: f64,
    pub angle_deg: f64,
    pub outside_voltage_limits: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BranchResult {
    pub id: Id,
    /// Oriented away from the source, regardless of stored from/to ordering.
    pub from_bus: Id,
    pub to_bus: Id,
    pub current_from_a: f64,
    pub current_to_a: f64,
    pub p_from_w: f64,
    pub q_from_var: f64,
    pub p_to_w: f64,
    pub q_to_var: f64,
    pub loss_w: f64,
    pub loss_var: f64,
    pub loading_percent: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoadFlow {
    pub iterations: usize,
    /// Max complex voltage residual of the current-summation fixed-point equations.
    pub residual_pu: f64,
    pub source_p_w: f64,
    pub source_q_var: f64,
    pub net_demand_p_w: f64,
    pub net_demand_q_var: f64,
    pub total_loss_w: f64,
    pub total_loss_var: f64,
    pub balance_error_va: f64,
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

// A minimal private complex scalar keeps numerical conventions in one place.
#[derive(Clone, Copy, Debug, Default)]
struct C {
    re: f64,
    im: f64,
}
impl C {
    fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }
    fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
    fn finite(self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }
}
impl std::ops::Add for C {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self::new(self.re + b.re, self.im + b.im)
    }
}
impl std::ops::Sub for C {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self::new(self.re - b.re, self.im - b.im)
    }
}
impl std::ops::Mul for C {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        Self::new(
            self.re * b.re - self.im * b.im,
            self.re * b.im + self.im * b.re,
        )
    }
}
impl std::ops::Mul<f64> for C {
    type Output = Self;
    fn mul(self, b: f64) -> Self {
        Self::new(self.re * b, self.im * b)
    }
}
impl std::ops::Div for C {
    type Output = Self;
    fn div(self, b: Self) -> Self {
        let d = b.re * b.re + b.im * b.im;
        Self::new(
            (self.re * b.re + self.im * b.im) / d,
            (self.im * b.re - self.re * b.im) / d,
        )
    }
}

struct Edge {
    id: Id,
    a: usize,
    b: usize,
    z: C,
    limit: Limit,
}
enum Limit {
    Current(f64),
    Power(f64),
    None,
}
struct Circuit {
    buses: Vec<Id>,
    nominal: Vec<f64>,
    demand: Vec<C>,
    edges: Vec<Edge>,
    order: Vec<usize>,
    parent: Vec<Option<(usize, usize)>>,
    source_voltage: f64,
    assumptions: Vec<String>,
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(CalculationError::Cancelled)
    } else {
        Ok(())
    }
}
impl Circuit {
    fn build(network: &Network, options: &Options, cancel: &AtomicBool) -> Result<Self> {
        network.validate()?;
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
                Equipment::Bus { voltage_v } => voltage_v,
                _ => unreachable!(),
            })
            .collect();
        let mut sources = vec![];
        let mut edges = vec![];
        let mut demand = vec![C::default(); buses.len()];
        let mut assumptions=vec!["Réseau radial AC équilibré, charges P/Q constantes, sans shunts ni neutre. Seuils de tension indicatifs configurables.".into()];
        let mut approximated_loads = 0;
        let mut approximated_transformers = 0;
        for o in network.objects.values().filter(|o| o.in_service) {
            cancelled(cancel)?;
            match o.equipment {
                Equipment::Source { bus, voltage_pu } => {
                    if let Some(i) = index.get(&bus) {
                        sources.push((*i, voltage_pu));
                    }
                }
                Equipment::Load {
                    bus,
                    p_w,
                    q_var,
                    phase,
                }
                | Equipment::Generator {
                    bus,
                    p_w,
                    q_var,
                    phase,
                } => {
                    let Some(&i) = index.get(&bus) else {
                        continue;
                    };
                    if phase != Phase::Abc {
                        if !options.allow_balanced_equivalent {
                            return Err(CalculationError::Unsupported(format!(
                                "{} : charge/injection monophasée ; activer explicitement l'équivalent équilibré ou utiliser un futur moteur déséquilibré",
                                o.name
                            )));
                        }
                        approximated_loads += 1;
                    }
                    let sign = if matches!(o.equipment, Equipment::Load { .. }) {
                        1.0
                    } else {
                        -1.0
                    };
                    demand[i] = demand[i] + C::new(sign * p_w / BASE_VA, sign * q_var / BASE_VA);
                }
                Equipment::Line {
                    from,
                    to,
                    length_m,
                    r_ohm_per_km,
                    x_ohm_per_km,
                    max_current_a,
                } => {
                    if let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) {
                        let z = C::new(r_ohm_per_km, x_ohm_per_km)
                            * (length_m / 1000.0 * BASE_VA / nominal[a].powi(2));
                        edges.push(Edge {
                            id: o.id,
                            a,
                            b,
                            z,
                            limit: Limit::Current(max_current_a),
                        });
                    }
                }
                Equipment::Transformer {
                    from,
                    to,
                    rated_va,
                    uk_percent,
                } => {
                    if let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) {
                        if !options.allow_reactive_transformers {
                            return Err(CalculationError::Unsupported(format!(
                                "{} : paramètres de transformateur incomplets ; autoriser explicitement le modèle purement réactif simplifié",
                                o.name
                            )));
                        }
                        approximated_transformers += 1;
                        edges.push(Edge {
                            id: o.id,
                            a,
                            b,
                            z: C::new(0.0, uk_percent / 100.0 * BASE_VA / rated_va),
                            limit: Limit::Power(rated_va),
                        });
                    }
                }
                Equipment::Switch {
                    from,
                    to,
                    closed: true,
                } => {
                    if let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) {
                        edges.push(Edge {
                            id: o.id,
                            a,
                            b,
                            z: C::default(),
                            limit: Limit::None,
                        });
                    }
                }
                _ => {}
            }
        }
        if sources.len() != 1 {
            return Err(CalculationError::Unsupported(format!(
                "ce moteur exige une source active unique ; {} trouvées",
                sources.len()
            )));
        }
        if approximated_loads > 0 {
            assumptions.push(format!("{approximated_loads} charges/injections monophasées remplacées par leurs équivalents triphasés équilibrés. Aucun résultat de déséquilibre ou de neutre."));
        }
        if approximated_transformers > 0 {
            assumptions.push(format!("{approximated_transformers} transformateurs : uk assimilé à X, résistance et pertes fer nulles, rapport nominal des jeux de barres, prise 1 pu, déphasage nul. Les pertes actives des transformateurs ne sont pas évaluées."));
        }
        let mut adjacency = vec![vec![]; buses.len()];
        for (e, edge) in edges.iter().enumerate() {
            adjacency[edge.a].push((edge.b, e));
            adjacency[edge.b].push((edge.a, e));
        }
        let root = sources[0].0;
        let mut queue = VecDeque::from([root]);
        let mut seen = vec![false; buses.len()];
        seen[root] = true;
        let mut parent = vec![None; buses.len()];
        let mut order = vec![];
        while let Some(a) = queue.pop_front() {
            cancelled(cancel)?;
            order.push(a);
            for &(b, e) in &adjacency[a] {
                if parent[a].is_some_and(|(_, pe)| pe == e) {
                    continue;
                }
                if seen[b] {
                    return Err(CalculationError::Unsupported(format!(
                        "boucle ou branches parallèles détectées à {} ; moteur maillé non encore disponible",
                        network.objects[&edges[e].id].name
                    )));
                }
                seen[b] = true;
                parent[b] = Some((a, e));
                queue.push_back(b);
            }
        }
        if order.len() != buses.len() {
            let names = buses
                .iter()
                .enumerate()
                .filter(|(i, _)| !seen[*i])
                .map(|(_, id)| network.objects[id].name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(CalculationError::Unsupported(format!(
                "jeux de barres en service non alimentés : {names}"
            )));
        }
        if edges.iter().any(|e| !e.z.finite()) || demand.iter().any(|s| !s.finite()) {
            return Err(CalculationError::Unsupported(
                "paramètres trop grands pour le domaine numérique".into(),
            ));
        }
        Ok(Self {
            buses,
            nominal,
            demand,
            edges,
            order,
            parent,
            source_voltage: sources[0].1,
            assumptions,
        })
    }
    fn currents(&self, voltages: &[C]) -> Result<Vec<C>> {
        let mut currents = vec![C::default(); self.buses.len()];
        for &i in self.order.iter().rev() {
            if !voltages[i].finite() || voltages[i].abs() < 1e-6 {
                return Err(CalculationError::Numerical(self.buses[i]));
            }
            currents[i] = currents[i] + (self.demand[i] / voltages[i]).conj();
            if !currents[i].finite() {
                return Err(CalculationError::Numerical(self.buses[i]));
            }
            if let Some((p, _)) = self.parent[i] {
                currents[p] = currents[p] + currents[i];
            }
        }
        Ok(currents)
    }
    fn sweep(&self, currents: &[C]) -> Vec<C> {
        let mut voltages = vec![C::new(self.source_voltage, 0.0); self.buses.len()];
        for &i in &self.order {
            if let Some((p, e)) = self.parent[i] {
                voltages[i] = voltages[p] - self.edges[e].z * currents[i];
            }
        }
        voltages
    }
}

pub fn solve(network: &Network, options: &Options) -> Result<LoadFlow> {
    solve_cancellable(network, options, &AtomicBool::new(false))
}
pub fn study(project: &Project, options: &Options, cancel: &AtomicBool) -> Result<StudyResult> {
    cancelled(cancel)?;
    let result = solve_cancellable(&project.network()?, options, cancel)?;
    Ok(StudyResult {
        project_id: project.id,
        network_version: project.head(),
        solver_version: SOLVER_VERSION.into(),
        options: options.clone(),
        result,
    })
}
pub fn solve_cancellable(
    network: &Network,
    options: &Options,
    cancel: &AtomicBool,
) -> Result<LoadFlow> {
    options.validate()?;
    cancelled(cancel)?;
    let circuit = Circuit::build(network, options, cancel)?;
    let mut voltages = vec![C::new(circuit.source_voltage, 0.0); circuit.buses.len()];
    let mut residual = f64::INFINITY;
    for iteration in 1..=options.max_iterations {
        cancelled(cancel)?;
        let old_currents = circuit.currents(&voltages)?;
        voltages = circuit.sweep(&old_currents);
        let currents = circuit.currents(&voltages)?;
        let corrected = circuit.sweep(&currents);
        residual = voltages
            .iter()
            .zip(corrected)
            .map(|(a, b)| (*a - b).abs())
            .fold(0.0, f64::max);
        if !residual.is_finite() {
            return Err(CalculationError::Numerical(circuit.buses[circuit.order[0]]));
        }
        if residual > options.tolerance_pu {
            continue;
        }
        let mut buses = vec![];
        let mut branches = vec![];
        let mut loss = C::default();
        let mut net = C::default();
        for (i, &id) in circuit.buses.iter().enumerate() {
            let magnitude = voltages[i].abs();
            if !(magnitude * circuit.nominal[i]).is_finite() {
                return Err(CalculationError::Numerical(id));
            }
            net = net + circuit.demand[i] * BASE_VA;
            buses.push(BusResult {
                id,
                voltage_pu: magnitude,
                voltage_ll_v: magnitude * circuit.nominal[i],
                angle_deg: voltages[i].im.atan2(voltages[i].re).to_degrees(),
                outside_voltage_limits: magnitude < options.min_voltage_pu
                    || magnitude > options.max_voltage_pu,
            });
            if let Some((p, e)) = circuit.parent[i] {
                let edge = &circuit.edges[e];
                let current = currents[i];
                let from = voltages[p] * current.conj() * BASE_VA;
                let to = voltages[i] * current.conj() * BASE_VA;
                let branch_loss = edge.z * (current.abs().powi(2) * BASE_VA);
                loss = loss + branch_loss;
                let from_a = current.abs() * BASE_VA / (3.0_f64.sqrt() * circuit.nominal[p]);
                let to_a = current.abs() * BASE_VA / (3.0_f64.sqrt() * circuit.nominal[i]);
                let loading = match edge.limit {
                    Limit::Current(limit) => Some(from_a.max(to_a) / limit * 100.0),
                    Limit::Power(limit) => Some(from.abs().max(to.abs()) / limit * 100.0),
                    Limit::None => None,
                };
                if !from.finite()
                    || !to.finite()
                    || !branch_loss.finite()
                    || !from_a.is_finite()
                    || !to_a.is_finite()
                    || loading.is_some_and(|value| !value.is_finite())
                {
                    return Err(CalculationError::Numerical(id));
                }
                branches.push(BranchResult {
                    id: edge.id,
                    from_bus: circuit.buses[p],
                    to_bus: id,
                    current_from_a: from_a,
                    current_to_a: to_a,
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
        let source = voltages[circuit.order[0]] * currents[circuit.order[0]].conj() * BASE_VA;
        let balance = (source - net - loss).abs();
        if !source.finite() || !loss.finite() || !net.finite() || !balance.is_finite() {
            return Err(CalculationError::Numerical(circuit.buses[circuit.order[0]]));
        }
        cancelled(cancel)?;
        return Ok(LoadFlow {
            iterations: iteration,
            residual_pu: residual,
            source_p_w: source.re,
            source_q_var: source.im,
            net_demand_p_w: net.re,
            net_demand_q_var: net.im,
            total_loss_w: loss.re,
            total_loss_var: loss.im,
            balance_error_va: balance,
            buses,
            branches,
            assumptions: circuit.assumptions,
        });
    }
    Err(CalculationError::NonConvergent {
        iterations: options.max_iterations,
        residual_pu: residual,
    })
}
