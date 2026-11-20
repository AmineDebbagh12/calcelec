use crate::geometry::Geometry;
use crate::model::{Equipment, Error, Id, Network, Object, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Create {
        object: Object,
    },
    Delete {
        id: Id,
    },
    Rename {
        id: Id,
        name: String,
    },
    SetService {
        id: Id,
        in_service: bool,
    },
    SetPower {
        id: Id,
        p_w: f64,
        q_var: f64,
    },
    SetSwitch {
        id: Id,
        closed: bool,
    },
    SetGeometry {
        id: Id,
        geometry: Geometry,
    },
    SetEquipment {
        id: Id,
        equipment: Equipment,
    },
    SetElectrical {
        id: Id,
        electrical: Option<crate::electrical::ElectricalData>,
    },
    PutTechnology {
        technology: crate::catalog::Technology,
    },
    RemoveTechnology {
        id: Id,
    },
    SetTechnology {
        id: Id,
        technology: Option<crate::catalog::TechnologyBinding>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeSet {
    pub schema_version: u32,
    pub project_id: Id,
    pub id: Id,
    pub base_version: Id,
    pub version: Id,
    pub author: String,
    pub message: String,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub schema_version: u32,
    pub id: Id,
    pub root_version: Id,
    pub root: Network,
    pub changes: Vec<ChangeSet>,
}

impl ChangeSet {
    pub(crate) fn migrate(mut self) -> Result<Self> {
        if self.schema_version < 5
            && self.operations.iter().any(|op| {
                matches!(
                    op,
                    Operation::PutTechnology { .. }
                        | Operation::RemoveTechnology { .. }
                        | Operation::SetTechnology { .. }
                )
            })
        {
            return Err(Error::Invalid(
                "les technologies exigent le schéma 5".into(),
            ));
        }
        if self.schema_version < 4
            && self.operations.iter().any(|op| {
                matches!(op, Operation::SetElectrical { .. })
                    || matches!(op,Operation::Create {object} if object.electrical.is_some())
            })
        {
            return Err(Error::Invalid(
                "les modèles détaillés exigent le schéma 4".into(),
            ));
        }
        if self.schema_version < 3
            && self
                .operations
                .iter()
                .any(|op| matches!(op, Operation::SetEquipment { .. }))
        {
            return Err(Error::Invalid("set_equipment exige le schéma 3".into()));
        }
        if self.schema_version == 1 {
            if self.operations.iter().any(|op| matches!(op, Operation::SetGeometry { .. }) || matches!(op, Operation::Create { object } if object.geometry != Geometry::Unlocated)) {
                return Err(Error::Invalid("géométrie interdite dans un lot de schéma 1".into()));
            }
            self.schema_version = SCHEMA_VERSION;
        }
        if matches!(self.schema_version, 2..=4) {
            self.schema_version = SCHEMA_VERSION;
        }
        supported(self.schema_version)?;
        Ok(self)
    }
}

fn supported(version: u32) -> Result<()> {
    if version != SCHEMA_VERSION {
        return Err(Error::Invalid(format!("schéma {version} non supporté")));
    }
    Ok(())
}

impl Project {
    /// Upgrade representation without changing object, batch or network-version IDs.
    pub(crate) fn migrate(mut self) -> Result<Self> {
        if self.schema_version < 5
            && (!self.root.technologies.is_empty()
                || self
                    .root
                    .objects
                    .values()
                    .any(|object| object.technology.is_some()))
        {
            return Err(Error::Invalid(
                "les technologies exigent le schéma 5".into(),
            ));
        }
        if self.schema_version < 4
            && self
                .root
                .objects
                .values()
                .any(|object| object.electrical.is_some())
        {
            return Err(Error::Invalid(
                "les modèles détaillés exigent le schéma 4".into(),
            ));
        }
        if self.schema_version == 1 {
            if self.root.coordinate_reference != crate::geometry::CoordinateReference::LocalMeters
                || self
                    .root
                    .objects
                    .values()
                    .any(|o| o.geometry != Geometry::Unlocated)
            {
                return Err(Error::Invalid(
                    "géométrie interdite dans un projet de schéma 1".into(),
                ));
            }
            self.schema_version = SCHEMA_VERSION;
        }
        if matches!(self.schema_version, 2..=4) {
            self.schema_version = SCHEMA_VERSION;
        }
        supported(self.schema_version)?;
        self.changes = self
            .changes
            .into_iter()
            .map(ChangeSet::migrate)
            .collect::<Result<_>>()?;
        self.network()?;
        Ok(self)
    }
    pub fn new(root: Network) -> Result<Self> {
        root.validate()?;
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            id: Uuid::new_v4(),
            root_version: Uuid::new_v4(),
            root,
            changes: vec![],
        })
    }
    pub fn head(&self) -> Id {
        self.changes.last().map_or(self.root_version, |c| c.version)
    }
    pub fn network(&self) -> Result<Network> {
        supported(self.schema_version)?;
        if self.id.is_nil() || self.root_version.is_nil() {
            return Err(Error::Invalid(
                "identifiant de projet ou de version vide".into(),
            ));
        }
        self.root.validate()?;
        let mut network = self.root.clone();
        let mut head = self.root_version;
        let mut versions = std::collections::BTreeSet::from([head]);
        let mut batches = std::collections::BTreeSet::new();
        for change in &self.changes {
            if !batches.insert(change.id) || !versions.insert(change.version) {
                return Err(Error::Invalid("identifiant d'historique réutilisé".into()));
            }
            check_change(change, self.id, head)?;
            apply_operations(&mut network, &change.operations)?;
            head = change.version;
        }
        Ok(network)
    }
    pub fn prepare(&self, author: &str, message: &str, operations: Vec<Operation>) -> ChangeSet {
        ChangeSet {
            schema_version: SCHEMA_VERSION,
            project_id: self.id,
            id: Uuid::new_v4(),
            base_version: self.head(),
            version: Uuid::new_v4(),
            author: author.into(),
            message: message.into(),
            operations,
        }
    }
    /// Returns false for an identical batch already received. On failure self is unchanged.
    pub fn apply(&mut self, change: ChangeSet) -> Result<bool> {
        let change = change.migrate()?;
        let mut network = self.network()?;
        if let Some(existing) = self.changes.iter().find(|c| c.id == change.id) {
            return if existing == &change {
                Ok(false)
            } else {
                Err(Error::Conflict(
                    "identifiant de lot réutilisé avec un autre contenu".into(),
                ))
            };
        }
        if change.version == self.root_version
            || self.changes.iter().any(|c| c.version == change.version)
        {
            return Err(Error::Conflict(
                "identifiant de version déjà utilisé".into(),
            ));
        }
        check_change(&change, self.id, self.head())?;
        apply_operations(&mut network, &change.operations)?;
        self.changes.push(change);
        Ok(true)
    }
    pub fn from_json(json: &str) -> Result<Self> {
        let project: Self = serde_json::from_str(json)?;
        project.migrate()
    }
    pub fn to_json(&self) -> Result<String> {
        self.network()?;
        Ok(serde_json::to_string(self)?)
    }
}

fn check_change(change: &ChangeSet, project: Id, head: Id) -> Result<()> {
    supported(change.schema_version)?;
    if change.project_id != project || change.base_version != head {
        return Err(Error::Conflict(format!(
            "base attendue {head}, base reçue {} (vérifier aussi le projet)",
            change.base_version
        )));
    }
    if change.id.is_nil()
        || change.version.is_nil()
        || change.version == change.base_version
        || change.operations.is_empty()
        || change.author.trim().is_empty()
        || change.message.trim().is_empty()
    {
        return Err(Error::Invalid("lot vide ou métadonnées invalides".into()));
    }
    Ok(())
}

fn apply_operations(network: &mut Network, operations: &[Operation]) -> Result<()> {
    for operation in operations {
        if let Operation::PutTechnology { technology } = operation {
            technology.validate()?;
            match network.technologies.get(&technology.id) {
                Some(existing) if existing != technology => {
                    return Err(Error::Conflict(format!(
                        "la révision technologique {} est immuable",
                        technology.id
                    )));
                }
                Some(_) => {}
                None => {
                    network
                        .technologies
                        .insert(technology.id, technology.clone());
                }
            }
            continue;
        }
        if let Operation::RemoveTechnology { id } = operation {
            if network.objects.values().any(|object| {
                object
                    .technology
                    .as_ref()
                    .is_some_and(|binding| binding.technology_id == *id)
            }) {
                return Err(Error::Invalid(format!(
                    "technologie {id} encore utilisée par un ouvrage"
                )));
            }
            if network.technologies.remove(id).is_none() {
                return Err(Error::Invalid(format!("technologie {id} absente")));
            }
            continue;
        }
        if let Operation::Create { object } = operation {
            if network.objects.contains_key(&object.id) {
                return Err(Error::Conflict(format!("objet {} déjà présent", object.id)));
            }
            network.objects.insert(object.id, object.clone());
            continue;
        }
        let id = match operation {
            Operation::Delete { id }
            | Operation::Rename { id, .. }
            | Operation::SetService { id, .. }
            | Operation::SetPower { id, .. }
            | Operation::SetSwitch { id, .. } => id,
            Operation::SetGeometry { id, .. }
            | Operation::SetEquipment { id, .. }
            | Operation::SetElectrical { id, .. }
            | Operation::SetTechnology { id, .. } => id,
            Operation::Create { .. }
            | Operation::PutTechnology { .. }
            | Operation::RemoveTechnology { .. } => unreachable!(),
        };
        let object = network
            .objects
            .get_mut(id)
            .ok_or_else(|| Error::Invalid(format!("objet {id} absent")))?;
        match operation {
            Operation::Delete { id } => {
                network.objects.remove(id);
            }
            Operation::SetElectrical { electrical, .. } => object.electrical = electrical.clone(),
            Operation::SetTechnology { technology, .. } => object.technology = technology.clone(),
            Operation::SetEquipment { equipment, .. } => {
                if std::mem::discriminant(equipment) != std::mem::discriminant(&object.equipment) {
                    return Err(Error::Invalid("le type d'un ouvrage ne peut pas être changé par édition de ses propriétés".into()));
                }
                object.equipment = equipment.clone();
            }
            Operation::Rename { name, .. } => object.name = name.clone(),
            Operation::SetGeometry { geometry, .. } => object.geometry = geometry.clone(),
            Operation::SetService { in_service, .. } => object.in_service = *in_service,
            Operation::SetPower { p_w, q_var, .. } => match &mut object.equipment {
                Equipment::Load {
                    p_w: p, q_var: q, ..
                }
                | Equipment::Generator {
                    p_w: p, q_var: q, ..
                } => {
                    *p = *p_w;
                    *q = *q_var;
                }
                _ => {
                    return Err(Error::Invalid(
                        "puissance applicable seulement aux charges et producteurs".into(),
                    ));
                }
            },
            Operation::SetSwitch { closed, .. } => match &mut object.equipment {
                Equipment::Switch { closed: state, .. } => *state = *closed,
                _ => return Err(Error::Invalid("objet différent d'un interrupteur".into())),
            },
            Operation::Create { .. }
            | Operation::PutTechnology { .. }
            | Operation::RemoveTechnology { .. } => unreachable!(),
        }
    }
    network.validate()
}
