//! Persistent result envelopes and human-readable screening exports.
use crate::{
    model::{Error, Id, Network, Result},
    project::Project,
    screening::{MetricKind, ScreeningReport, ScreeningStatus},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudyKind {
    BalancedLoadFlow,
    UnbalancedLoadFlow,
    ScenarioScreening,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredResult {
    pub id: Id,
    pub name: String,
    pub project_id: Id,
    pub network_version: Id,
    pub kind: StudyKind,
    pub solver_version: String,
    pub payload: Value,
}

impl StoredResult {
    pub fn new<T: Serialize>(
        name: &str,
        project_id: Id,
        network_version: Id,
        kind: StudyKind,
        solver_version: &str,
        payload: &T,
    ) -> Result<Self> {
        let result = Self {
            id: Uuid::new_v4(),
            name: name.into(),
            project_id,
            network_version,
            kind,
            solver_version: solver_version.into(),
            payload: serde_json::to_value(payload)?,
        };
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<()> {
        if self.id.is_nil()
            || self.project_id.is_nil()
            || self.network_version.is_nil()
            || self.name.trim().is_empty()
            || self.solver_version.trim().is_empty()
            || !self.payload.is_object()
        {
            return Err(Error::Invalid("enveloppe de résultat invalide".into()));
        }
        Ok(())
    }

    pub fn validate_for(&self, project: &Project) -> Result<()> {
        self.validate()?;
        if self.project_id != project.id {
            return Err(Error::Conflict("résultat lié à un autre projet".into()));
        }
        let known = self.network_version == project.root_version
            || project
                .changes
                .iter()
                .any(|change| change.version == self.network_version);
        if !known {
            return Err(Error::Conflict(
                "résultat lié à une version inconnue du projet".into(),
            ));
        }
        Ok(())
    }

    pub fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        Ok(serde_json::from_value(self.payload.clone())?)
    }
}

fn status(value: ScreeningStatus) -> &'static str {
    match value {
        ScreeningStatus::Compliant => "conforme",
        ScreeningStatus::Constrained => "contrainte",
        ScreeningStatus::Unable => "non_calculable",
    }
}

fn metric(value: MetricKind) -> &'static str {
    match value {
        MetricKind::MinimumVoltage => "tension_minimale",
        MetricKind::MaximumVoltage => "tension_maximale",
        MetricKind::MaximumLoading => "charge_maximale",
        MetricKind::NegativeSequence => "desequilibre_inverse",
        MetricKind::NeutralDisplacement => "deplacement_neutre",
        MetricKind::MinimumShortCircuit => "iccb_minimal",
        MetricKind::MinimumResidualDetection => "residuel_detection_minimal",
        MetricKind::MaximumResidualCurrent => "courant_residuel_maximal",
    }
}

fn csv(value: &str) -> String {
    if value.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.into()
    }
}

/// Semicolon-delimited UTF-8, convenient for French spreadsheet defaults.
pub fn screening_csv(report: &ScreeningReport, network: &Network) -> String {
    let mut rows = vec![
        "section;situation;verdict;object_id;ouvrage;grandeur;valeur;unite;limite;marge;contrainte"
            .into(),
    ];
    for scenario in &report.scenarios {
        rows.push(format!(
            "situation;{};{};;;;{};pu;;;{}",
            csv(&scenario.scenario_name),
            status(scenario.status),
            scenario
                .minimum_voltage_pu
                .map_or_else(String::new, |value| value.to_string()),
            !scenario.constraints.is_empty()
        ));
        for constraint in &scenario.constraints {
            let name = network
                .objects
                .get(&constraint.object_id)
                .map_or("ouvrage inconnu", |object| object.name.as_str());
            rows.push(format!(
                "contrainte;{};{};{};{};{};{};{};{};{};true",
                csv(&scenario.scenario_name),
                status(scenario.status),
                constraint.object_id,
                csv(name),
                metric(constraint.metric),
                constraint.value,
                csv(&constraint.unit),
                constraint.limit,
                constraint.margin
            ));
        }
    }
    for case in &report.dimensioning_cases {
        let name = network
            .objects
            .get(&case.object_id)
            .map_or("ouvrage inconnu", |object| object.name.as_str());
        rows.push(format!(
            "dimensionnant;{};;{};{};{};{};{};{};{};{}",
            csv(&case.scenario_name),
            case.object_id,
            csv(name),
            metric(case.metric),
            case.value,
            csv(&case.unit),
            case.limit
                .map_or_else(String::new, |value| value.to_string()),
            case.margin
                .map_or_else(String::new, |value| value.to_string()),
            case.constrained
        ));
    }
    rows.join("\n") + "\n"
}

fn md(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

pub fn screening_markdown(report: &ScreeningReport, network: &Network) -> String {
    let mut out = format!(
        "# Rapport de vérification du réseau\n\n- Projet : `{}`\n- Version du réseau : `{}`\n- Moteur : `{}` / `{}`\n- Verdict global : **{}**\n- Limites : {:.4} ≤ U ≤ {:.4} pu ; charge ≤ {:.2} %\n\n## Situations\n\n| Situation | Verdict | U min (pu) | U max (pu) | Charge max (%) | Contraintes |\n|---|---:|---:|---:|---:|---:|\n",
        report.project_id,
        report.network_version,
        report.screening_version,
        report.solver_version,
        status(report.status),
        report.limits.min_voltage_pu,
        report.limits.max_voltage_pu,
        report.limits.max_loading_percent,
    );
    for scenario in &report.scenarios {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            md(&scenario.scenario_name),
            status(scenario.status),
            scenario
                .minimum_voltage_pu
                .map_or_else(|| "—".into(), |v| format!("{v:.4}")),
            scenario
                .maximum_voltage_pu
                .map_or_else(|| "—".into(), |v| format!("{v:.4}")),
            scenario
                .maximum_loading_percent
                .map_or_else(|| "—".into(), |v| format!("{v:.2}")),
            scenario.constraints.len(),
        ));
    }
    out.push_str("\n## Situations dimensionnantes\n\n| Ouvrage | Grandeur | Valeur | Limite | Marge | Situation | Contrainte |\n|---|---|---:|---:|---:|---|---:|\n");
    for case in &report.dimensioning_cases {
        let name = network
            .objects
            .get(&case.object_id)
            .map_or("ouvrage inconnu", |object| object.name.as_str());
        out.push_str(&format!(
            "| {} | {} | {:.4} {} | {} | {} | {} | {} |\n",
            md(name),
            metric(case.metric),
            case.value,
            md(&case.unit),
            case.limit.map_or_else(|| "—".into(), |v| format!("{v:.4}")),
            case.margin
                .map_or_else(|| "—".into(), |v| format!("{v:+.4}")),
            md(&case.scenario_name),
            if case.constrained { "oui" } else { "non" },
        ));
    }
    if let Some(fault) = &report.fault_study {
        out.push_str(&format!(
            "\n## Protections\n\nVerdict IccBmin / 3I0 : **{}** ; moteur `{}` ; {} situation(s) calculée(s).\n",
            status(fault.status),
            fault.solver_version,
            fault.cases.len(),
        ));
    }
    out
}
