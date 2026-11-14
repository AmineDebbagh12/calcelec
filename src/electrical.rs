//! Detailed steady-state/fault input data. Units: ohm, siemens, V, W, A, percent.
use crate::model::{Equipment, Error, Id, Network, Object, Phase, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Conductor {
    A,
    B,
    C,
    N,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Impedance {
    pub r: f64,
    pub x: f64,
}
impl Impedance {
    pub const ZERO: Self = Self { r: 0.0, x: 0.0 };
    pub fn validate(self) -> Result<()> {
        if !self.r.is_finite() || !self.x.is_finite() || self.r < 0.0 {
            return invalid("impédance : R doit être positive ou nulle, R et X finies");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Grounding {
    Unspecified,
    NotApplicable,
    Isolated,
    Impedance { ohm: Impedance },
}
impl Grounding {
    fn validate(&self) -> Result<()> {
        if let Self::Impedance { ohm } = self {
            ohm.validate()?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusData {
    pub conductors: Vec<Conductor>,
    pub neutral_grounding: Grounding,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admittance {
    pub g: f64,
    pub b: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineData {
    pub conductors: Vec<Conductor>,
    /// Full reciprocal primitive matrix; no implicit Kron reduction. Same order as conductors.
    pub series_ohm_per_km: Vec<Vec<Impedance>>,
    /// Full admittance matrix, same conductor order, real G and imaginary B.
    pub shunt_siemens_per_km: Option<Vec<Vec<Admittance>>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindingConnection {
    Delta,
    Star,
    Zigzag,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Winding {
    pub connection: WindingConnection,
    pub rated_voltage_ll_v: f64,
    /// Whether an N conductor is connected to the associated bus; grounding can also be internal.
    pub neutral_terminal: bool,
    pub grounding: Grounding,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    From,
    To,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapChanger {
    pub side: Side,
    pub min_position: i16,
    pub max_position: i16,
    pub neutral_position: i16,
    pub position: i16,
    pub step_percent: f64,
}
impl TapChanger {
    pub fn factor(&self) -> f64 {
        1.0 + (self.position as f64 - self.neutral_position as f64) * self.step_percent / 100.0
    }
    fn validate(&self) -> Result<()> {
        if self.min_position > self.max_position
            || !(self.min_position..=self.max_position).contains(&self.neutral_position)
            || !(self.min_position..=self.max_position).contains(&self.position)
            || !self.step_percent.is_finite()
            || self.step_percent < 0.0
        {
            return invalid("positions ou pas de prise invalides");
        }
        for pos in [self.min_position, self.max_position] {
            let factor =
                1.0 + (pos as f64 - self.neutral_position as f64) * self.step_percent / 100.0;
            positive(factor, "facteur de prise")?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformerData {
    pub from: Winding,
    pub to: Winding,
    /// LV/to phase relative to from: -clock*30 degrees modulo 360.
    pub clock: u8,
    pub copper_loss_w: f64,
    pub iron_loss_w: Option<f64>,
    pub no_load_current_percent: Option<f64>,
    pub tap: Option<TapChanger>,
    /// Zero-sequence series impedance referred to the from side, excluding external neutral impedances.
    pub zero_sequence_ohm_referred_from: Option<Impedance>,
}
impl TransformerData {
    pub fn phase_shift_deg(&self) -> f64 {
        let angle = -(self.clock as f64) * 30.0;
        (angle + 180.0).rem_euclid(360.0) - 180.0
    }
    pub fn ratio(&self) -> f64 {
        let ratio = self.from.rated_voltage_ll_v / self.to.rated_voltage_ll_v;
        match &self.tap {
            Some(t) if t.side == Side::From => ratio * t.factor(),
            Some(t) => ratio / t.factor(),
            None => ratio,
        }
    }
    /// On the transformer's own three-phase rated power base.
    pub fn series_pu(&self, rated_va: f64, uk_percent: f64) -> Result<Impedance> {
        positive(rated_va, "puissance assignée")?;
        positive(uk_percent, "uk")?;
        nonnegative(self.copper_loss_w, "pertes cuivre")?;
        let r = self.copper_loss_w / rated_va;
        let z = uk_percent / 100.0;
        if r > z + 1e-12 {
            return invalid("pertes cuivre incompatibles avec uk : R dépasse |Z|");
        }
        let x = ((z - r).max(0.0) * (z + r)).sqrt();
        let result = Impedance { r, x };
        result.validate()?;
        Ok(result)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceData {
    /// Internal sequence impedances, excluding separately declared neutral grounding.
    pub positive_sequence_ohm: Option<Impedance>,
    pub negative_sequence_ohm: Option<Impedance>,
    pub zero_sequence_ohm: Option<Impedance>,
    pub neutral_grounding: Grounding,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InjectionConnection {
    PhaseNeutral { phase: Conductor },
    PhasePhase { from: Conductor, to: Conductor },
    ThreePhaseStar { neutral: bool },
    ThreePhaseDelta,
}
impl InjectionConnection {
    pub fn balanced(&self) -> bool {
        matches!(self, Self::ThreePhaseStar { .. } | Self::ThreePhaseDelta)
    }
    fn required(&self) -> Vec<Conductor> {
        match self {
            Self::PhaseNeutral { phase } => vec![*phase, Conductor::N],
            Self::PhasePhase { from, to } => vec![*from, *to],
            Self::ThreePhaseStar { neutral: true } => {
                vec![Conductor::A, Conductor::B, Conductor::C, Conductor::N]
            }
            _ => vec![Conductor::A, Conductor::B, Conductor::C],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FaultContribution {
    Unspecified,
    None,
    CurrentLimited { max_current_a: f64 },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InjectionData {
    pub connection: InjectionConnection,
    pub fault_contribution: FaultContribution,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ElectricalData {
    Bus(BusData),
    Line(LineData),
    Transformer(TransformerData),
    Source(SourceData),
    Injection(InjectionData),
    Switch { conductors: Vec<Conductor> },
}
fn invalid<T>(message: &str) -> Result<T> {
    Err(Error::Invalid(message.into()))
}
fn positive(v: f64, name: &str) -> Result<()> {
    if !v.is_finite() || v <= 0.0 {
        return invalid(&format!("{name} doit être fini et positif"));
    }
    Ok(())
}
fn nonnegative(v: f64, name: &str) -> Result<()> {
    if !v.is_finite() || v < 0.0 {
        return invalid(&format!("{name} doit être fini et positif ou nul"));
    }
    Ok(())
}
fn conductors(values: &[Conductor]) -> Result<()> {
    let unique: BTreeSet<_> = values.iter().collect();
    if values.is_empty()
        || unique.len() != values.len()
        || values.iter().all(|c| *c == Conductor::N)
    {
        return invalid("conducteurs vides, dupliqués ou sans phase");
    }
    Ok(())
}
fn available(network: &Network, bus: Id, required: &[Conductor]) -> Result<()> {
    if let Some(ElectricalData::Bus(data)) = network
        .objects
        .get(&bus)
        .and_then(|o| o.electrical.as_ref())
        && required.iter().any(|c| !data.conductors.contains(c))
    {
        return invalid(&format!("conducteurs absents au jeu de barres {bus}"));
    }
    Ok(())
}
fn matrix(matrix: &[Vec<Impedance>], size: usize) -> Result<()> {
    if matrix.len() != size || matrix.iter().any(|row| row.len() != size) {
        return invalid("dimensions de matrice incompatibles avec l'ordre des conducteurs");
    }
    let scale = matrix
        .iter()
        .flatten()
        .map(|z| z.r.abs().max(z.x.abs()))
        .fold(1.0, f64::max);
    let tolerance = scale * 1e-12;
    for (i, row) in matrix.iter().enumerate() {
        for (j, z) in row.iter().copied().enumerate() {
            if !z.r.is_finite() || !z.x.is_finite() {
                return invalid("matrice non finie");
            }
            if (z.r - matrix[j][i].r).abs() > tolerance || (z.x - matrix[j][i].x).abs() > tolerance
            {
                return invalid("la matrice doit être réciproque (symétrique, sans conjugaison)");
            }
        }
    }
    // Symmetric elimination tests positive semidefiniteness of the real part (passivity).
    let mut real: Vec<Vec<f64>> = matrix
        .iter()
        .map(|row| row.iter().map(|z| z.r).collect())
        .collect();
    for i in 0..size {
        let pivot = real[i][i];
        if pivot < -tolerance {
            return invalid("partie résistive non passive");
        }
        if pivot.abs() <= tolerance {
            if real.iter().skip(i + 1).any(|row| row[i].abs() > tolerance) {
                return invalid("partie résistive non passive (pivot nul couplé)");
            }
        } else {
            let column: Vec<_> = real.iter().map(|row| row[i]).collect();
            for j in i + 1..size {
                for k in i + 1..size {
                    real[j][k] -= column[j] * column[k] / pivot;
                }
            }
        }
    }
    if real.iter().flatten().any(|v| !v.is_finite()) {
        return invalid("matrice hors domaine numérique de validation");
    }
    Ok(())
}
fn winding(w: &Winding, network: &Network, bus: Id) -> Result<()> {
    positive(w.rated_voltage_ll_v, "tension assignée d'enroulement")?;
    w.grounding.validate()?;
    match w.connection {
        WindingConnection::Delta
            if w.neutral_terminal || w.grounding != Grounding::NotApplicable =>
        {
            return invalid(
                "un triangle ne possède ni borne neutre ni mise à la terre du point étoile",
            );
        }
        WindingConnection::Star | WindingConnection::Zigzag
            if w.grounding == Grounding::NotApplicable =>
        {
            return invalid("préciser le régime du point étoile, ou le déclarer inconnu");
        }
        _ => {}
    }
    let mut required = vec![Conductor::A, Conductor::B, Conductor::C];
    if w.neutral_terminal {
        required.push(Conductor::N);
    }
    available(network, bus, &required)
}
impl ElectricalData {
    pub fn validate_for(&self, object: &Object, network: &Network) -> Result<()> {
        match (self, &object.equipment) {
            (Self::Bus(data), Equipment::Bus { .. }) => {
                conductors(&data.conductors)?;
                data.neutral_grounding.validate()?;
                if data.conductors.contains(&Conductor::N)
                    == (data.neutral_grounding == Grounding::NotApplicable)
                {
                    return invalid("mise à la terre du neutre incohérente avec la présence de N");
                }
            }
            (Self::Line(data), Equipment::Line { from, to, .. }) => {
                conductors(&data.conductors)?;
                matrix(&data.series_ohm_per_km, data.conductors.len())?;
                if let Some(shunt) = &data.shunt_siemens_per_km {
                    let converted: Vec<Vec<Impedance>> = shunt
                        .iter()
                        .map(|row| row.iter().map(|y| Impedance { r: y.g, x: y.b }).collect())
                        .collect();
                    matrix(&converted, data.conductors.len())?;
                }
                available(network, *from, &data.conductors)?;
                available(network, *to, &data.conductors)?;
            }
            (
                Self::Transformer(data),
                Equipment::Transformer {
                    from,
                    to,
                    rated_va,
                    uk_percent,
                },
            ) => {
                winding(&data.from, network, *from)?;
                winding(&data.to, network, *to)?;
                let parity =
                    |connection: WindingConnection| u8::from(connection != WindingConnection::Star);
                if data.clock % 2 != parity(data.from.connection) ^ parity(data.to.connection) {
                    return invalid(
                        "parité du groupe horaire incompatible avec les couplages déclarés",
                    );
                }
                if data.clock > 11 {
                    return invalid("groupe horaire : indice entre 0 et 11 requis");
                }
                data.series_pu(*rated_va, *uk_percent)?;
                if let Some(loss) = data.iron_loss_w {
                    nonnegative(loss, "pertes fer")?;
                }
                if let Some(current) = data.no_load_current_percent {
                    nonnegative(current, "courant à vide")?;
                }
                if let (Some(loss), Some(current)) =
                    (data.iron_loss_w, data.no_load_current_percent)
                    && loss > *rated_va * current / 100.0 + 1e-9
                {
                    return invalid("pertes fer supérieures à la puissance apparente à vide");
                }
                if let Some(tap) = &data.tap {
                    tap.validate()?;
                }
                positive(data.ratio(), "rapport de transformation")?;
                if let Some(z) = data.zero_sequence_ohm_referred_from {
                    z.validate()?;
                }
            }
            (Self::Source(data), Equipment::Source { .. }) => {
                data.neutral_grounding.validate()?;
                for z in [
                    data.positive_sequence_ohm,
                    data.negative_sequence_ohm,
                    data.zero_sequence_ohm,
                ]
                .into_iter()
                .flatten()
                {
                    z.validate()?;
                    if z.r == 0.0 && z.x == 0.0 {
                        return invalid(
                            "impédance interne de source nulle : donnée de défaut non exploitable",
                        );
                    }
                }
            }
            (
                Self::Injection(data),
                Equipment::Load { bus, phase, .. } | Equipment::Generator { bus, phase, .. },
            ) => {
                match data.connection {
                    InjectionConnection::PhaseNeutral {
                        phase: Conductor::N,
                    } => return invalid("N ne peut pas être la phase d'une charge"),
                    InjectionConnection::PhaseNeutral { phase: conductor } => {
                        let expected = match conductor {
                            Conductor::A => Phase::A,
                            Conductor::B => Phase::B,
                            Conductor::C => Phase::C,
                            Conductor::N => unreachable!(),
                        };
                        if *phase != expected {
                            return invalid(
                                "phase historique et connexion phase-neutre incohérentes",
                            );
                        }
                    }
                    InjectionConnection::PhasePhase { from, to }
                        if from == to || from == Conductor::N || to == Conductor::N =>
                    {
                        return invalid("connexion entre deux phases distinctes requise");
                    }
                    _ if *phase != Phase::Abc => {
                        return invalid(
                            "une connexion triphasée ou entre phases exige le repère historique Abc",
                        );
                    }
                    _ => {}
                }
                available(network, *bus, &data.connection.required())?;
                if let FaultContribution::CurrentLimited { max_current_a } = data.fault_contribution
                {
                    positive(max_current_a, "courant maximal de contribution au défaut")?;
                }
            }
            (Self::Switch { conductors: values }, Equipment::Switch { from, to, .. }) => {
                conductors(values)?;
                available(network, *from, values)?;
                available(network, *to, values)?;
            }
            _ => {
                return invalid(
                    "le modèle électrique détaillé ne correspond pas au type d'ouvrage",
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MissingData {
    pub object_id: Id,
    pub message: String,
}
pub fn missing_data(network: &Network) -> Vec<MissingData> {
    let mut missing = vec![];
    for object in network.objects.values() {
        let mut add = |message: &str| {
            missing.push(MissingData {
                object_id: object.id,
                message: message.into(),
            })
        };
        match &object.electrical {
            None => add("modèle électrique détaillé non renseigné"),
            Some(ElectricalData::Bus(b)) if b.neutral_grounding == Grounding::Unspecified => {
                add("mise à la terre du neutre non renseignée")
            }
            Some(ElectricalData::Transformer(t)) => {
                if t.from.grounding == Grounding::Unspecified
                    || t.to.grounding == Grounding::Unspecified
                {
                    add("mise à la terre d'un enroulement non renseignée");
                }
                if t.iron_loss_w.is_none() || t.no_load_current_percent.is_none() {
                    add("essai à vide incomplet");
                }
                if t.zero_sequence_ohm_referred_from.is_none() {
                    add("paramètres homopolaires de transformateur non renseignés");
                }
            }
            Some(ElectricalData::Source(s)) => {
                if s.positive_sequence_ohm.is_none()
                    || s.negative_sequence_ohm.is_none()
                    || s.zero_sequence_ohm.is_none()
                {
                    add("impédances de séquence de la source incomplètes");
                }
                if s.neutral_grounding == Grounding::Unspecified {
                    add("mise à la terre du neutre source non renseignée");
                }
            }
            Some(ElectricalData::Injection(i))
                if matches!(i.fault_contribution, FaultContribution::Unspecified) =>
            {
                add("contribution au défaut non renseignée")
            }
            _ => {}
        }
    }
    missing
}

impl LineData {
    /// Exact positive-sequence reduction only for a balanced, transposed primitive matrix.
    /// The neutral remains in the stored model; no neutral elimination is performed.
    pub fn balanced_impedance(&self) -> Result<Impedance> {
        conductors(&self.conductors)?;
        let indices = [Conductor::A, Conductor::B, Conductor::C]
            .map(|c| self.conductors.iter().position(|v| *v == c));
        let [Some(a), Some(b), Some(c)] = indices else {
            return invalid("la réduction équilibrée exige les trois phases A/B/C");
        };
        matrix(&self.series_ohm_per_km, self.conductors.len())?;
        if self
            .shunt_siemens_per_km
            .as_ref()
            .is_some_and(|m| m.iter().flatten().any(|z| z.g != 0.0 || z.b != 0.0))
        {
            return invalid(
                "les admittances shunt ne sont pas supportées par le moteur équilibré actuel",
            );
        }
        let diag = self.series_ohm_per_km[a][a];
        let mutual = self.series_ohm_per_km[a][b];
        let close = |left: Impedance, right: Impedance| {
            (left.r - right.r).abs() <= 1e-10 && (left.x - right.x).abs() <= 1e-10
        };
        for i in [a, b, c] {
            for j in [a, b, c] {
                if !close(
                    self.series_ohm_per_km[i][j],
                    if i == j { diag } else { mutual },
                ) {
                    return invalid(
                        "matrice de phases dissymétrique : un moteur déséquilibré est requis",
                    );
                }
            }
        }
        if let Some(n) = self.conductors.iter().position(|v| *v == Conductor::N) {
            for i in [b, c] {
                if !close(self.series_ohm_per_km[i][n], self.series_ohm_per_km[a][n]) {
                    return invalid(
                        "couplages phase-neutre dissymétriques : un moteur déséquilibré est requis",
                    );
                }
            }
        }
        let z = Impedance {
            r: (diag.r - mutual.r).max(0.0),
            x: diag.x - mutual.x,
        };
        z.validate()?;
        Ok(z)
    }
}

/// Generates an explicitly provisional editing template, not surveyed or manufacturer data.
pub fn editing_template(object: &Object, network: &Network) -> ElectricalData {
    let abc = vec![Conductor::A, Conductor::B, Conductor::C];
    let voltage = |id: Id| match network.objects.get(&id).map(|o| &o.equipment) {
        Some(Equipment::Bus { voltage_v }) => *voltage_v,
        _ => 1.0,
    };
    match object.equipment {
        Equipment::Bus { voltage_v } => {
            let mut conductors = abc;
            if voltage_v < 1000.0 {
                conductors.push(Conductor::N);
            }
            ElectricalData::Bus(BusData {
                neutral_grounding: if conductors.contains(&Conductor::N) {
                    Grounding::Unspecified
                } else {
                    Grounding::NotApplicable
                },
                conductors,
            })
        }
        Equipment::Line {
            r_ohm_per_km,
            x_ohm_per_km,
            ..
        } => {
            let mut matrix = vec![vec![Impedance::ZERO; 3]; 3];
            for (i, row) in matrix.iter_mut().enumerate() {
                row[i] = Impedance {
                    r: r_ohm_per_km,
                    x: x_ohm_per_km,
                };
            }
            ElectricalData::Line(LineData {
                conductors: abc,
                series_ohm_per_km: matrix,
                shunt_siemens_per_km: None,
            })
        }
        Equipment::Transformer { from, to, .. } => ElectricalData::Transformer(TransformerData {
            from: Winding {
                connection: WindingConnection::Star,
                rated_voltage_ll_v: voltage(from),
                neutral_terminal: false,
                grounding: Grounding::Unspecified,
            },
            to: Winding {
                connection: WindingConnection::Star,
                rated_voltage_ll_v: voltage(to),
                neutral_terminal: false,
                grounding: Grounding::Unspecified,
            },
            clock: 0,
            copper_loss_w: 0.0,
            iron_loss_w: None,
            no_load_current_percent: None,
            tap: None,
            zero_sequence_ohm_referred_from: None,
        }),
        Equipment::Source { .. } => ElectricalData::Source(SourceData {
            positive_sequence_ohm: None,
            negative_sequence_ohm: None,
            zero_sequence_ohm: None,
            neutral_grounding: Grounding::Unspecified,
        }),
        Equipment::Load { phase, .. } | Equipment::Generator { phase, .. } => {
            ElectricalData::Injection(InjectionData {
                connection: match phase {
                    Phase::Abc => InjectionConnection::ThreePhaseStar { neutral: false },
                    Phase::A => InjectionConnection::PhaseNeutral {
                        phase: Conductor::A,
                    },
                    Phase::B => InjectionConnection::PhaseNeutral {
                        phase: Conductor::B,
                    },
                    Phase::C => InjectionConnection::PhaseNeutral {
                        phase: Conductor::C,
                    },
                },
                fault_contribution: FaultContribution::Unspecified,
            })
        }
        Equipment::Switch { .. } => ElectricalData::Switch { conductors: abc },
    }
}
