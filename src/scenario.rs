//! Compact, shareable operating scenarios applied without mutating a project version.
use crate::model::{Equipment, Error, Id, Network, Result};
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioKind {
    MaximumConsumption,
    MaximumGeneration,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InjectionOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_service: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p_w: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q_var: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: Id,
    pub name: String,
    pub kind: ScenarioKind,
    pub load_scale: f64,
    pub generation_scale: f64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub injections: BTreeMap<Id, InjectionOverride>,
}

impl Scenario {
    pub fn validate(&self, network: &Network) -> Result<()> {
        if self.id.is_nil() || self.name.trim().is_empty() {
            return Err(Error::Invalid(
                "identifiant ou nom de scénario invalide".into(),
            ));
        }
        for (name, value) in [
            ("coefficient des charges", self.load_scale),
            ("coefficient des productions", self.generation_scale),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(Error::Invalid(format!(
                    "{name} doit être fini et positif ou nul"
                )));
            }
        }
        for (id, values) in &self.injections {
            let Some(object) = network.objects.get(id) else {
                return Err(Error::Invalid(format!(
                    "scénario {} : injection {id} absente",
                    self.name
                )));
            };
            if !matches!(
                object.equipment,
                Equipment::Load { .. } | Equipment::Generator { .. }
            ) {
                return Err(Error::Invalid(format!(
                    "{} : seuls les charges et producteurs peuvent être surchargés par un scénario",
                    object.name
                )));
            }
            for (name, value) in [("P", values.p_w), ("Q", values.q_var)] {
                if value.is_some_and(|value| !value.is_finite() || (name == "P" && value < 0.0)) {
                    return Err(Error::Invalid(format!(
                        "{} : puissance {name} de scénario invalide",
                        object.name
                    )));
                }
            }
        }
        Ok(())
    }

    /// Returns a study copy. The base project and its version remain unchanged.
    pub fn apply(&self, network: &Network) -> Result<Network> {
        self.validate(network)?;
        let mut result = network.clone();
        for object in result.objects.values_mut() {
            let scale = match object.equipment {
                Equipment::Load { .. } => self.load_scale,
                Equipment::Generator { .. } => self.generation_scale,
                _ => continue,
            };
            match &mut object.equipment {
                Equipment::Load { p_w, q_var, .. } | Equipment::Generator { p_w, q_var, .. } => {
                    *p_w *= scale;
                    *q_var *= scale;
                    if let Some(values) = self.injections.get(&object.id) {
                        object.in_service = values.in_service.unwrap_or(object.in_service);
                        *p_w = values.p_w.unwrap_or(*p_w);
                        *q_var = values.q_var.unwrap_or(*q_var);
                    }
                }
                _ => unreachable!(),
            }
        }
        result.validate()?;
        Ok(result)
    }
}

pub const COLLECTION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioCollection {
    pub schema_version: u32,
    pub project_id: Id,
    pub network_version: Id,
    pub scenarios: Vec<Scenario>,
}

impl ScenarioCollection {
    pub fn new(project: &Project, scenarios: Vec<Scenario>) -> Result<Self> {
        let collection = Self {
            schema_version: COLLECTION_SCHEMA_VERSION,
            project_id: project.id,
            network_version: project.head(),
            scenarios,
        };
        collection.validate(project)?;
        Ok(collection)
    }

    pub fn validate(&self, project: &Project) -> Result<()> {
        if self.schema_version != COLLECTION_SCHEMA_VERSION {
            return Err(Error::Invalid(format!(
                "schéma de collection de scénarios {} non supporté",
                self.schema_version
            )));
        }
        if self.project_id != project.id || self.network_version != project.head() {
            return Err(Error::Conflict(
                "la collection de scénarios cible un autre projet ou une autre version du réseau"
                    .into(),
            ));
        }
        let network = project.network()?;
        let mut identifiers = std::collections::BTreeSet::new();
        for scenario in &self.scenarios {
            if !identifiers.insert(scenario.id) {
                return Err(Error::Invalid(format!(
                    "identifiant de scénario réutilisé : {}",
                    scenario.id
                )));
            }
            scenario.validate(&network)?;
        }
        Ok(())
    }

    pub fn from_json(project: &Project, json: &str) -> Result<Self> {
        let collection: Self = serde_json::from_str(json)?;
        collection.validate(project)?;
        Ok(collection)
    }

    pub fn to_json(&self, project: &Project) -> Result<String> {
        self.validate(project)?;
        Ok(serde_json::to_string(self)?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioStudy<T, O> {
    pub project_id: Id,
    pub network_version: Id,
    pub scenario: Scenario,
    pub solver_version: String,
    pub options: O,
    pub result: T,
}
