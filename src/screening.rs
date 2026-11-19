//! Batch scenario screening and dimensioning envelopes.
use crate::{
    load_flow,
    model::{Error, Id, Result},
    project::Project,
    scenario::Scenario,
    unbalanced,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};

pub const SCREENING_VERSION: &str = "scenario-screening-0.1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreeningMode {
    Balanced,
    Unbalanced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreeningStatus {
    Compliant,
    Constrained,
    Unable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    MinimumVoltage,
    MaximumVoltage,
    MaximumLoading,
    NegativeSequence,
    NeutralDisplacement,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub min_voltage_pu: f64,
    pub max_voltage_pu: f64,
    pub max_loading_percent: f64,
    pub max_negative_sequence_percent: Option<f64>,
    pub max_neutral_displacement_v: Option<f64>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            min_voltage_pu: 0.9,
            max_voltage_pu: 1.1,
            max_loading_percent: 100.0,
            max_negative_sequence_percent: None,
            max_neutral_displacement_v: None,
        }
    }
}

impl Limits {
    pub fn validate(&self) -> Result<()> {
        if !self.min_voltage_pu.is_finite()
            || !self.max_voltage_pu.is_finite()
            || self.min_voltage_pu <= 0.0
            || self.max_voltage_pu <= self.min_voltage_pu
            || !self.max_loading_percent.is_finite()
            || self.max_loading_percent <= 0.0
            || self
                .max_negative_sequence_percent
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
            || self
                .max_neutral_displacement_v
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
        {
            return Err(Error::Invalid(
                "limites de screening finies, positives et ordonnées requises".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    pub object_id: Id,
    pub metric: MetricKind,
    pub value: f64,
    pub limit: f64,
    /// Positive or zero means compliant, negative means constrained.
    pub margin: f64,
    pub unit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSummary {
    pub scenario_id: Option<Id>,
    pub scenario_name: String,
    pub status: ScreeningStatus,
    pub constraints: Vec<Constraint>,
    pub error: Option<String>,
    pub minimum_voltage_pu: Option<f64>,
    pub maximum_voltage_pu: Option<f64>,
    pub maximum_loading_percent: Option<f64>,
    pub maximum_negative_sequence_percent: Option<f64>,
    pub maximum_neutral_displacement_v: Option<f64>,
    pub total_loss_w: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DimensioningCase {
    pub object_id: Id,
    pub metric: MetricKind,
    pub value: f64,
    pub limit: Option<f64>,
    pub margin: Option<f64>,
    pub unit: String,
    pub scenario_id: Option<Id>,
    pub scenario_name: String,
    pub constrained: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreeningReport {
    pub project_id: Id,
    pub network_version: Id,
    pub screening_version: String,
    pub solver_version: String,
    pub mode: ScreeningMode,
    pub limits: Limits,
    pub include_base_case: bool,
    pub status: ScreeningStatus,
    pub scenarios: Vec<ScenarioSummary>,
    pub dimensioning_cases: Vec<DimensioningCase>,
}

struct CaseBuilder {
    scenario_id: Option<Id>,
    scenario_name: String,
    constraints: Vec<Constraint>,
    minimum_voltage_pu: Option<f64>,
    maximum_voltage_pu: Option<f64>,
    maximum_loading_percent: Option<f64>,
    maximum_negative_sequence_percent: Option<f64>,
    maximum_neutral_displacement_v: Option<f64>,
    total_loss_w: f64,
}

impl CaseBuilder {
    fn new(scenario: Option<&Scenario>, total_loss_w: f64) -> Self {
        Self {
            scenario_id: scenario.map(|value| value.id),
            scenario_name: scenario
                .map(|value| value.name.clone())
                .unwrap_or_else(|| "Réseau de base".into()),
            constraints: vec![],
            minimum_voltage_pu: None,
            maximum_voltage_pu: None,
            maximum_loading_percent: None,
            maximum_negative_sequence_percent: None,
            maximum_neutral_displacement_v: None,
            total_loss_w,
        }
    }

    fn measure(
        &mut self,
        envelope: &mut BTreeMap<(Id, MetricKind), DimensioningCase>,
        object_id: Id,
        metric: MetricKind,
        value: f64,
        limit: Option<f64>,
        unit: &str,
    ) {
        let margin = limit.map(|limit| match metric {
            MetricKind::MinimumVoltage => value - limit,
            _ => limit - value,
        });
        if margin.is_some_and(|margin| margin < 0.0) {
            let constraint = Constraint {
                object_id,
                metric,
                value,
                limit: limit.expect("constraint has a limit"),
                margin: margin.expect("constraint has a margin"),
                unit: unit.into(),
            };
            if let Some(current) = self
                .constraints
                .iter_mut()
                .find(|current| current.object_id == object_id && current.metric == metric)
            {
                let worse = match metric {
                    MetricKind::MinimumVoltage => constraint.value < current.value,
                    _ => constraint.value > current.value,
                };
                if worse {
                    *current = constraint;
                }
            } else {
                self.constraints.push(constraint);
            }
        }
        let candidate = DimensioningCase {
            object_id,
            metric,
            value,
            limit,
            margin,
            unit: unit.into(),
            scenario_id: self.scenario_id,
            scenario_name: self.scenario_name.clone(),
            constrained: margin.is_some_and(|margin| margin < 0.0),
        };
        envelope
            .entry((object_id, metric))
            .and_modify(|current| {
                let worse = match metric {
                    MetricKind::MinimumVoltage => candidate.value < current.value,
                    _ => candidate.value > current.value,
                };
                if worse {
                    *current = candidate.clone();
                }
            })
            .or_insert(candidate);
    }

    fn finish(self) -> ScenarioSummary {
        ScenarioSummary {
            scenario_id: self.scenario_id,
            scenario_name: self.scenario_name,
            status: if self.constraints.is_empty() {
                ScreeningStatus::Compliant
            } else {
                ScreeningStatus::Constrained
            },
            constraints: self.constraints,
            error: None,
            minimum_voltage_pu: self.minimum_voltage_pu,
            maximum_voltage_pu: self.maximum_voltage_pu,
            maximum_loading_percent: self.maximum_loading_percent,
            maximum_negative_sequence_percent: self.maximum_negative_sequence_percent,
            maximum_neutral_displacement_v: self.maximum_neutral_displacement_v,
            total_loss_w: Some(self.total_loss_w),
        }
    }
}

fn scenarios(values: &[Scenario], include_base: bool) -> Vec<Option<&Scenario>> {
    let mut result = Vec::with_capacity(values.len() + usize::from(include_base));
    if include_base {
        result.push(None);
    }
    result.extend(values.iter().map(Some));
    result
}

fn unable(scenario: Option<&Scenario>, error: impl ToString) -> ScenarioSummary {
    ScenarioSummary {
        scenario_id: scenario.map(|value| value.id),
        scenario_name: scenario
            .map(|value| value.name.clone())
            .unwrap_or_else(|| "Réseau de base".into()),
        status: ScreeningStatus::Unable,
        constraints: vec![],
        error: Some(error.to_string()),
        minimum_voltage_pu: None,
        maximum_voltage_pu: None,
        maximum_loading_percent: None,
        maximum_negative_sequence_percent: None,
        maximum_neutral_displacement_v: None,
        total_loss_w: None,
    }
}

fn overall(scenarios: &[ScenarioSummary]) -> ScreeningStatus {
    if scenarios
        .iter()
        .any(|scenario| scenario.status == ScreeningStatus::Unable)
    {
        ScreeningStatus::Unable
    } else if scenarios
        .iter()
        .any(|scenario| scenario.status == ScreeningStatus::Constrained)
    {
        ScreeningStatus::Constrained
    } else {
        ScreeningStatus::Compliant
    }
}

pub fn screen_balanced(
    project: &Project,
    scenario_values: &[Scenario],
    include_base_case: bool,
    solver_options: &load_flow::Options,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<ScreeningReport> {
    limits.validate()?;
    let base = project.network()?;
    let mut summaries = vec![];
    let mut envelope = BTreeMap::new();
    for scenario in scenarios(scenario_values, include_base_case) {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Invalid("screening annulé".into()));
        }
        let network = match scenario.map_or_else(|| Ok(base.clone()), |value| value.apply(&base)) {
            Ok(network) => network,
            Err(error) => {
                summaries.push(unable(scenario, error));
                continue;
            }
        };
        let result = match load_flow::solve_cancellable(&network, solver_options, cancel) {
            Ok(result) => result,
            Err(error) => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Invalid("screening annulé".into()));
                }
                summaries.push(unable(scenario, error));
                continue;
            }
        };
        let mut builder = CaseBuilder::new(scenario, result.total_loss_w);
        for bus in &result.buses {
            builder.minimum_voltage_pu = Some(
                builder
                    .minimum_voltage_pu
                    .map_or(bus.voltage_pu, |value| value.min(bus.voltage_pu)),
            );
            builder.maximum_voltage_pu = Some(
                builder
                    .maximum_voltage_pu
                    .map_or(bus.voltage_pu, |value| value.max(bus.voltage_pu)),
            );
            builder.measure(
                &mut envelope,
                bus.id,
                MetricKind::MinimumVoltage,
                bus.voltage_pu,
                Some(limits.min_voltage_pu),
                "pu",
            );
            builder.measure(
                &mut envelope,
                bus.id,
                MetricKind::MaximumVoltage,
                bus.voltage_pu,
                Some(limits.max_voltage_pu),
                "pu",
            );
        }
        for branch in &result.branches {
            if let Some(loading) = branch.loading_percent {
                builder.maximum_loading_percent = Some(
                    builder
                        .maximum_loading_percent
                        .map_or(loading, |value| value.max(loading)),
                );
                builder.measure(
                    &mut envelope,
                    branch.id,
                    MetricKind::MaximumLoading,
                    loading,
                    Some(limits.max_loading_percent),
                    "%",
                );
            }
        }
        summaries.push(builder.finish());
    }
    if summaries.is_empty() {
        return Err(Error::Invalid(
            "screening sans réseau de base ni scénario".into(),
        ));
    }
    Ok(ScreeningReport {
        project_id: project.id,
        network_version: project.head(),
        screening_version: SCREENING_VERSION.into(),
        solver_version: load_flow::SOLVER_VERSION.into(),
        mode: ScreeningMode::Balanced,
        limits: limits.clone(),
        include_base_case,
        status: overall(&summaries),
        scenarios: summaries,
        dimensioning_cases: envelope.into_values().collect(),
    })
}

pub fn screen_unbalanced(
    project: &Project,
    scenario_values: &[Scenario],
    include_base_case: bool,
    solver_options: &unbalanced::Options,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<ScreeningReport> {
    limits.validate()?;
    let base = project.network()?;
    let mut summaries = vec![];
    let mut envelope = BTreeMap::new();
    for scenario in scenarios(scenario_values, include_base_case) {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Invalid("screening annulé".into()));
        }
        let network = match scenario.map_or_else(|| Ok(base.clone()), |value| value.apply(&base)) {
            Ok(network) => network,
            Err(error) => {
                summaries.push(unable(scenario, error));
                continue;
            }
        };
        let result = match unbalanced::solve_cancellable(&network, solver_options, cancel) {
            Ok(result) => result,
            Err(error) => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Invalid("screening annulé".into()));
                }
                summaries.push(unable(scenario, error));
                continue;
            }
        };
        let mut builder = CaseBuilder::new(scenario, result.total_loss_w);
        for bus in &result.buses {
            for voltage in &bus.phase_neutral {
                builder.minimum_voltage_pu = Some(
                    builder
                        .minimum_voltage_pu
                        .map_or(voltage.pu, |value| value.min(voltage.pu)),
                );
                builder.maximum_voltage_pu = Some(
                    builder
                        .maximum_voltage_pu
                        .map_or(voltage.pu, |value| value.max(voltage.pu)),
                );
                builder.measure(
                    &mut envelope,
                    bus.id,
                    MetricKind::MinimumVoltage,
                    voltage.pu,
                    Some(limits.min_voltage_pu),
                    "pu",
                );
                builder.measure(
                    &mut envelope,
                    bus.id,
                    MetricKind::MaximumVoltage,
                    voltage.pu,
                    Some(limits.max_voltage_pu),
                    "pu",
                );
            }
            builder.maximum_negative_sequence_percent = Some(
                builder
                    .maximum_negative_sequence_percent
                    .map_or(bus.negative_sequence_percent, |value| {
                        value.max(bus.negative_sequence_percent)
                    }),
            );
            builder.maximum_neutral_displacement_v = Some(
                builder
                    .maximum_neutral_displacement_v
                    .map_or(bus.neutral_displacement_v, |value| {
                        value.max(bus.neutral_displacement_v)
                    }),
            );
            builder.measure(
                &mut envelope,
                bus.id,
                MetricKind::NegativeSequence,
                bus.negative_sequence_percent,
                limits.max_negative_sequence_percent,
                "%",
            );
            builder.measure(
                &mut envelope,
                bus.id,
                MetricKind::NeutralDisplacement,
                bus.neutral_displacement_v,
                limits.max_neutral_displacement_v,
                "V",
            );
        }
        for branch in &result.branches {
            if let Some(loading) = branch.loading_percent {
                builder.maximum_loading_percent = Some(
                    builder
                        .maximum_loading_percent
                        .map_or(loading, |value| value.max(loading)),
                );
                builder.measure(
                    &mut envelope,
                    branch.id,
                    MetricKind::MaximumLoading,
                    loading,
                    Some(limits.max_loading_percent),
                    "%",
                );
            }
        }
        summaries.push(builder.finish());
    }
    if summaries.is_empty() {
        return Err(Error::Invalid(
            "screening sans réseau de base ni scénario".into(),
        ));
    }
    Ok(ScreeningReport {
        project_id: project.id,
        network_version: project.head(),
        screening_version: SCREENING_VERSION.into(),
        solver_version: unbalanced::SOLVER_VERSION.into(),
        mode: ScreeningMode::Unbalanced,
        limits: limits.clone(),
        include_base_case,
        status: overall(&summaries),
        scenarios: summaries,
        dimensioning_cases: envelope.into_values().collect(),
    })
}
