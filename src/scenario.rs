//! Compact, shareable operating scenarios applied without mutating a project version.
use crate::project::Project;
use crate::{
    electrical::{ElectricalData, Impedance},
    model::{Equipment, Error, Id, Network, Result},
};
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

/// Sparse operating-state changes kept outside the versioned network.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_service: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tap_position: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceOverride>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_pu: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positive_sequence_ohm: Option<Impedance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_sequence_ohm: Option<Impedance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zero_sequence_ohm: Option<Impedance>,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub equipment: BTreeMap<Id, EquipmentOverride>,
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
        for (id, values) in &self.equipment {
            let Some(object) = network.objects.get(id) else {
                return Err(Error::Invalid(format!(
                    "scénario {} : ouvrage {id} absent",
                    self.name
                )));
            };
            if matches!(
                object.equipment,
                Equipment::Load { .. } | Equipment::Generator { .. }
            ) {
                return Err(Error::Invalid(format!(
                    "{} : l'état des injections se configure dans la table des charges et productions",
                    object.name
                )));
            }
            if matches!(object.equipment, Equipment::Switch { .. }) {
                return Err(Error::Invalid(format!(
                    "{} : l'état ouvert/fermé d'un interrupteur appartient au réseau versionné, pas au scénario",
                    object.name
                )));
            }
            if values.in_service.is_none()
                && values.tap_position.is_none()
                && values.source.is_none()
            {
                return Err(Error::Invalid(format!(
                    "{} : modification d'exploitation vide",
                    object.name
                )));
            }
            if let Some(position) = values.tap_position {
                let Some(ElectricalData::Transformer(data)) = &object.electrical else {
                    return Err(Error::Invalid(format!(
                        "{} : données détaillées de transformateur requises pour changer la prise",
                        object.name
                    )));
                };
                let Some(tap) = &data.tap else {
                    return Err(Error::Invalid(format!(
                        "{} : transformateur sans régleur déclaré",
                        object.name
                    )));
                };
                if !(tap.min_position..=tap.max_position).contains(&position) {
                    return Err(Error::Invalid(format!(
                        "{} : prise {position} hors de [{}, {}]",
                        object.name, tap.min_position, tap.max_position
                    )));
                }
            }
            if let Some(source) = &values.source {
                let Equipment::Source { .. } = object.equipment else {
                    return Err(Error::Invalid(format!(
                        "{} : les hypothèses de source ne s'appliquent qu'à une source",
                        object.name
                    )));
                };
                if source.voltage_pu.is_none()
                    && source.positive_sequence_ohm.is_none()
                    && source.negative_sequence_ohm.is_none()
                    && source.zero_sequence_ohm.is_none()
                {
                    return Err(Error::Invalid(format!(
                        "{} : hypothèse de source vide",
                        object.name
                    )));
                }
                if source
                    .voltage_pu
                    .is_some_and(|value| !value.is_finite() || value <= 0.0 || value > 1.2)
                {
                    return Err(Error::Invalid(format!(
                        "{} : tension de source en pu invalide",
                        object.name
                    )));
                }
                for impedance in [
                    source.positive_sequence_ohm,
                    source.negative_sequence_ohm,
                    source.zero_sequence_ohm,
                ]
                .into_iter()
                .flatten()
                {
                    impedance.validate()?;
                }
                if (source.positive_sequence_ohm.is_some()
                    || source.negative_sequence_ohm.is_some()
                    || source.zero_sequence_ohm.is_some())
                    && !matches!(object.electrical, Some(ElectricalData::Source(_)))
                {
                    return Err(Error::Invalid(format!(
                        "{} : données détaillées requises pour surcharger les impédances de source",
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
        for (id, values) in &self.equipment {
            let object = result
                .objects
                .get_mut(id)
                .expect("validated operating-state object exists");
            if let Some(in_service) = values.in_service {
                object.in_service = in_service;
            }
            if let Some(position) = values.tap_position {
                let Some(ElectricalData::Transformer(data)) = &mut object.electrical else {
                    unreachable!("validated transformer tap override")
                };
                data.tap.as_mut().expect("validated tap changer").position = position;
            }
            if let Some(source) = &values.source {
                if let Some(voltage_pu) = source.voltage_pu {
                    let Equipment::Source {
                        voltage_pu: current,
                        ..
                    } = &mut object.equipment
                    else {
                        unreachable!("validated source override")
                    };
                    *current = voltage_pu;
                }
                if source.positive_sequence_ohm.is_some()
                    || source.negative_sequence_ohm.is_some()
                    || source.zero_sequence_ohm.is_some()
                {
                    let Some(ElectricalData::Source(data)) = &mut object.electrical else {
                        unreachable!("validated detailed source override")
                    };
                    if let Some(value) = source.positive_sequence_ohm {
                        data.positive_sequence_ohm = Some(value);
                    }
                    if let Some(value) = source.negative_sequence_ohm {
                        data.negative_sequence_ohm = Some(value);
                    }
                    if let Some(value) = source.zero_sequence_ohm {
                        data.zero_sequence_ohm = Some(value);
                    }
                }
            }
        }
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
