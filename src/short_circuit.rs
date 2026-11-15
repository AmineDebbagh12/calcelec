//! Initial sequence-domain short-circuit engine for radial distribution networks.
use crate::{
    complex::C,
    electrical::{Conductor, ElectricalData, Grounding, Impedance, WindingConnection},
    model::{Equipment, Error, Id, Network, Result},
    project::Project,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

pub const SOLVER_VERSION: &str = "radial-sequence-fault-0.1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    /// Minimum voltage factor applied to the nominal line-to-line voltage.
    pub voltage_factor_min: f64,
    /// Impedance at the fault point, used once for phase-phase and three times for phase-earth.
    pub fault_impedance_ohm: Impedance,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protection_requirements: Vec<ProtectionRequirement>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            voltage_factor_min: 0.95,
            fault_impedance_ohm: Impedance::ZERO,
            protection_requirements: vec![],
        }
    }
}

impl Options {
    fn validate(&self) -> Result<()> {
        if !self.voltage_factor_min.is_finite()
            || self.voltage_factor_min <= 0.0
            || self.voltage_factor_min > 1.2
        {
            return Err(Error::Invalid(
                "facteur de tension minimal fini, positif et inférieur ou égal à 1,2 requis".into(),
            ));
        }
        self.fault_impedance_ohm.validate()?;
        for requirement in &self.protection_requirements {
            requirement.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidualCriterion {
    /// The fault current must reach the protection pickup threshold.
    MinimumDetection,
    /// The residual current must remain below an admissible ceiling.
    MaximumAdmissible,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResidualRequirement {
    pub criterion: ResidualCriterion,
    pub threshold_a: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectionRequirement {
    pub name: String,
    pub bus_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_icc_b_a: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub residual: Option<ResidualRequirement>,
}

impl ProtectionRequirement {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty()
            || self
                .minimum_icc_b_a
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
            || self
                .residual
                .as_ref()
                .is_some_and(|value| !value.threshold_a.is_finite() || value.threshold_a <= 0.0)
        {
            return Err(Error::Invalid(
                "exigence de protection : nom et seuils positifs finis requis".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceImpedance {
    pub r_ohm: f64,
    pub x_ohm: f64,
    pub magnitude_ohm: f64,
}

impl From<C> for SequenceImpedance {
    fn from(value: C) -> Self {
        Self {
            r_ohm: value.re,
            x_ohm: value.im,
            magnitude_ohm: value.abs(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusFaultResult {
    pub bus_id: Id,
    pub nominal_voltage_ll_v: f64,
    pub positive_sequence: SequenceImpedance,
    pub negative_sequence: SequenceImpedance,
    pub zero_sequence: Option<SequenceImpedance>,
    /// Minimum phase-phase short-circuit current at the fault point.
    pub icc_b_min_a: f64,
    /// Residual current for a phase-earth fault; equal to three times I0.
    pub three_i0_a: Option<f64>,
    pub earth_fault_limitation: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortCircuitResult {
    pub buses: Vec<BusFaultResult>,
    pub minimum_icc_b_bus: Id,
    pub minimum_icc_b_a: f64,
    pub maximum_three_i0_bus: Option<Id>,
    pub maximum_three_i0_a: Option<f64>,
    pub protection_assessments: Vec<ProtectionAssessment>,
    pub assumptions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectionAssessment {
    pub name: String,
    pub bus_id: Id,
    pub icc_b_min_a: f64,
    pub minimum_icc_b_threshold_a: Option<f64>,
    /// Positive means that the minimum phase-fault requirement is met.
    pub icc_b_margin_a: Option<f64>,
    pub icc_b_passed: Option<bool>,
    pub three_i0_a: Option<f64>,
    pub residual_criterion: Option<ResidualCriterion>,
    pub residual_threshold_a: Option<f64>,
    /// Positive means that the selected residual-current criterion is met.
    pub residual_margin_a: Option<f64>,
    pub residual_passed: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudyResult {
    pub project_id: Id,
    pub network_version: Id,
    pub solver_version: String,
    pub options: Options,
    pub result: ShortCircuitResult,
}

#[derive(Clone, Copy)]
enum EdgeModel {
    Line {
        positive: C,
        zero: Option<C>,
    },
    Transformer {
        ratio: f64,
        positive_to: C,
        zero_to: Option<C>,
    },
    Switch,
}

#[derive(Clone, Copy)]
struct Edge {
    id: Id,
    a: usize,
    b: usize,
    model: EdgeModel,
}

fn c(value: Impedance) -> C {
    C::new(value.r, value.x)
}

fn phase_indices(conductors: &[Conductor]) -> Result<[usize; 3]> {
    let index = |wanted| {
        conductors
            .iter()
            .position(|value| *value == wanted)
            .ok_or_else(|| {
                Error::Invalid("conducteurs A/B/C requis pour le calcul de défaut".into())
            })
    };
    Ok([
        index(Conductor::A)?,
        index(Conductor::B)?,
        index(Conductor::C)?,
    ])
}

fn line_sequences(
    name: &str,
    data: &crate::electrical::LineData,
    length_m: f64,
) -> Result<(C, Option<C>)> {
    if data.shunt_siemens_per_km.is_some() {
        return Err(Error::Invalid(format!(
            "{name} : les admittances shunt et courants capacitifs homopolaires seront ajoutés dans une version suivante"
        )));
    }
    let phase = phase_indices(&data.conductors)?;
    let z = |row: usize, column: usize| c(data.series_ohm_per_km[row][column]);
    let scale = data
        .series_ohm_per_km
        .iter()
        .flatten()
        .map(|value| value.r.abs().max(value.x.abs()))
        .fold(1.0, f64::max);
    let tolerance = scale * 1e-10;
    let own = z(phase[0], phase[0]);
    let mutual = z(phase[0], phase[1]);
    for &row in &phase {
        if (z(row, row) - own).abs() > tolerance {
            return Err(Error::Invalid(format!(
                "{name} : matrice de phases transposée requise pour le calcul de défaut en séquences"
            )));
        }
        for &column in &phase {
            if row != column && (z(row, column) - mutual).abs() > tolerance {
                return Err(Error::Invalid(format!(
                    "{name} : couplages mutuels de phases identiques requis"
                )));
            }
        }
    }
    let length = length_m / 1000.0;
    let positive = (own - mutual) * length;
    let zero = if data.conductors.len() == 3 {
        Some((own + mutual * 2.0) * length)
    } else {
        None
    };
    Ok((positive, zero))
}

fn transformer_model(
    name: &str,
    data: &crate::electrical::TransformerData,
    rated_va: f64,
    uk_percent: f64,
) -> Result<EdgeModel> {
    let supported = matches!(
        (data.from.connection, data.to.connection),
        (WindingConnection::Delta, WindingConnection::Star)
            | (WindingConnection::Star, WindingConnection::Zigzag)
    );
    if !supported {
        return Err(Error::Invalid(format!(
            "{name} : couplage non couvert par le premier moteur de défaut"
        )));
    }
    let ratio = data.ratio();
    let series = data.series_pu(rated_va, uk_percent)?;
    let base_to = data.to.rated_voltage_ll_v.powi(2) / rated_va;
    let positive_to = c(series) * base_to;
    let zero_to = match (&data.to.grounding, data.zero_sequence_ohm_referred_from) {
        (Grounding::Impedance { ohm: grounding }, Some(zero)) => {
            Some(c(zero) * (1.0 / ratio.powi(2)) + c(*grounding) * 3.0)
        }
        (Grounding::Isolated, _) | (_, None) => None,
        _ => None,
    };
    Ok(EdgeModel::Transformer {
        ratio,
        positive_to,
        zero_to,
    })
}

pub fn solve(network: &Network, options: &Options) -> Result<ShortCircuitResult> {
    network.validate()?;
    options.validate()?;
    let buses: Vec<_> = network
        .objects
        .values()
        .filter(|object| object.in_service && matches!(object.equipment, Equipment::Bus { .. }))
        .map(|object| object.id)
        .collect();
    let index: BTreeMap<_, _> = buses.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let sources: Vec<_> = network
        .objects
        .values()
        .filter(|object| object.in_service)
        .filter_map(|object| match (&object.equipment, &object.electrical) {
            (Equipment::Source { bus, .. }, Some(ElectricalData::Source(data)))
                if index.contains_key(bus) =>
            {
                Some((object, *bus, data))
            }
            _ => None,
        })
        .collect();
    if sources.len() != 1 {
        return Err(Error::Invalid(
            "le premier moteur de défaut exige une source active unique avec données de séquence"
                .into(),
        ));
    }
    let (_, source_bus, source_data) = sources[0];
    let source_positive = source_data
        .positive_sequence_ohm
        .ok_or_else(|| Error::Invalid("impédance directe de source requise pour IccBmin".into()))?;
    let source_negative = source_data
        .negative_sequence_ohm
        .ok_or_else(|| Error::Invalid("impédance inverse de source requise pour IccBmin".into()))?;
    let source_zero = source_data.zero_sequence_ohm.map(c);
    let root = index[&source_bus];

    let mut edges = vec![];
    for object in network.objects.values().filter(|object| object.in_service) {
        let (from, to, model) = match (&object.equipment, &object.electrical) {
            (
                Equipment::Line {
                    from, to, length_m, ..
                },
                Some(ElectricalData::Line(data)),
            ) => {
                let (positive, zero) = line_sequences(&object.name, data, *length_m)?;
                (*from, *to, EdgeModel::Line { positive, zero })
            }
            (
                Equipment::Transformer {
                    from,
                    to,
                    rated_va,
                    uk_percent,
                },
                Some(ElectricalData::Transformer(data)),
            ) => (
                *from,
                *to,
                transformer_model(&object.name, data, *rated_va, *uk_percent)?,
            ),
            (
                Equipment::Switch {
                    from,
                    to,
                    closed: true,
                },
                _,
            ) => (*from, *to, EdgeModel::Switch),
            (Equipment::Line { .. } | Equipment::Transformer { .. }, _) => {
                return Err(Error::Invalid(format!(
                    "{} : modèle électrique détaillé requis pour le calcul de défaut",
                    object.name
                )));
            }
            _ => continue,
        };
        let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) else {
            continue;
        };
        edges.push(Edge {
            id: object.id,
            a,
            b,
            model,
        });
    }
    let mut adjacency = vec![vec![]; buses.len()];
    for (edge, value) in edges.iter().enumerate() {
        adjacency[value.a].push((value.b, edge));
        adjacency[value.b].push((value.a, edge));
    }
    let mut parent = vec![None; buses.len()];
    let mut seen = vec![false; buses.len()];
    let mut edge_seen = vec![false; edges.len()];
    let mut order = vec![];
    seen[root] = true;
    let mut queue = VecDeque::from([root]);
    while let Some(node) = queue.pop_front() {
        order.push(node);
        for &(next, edge) in &adjacency[node] {
            if edge_seen[edge] {
                continue;
            }
            edge_seen[edge] = true;
            if seen[next] {
                return Err(Error::Invalid(format!(
                    "{} : les mailles seront couvertes dans une version suivante du moteur de défaut",
                    network.objects[&edges[edge].id].name
                )));
            }
            seen[next] = true;
            parent[next] = Some((node, edge));
            queue.push_back(next);
        }
    }
    if order.len() != buses.len() {
        return Err(Error::Invalid(
            "tous les jeux de barres doivent être reliés à la source pour le calcul de défaut"
                .into(),
        ));
    }

    let mut positive = vec![C::default(); buses.len()];
    let mut negative = vec![C::default(); buses.len()];
    let mut zero = vec![None; buses.len()];
    positive[root] = c(source_positive);
    negative[root] = c(source_negative);
    zero[root] = source_zero;
    for &node in order.iter().skip(1) {
        let (upstream, edge) = parent[node].expect("all non-root buses have a parent");
        let value = edges[edge];
        match value.model {
            EdgeModel::Line {
                positive: line,
                zero: line_zero,
            } => {
                positive[node] = positive[upstream] + line;
                negative[node] = negative[upstream] + line;
                zero[node] = zero[upstream]
                    .zip(line_zero)
                    .map(|(upstream, line)| upstream + line);
            }
            EdgeModel::Transformer {
                ratio,
                positive_to,
                zero_to,
            } => {
                if value.a != upstream || value.b != node {
                    return Err(Error::Invalid(format!(
                        "{} : transformateur à orienter du réseau amont vers l'aval",
                        network.objects[&value.id].name
                    )));
                }
                positive[node] = positive[upstream] * (1.0 / ratio.powi(2)) + positive_to;
                negative[node] = negative[upstream] * (1.0 / ratio.powi(2)) + positive_to;
                zero[node] = zero_to;
            }
            EdgeModel::Switch => {
                positive[node] = positive[upstream];
                negative[node] = negative[upstream];
                zero[node] = zero[upstream];
            }
        }
    }

    let fault = c(options.fault_impedance_ohm);
    let mut results = vec![];
    for (node, &bus) in buses.iter().enumerate() {
        let Equipment::Bus { voltage_v } = network.objects[&bus].equipment else {
            unreachable!()
        };
        let line_line_denominator = positive[node] + negative[node] + fault;
        if line_line_denominator.abs() <= 1e-15 || !line_line_denominator.finite() {
            return Err(Error::Invalid(format!(
                "{} : impédance de défaut biphasé nulle ou non finie",
                network.objects[&bus].name
            )));
        }
        let prefault_ll = voltage_v * options.voltage_factor_min;
        let icc_b_min_a = prefault_ll / line_line_denominator.abs();
        let (three_i0_a, limitation) = match zero[node] {
            Some(zero) => {
                let denominator = positive[node] + negative[node] + zero + fault * 3.0;
                if denominator.abs() <= 1e-15 || !denominator.finite() {
                    return Err(Error::Invalid(format!(
                        "{} : impédance de défaut monophasé-terre nulle ou non finie",
                        network.objects[&bus].name
                    )));
                }
                (
                    Some(3.0_f64.sqrt() * prefault_ll / denominator.abs()),
                    None,
                )
            }
            None => (
                None,
                Some(
                    "chemin homopolaire indisponible ou ligne ABCN à neutre explicite non encore réduite"
                        .into(),
                ),
            ),
        };
        results.push(BusFaultResult {
            bus_id: bus,
            nominal_voltage_ll_v: voltage_v,
            positive_sequence: positive[node].into(),
            negative_sequence: negative[node].into(),
            zero_sequence: zero[node].map(Into::into),
            icc_b_min_a,
            three_i0_a,
            earth_fault_limitation: limitation,
        });
    }
    let minimum = results
        .iter()
        .min_by(|left, right| left.icc_b_min_a.total_cmp(&right.icc_b_min_a))
        .expect("validated non-empty network");
    let maximum_earth = results
        .iter()
        .filter_map(|result| result.three_i0_a.map(|current| (result.bus_id, current)))
        .max_by(|left, right| left.1.total_cmp(&right.1));
    let mut protection_assessments = vec![];
    for requirement in &options.protection_requirements {
        let bus = results
            .iter()
            .find(|result| result.bus_id == requirement.bus_id)
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "{} : jeu de barres de protection absent ou hors service",
                    requirement.name
                ))
            })?;
        let icc_b_margin = requirement
            .minimum_icc_b_a
            .map(|threshold| bus.icc_b_min_a - threshold);
        let (criterion, residual_threshold, residual_margin, residual_passed) =
            match (&requirement.residual, bus.three_i0_a) {
                (Some(residual), Some(current)) => {
                    let margin = match residual.criterion {
                        ResidualCriterion::MinimumDetection => current - residual.threshold_a,
                        ResidualCriterion::MaximumAdmissible => residual.threshold_a - current,
                    };
                    (
                        Some(residual.criterion),
                        Some(residual.threshold_a),
                        Some(margin),
                        Some(margin >= 0.0),
                    )
                }
                (Some(residual), None) => (
                    Some(residual.criterion),
                    Some(residual.threshold_a),
                    None,
                    None,
                ),
                (None, _) => (None, None, None, None),
            };
        protection_assessments.push(ProtectionAssessment {
            name: requirement.name.clone(),
            bus_id: requirement.bus_id,
            icc_b_min_a: bus.icc_b_min_a,
            minimum_icc_b_threshold_a: requirement.minimum_icc_b_a,
            icc_b_margin_a: icc_b_margin,
            icc_b_passed: icc_b_margin.map(|margin| margin >= 0.0),
            three_i0_a: bus.three_i0_a,
            residual_criterion: criterion,
            residual_threshold_a: residual_threshold,
            residual_margin_a: residual_margin,
            residual_passed,
        });
    }
    Ok(ShortCircuitResult {
        minimum_icc_b_bus: minimum.bus_id,
        minimum_icc_b_a: minimum.icc_b_min_a,
        maximum_three_i0_bus: maximum_earth.map(|value| value.0),
        maximum_three_i0_a: maximum_earth.map(|value| value.1),
        protection_assessments,
        buses: results,
        assumptions: vec![
            "Calcul initial radial par réseaux de séquence ; IccBmin = cmin·Un/|Z1+Z2+Zf| et 3I0 = √3·cmin·Un/|Z1+Z2+Z0+3Zf|.".into(),
            "Les impédances de source fournies sont supposées correspondre au cas minimal étudié ; charges et contributions des producteurs sont négligées.".into(),
            "Les lignes ABCN conservent IccBmin mais leur défaut terre reste non publié tant que le partage exact neutre/terre n'est pas intégré au moteur de défaut.".into(),
            "Les admittances shunt, courants capacitifs homopolaires, mailles, sources multiples et protections temporisées ne sont pas encore couverts.".into(),
        ],
    })
}

pub fn study(project: &Project, options: &Options) -> Result<StudyResult> {
    let network = project.network()?;
    Ok(StudyResult {
        project_id: project.id,
        network_version: project.head(),
        solver_version: SOLVER_VERSION.into(),
        options: options.clone(),
        result: solve(&network, options)?,
    })
}
