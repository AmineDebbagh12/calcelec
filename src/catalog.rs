//! Embedded, immutable technology definitions. Equipment keeps resolved values.
use crate::model::{Equipment, Error, Id, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineConstruction {
    Overhead,
    Underground,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TechnologyData {
    Line {
        construction: LineConstruction,
        conductor_material: String,
        section_mm2: f64,
        r_ohm_per_km: f64,
        x_ohm_per_km: f64,
        max_current_a: f64,
    },
    Transformer {
        primary_voltage_v: f64,
        secondary_voltage_v: f64,
        rated_va: f64,
        uk_percent: f64,
        vector_group: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Technology {
    /// Immutable identifier of this exact revision.
    pub id: Id,
    /// Stable identifier shared by successive revisions of the same type.
    pub family_id: Id,
    pub revision: u32,
    pub designation: String,
    pub provenance: String,
    pub domain: String,
    pub data: TechnologyData,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electrical: Option<crate::electrical::ElectricalData>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnologyBinding {
    pub technology_id: Id,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locally_modified: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Technology {
    pub fn validate(&self) -> Result<()> {
        if self.id.is_nil()
            || self.family_id.is_nil()
            || self.revision == 0
            || self.designation.trim().is_empty()
            || self.provenance.trim().is_empty()
            || self.domain.trim().is_empty()
        {
            return Err(Error::Invalid(
                "identité ou documentation de technologie invalide".into(),
            ));
        }
        let positive = |value: f64| value.is_finite() && value > 0.0;
        match &self.data {
            TechnologyData::Line {
                conductor_material,
                section_mm2,
                r_ohm_per_km,
                x_ohm_per_km,
                max_current_a,
                ..
            } => {
                if conductor_material.trim().is_empty()
                    || !positive(*section_mm2)
                    || !r_ohm_per_km.is_finite()
                    || *r_ohm_per_km < 0.0
                    || !x_ohm_per_km.is_finite()
                    || !positive(*max_current_a)
                {
                    return Err(Error::Invalid(format!(
                        "{} : paramètres de ligne invalides",
                        self.designation
                    )));
                }
            }
            TechnologyData::Transformer {
                primary_voltage_v,
                secondary_voltage_v,
                rated_va,
                uk_percent,
                vector_group,
            } => {
                if !positive(*primary_voltage_v)
                    || !positive(*secondary_voltage_v)
                    || !positive(*rated_va)
                    || !positive(*uk_percent)
                    || vector_group.trim().is_empty()
                {
                    return Err(Error::Invalid(format!(
                        "{} : paramètres de transformateur invalides",
                        self.designation
                    )));
                }
            }
        }
        if self.electrical.as_ref().is_some_and(|data| {
            !matches!(
                (&self.data, data),
                (
                    TechnologyData::Line { .. },
                    crate::electrical::ElectricalData::Line(_)
                ) | (
                    TechnologyData::Transformer { .. },
                    crate::electrical::ElectricalData::Transformer(_)
                )
            )
        }) {
            return Err(Error::Invalid(format!(
                "{} : modèle électrique détaillé incompatible",
                self.designation
            )));
        }
        Ok(())
    }

    pub fn applies_to(&self, equipment: &Equipment) -> bool {
        matches!(
            (&self.data, equipment),
            (TechnologyData::Line { .. }, Equipment::Line { .. })
                | (
                    TechnologyData::Transformer { .. },
                    Equipment::Transformer { .. }
                )
        )
    }

    /// Resolve catalogue values while preserving topology and line length.
    pub fn instantiate(&self, equipment: &Equipment) -> Result<Equipment> {
        match (&self.data, equipment) {
            (
                TechnologyData::Line {
                    r_ohm_per_km,
                    x_ohm_per_km,
                    max_current_a,
                    ..
                },
                Equipment::Line {
                    from, to, length_m, ..
                },
            ) => Ok(Equipment::Line {
                from: *from,
                to: *to,
                length_m: *length_m,
                r_ohm_per_km: *r_ohm_per_km,
                x_ohm_per_km: *x_ohm_per_km,
                max_current_a: *max_current_a,
            }),
            (
                TechnologyData::Transformer {
                    rated_va,
                    uk_percent,
                    ..
                },
                Equipment::Transformer { from, to, .. },
            ) => Ok(Equipment::Transformer {
                from: *from,
                to: *to,
                rated_va: *rated_va,
                uk_percent: *uk_percent,
            }),
            _ => Err(Error::Invalid(format!(
                "{} : technologie incompatible avec l'ouvrage",
                self.designation
            ))),
        }
    }

    pub fn resolved_matches(
        &self,
        equipment: &Equipment,
        electrical: &Option<crate::electrical::ElectricalData>,
    ) -> bool {
        self.instantiate(equipment)
            .is_ok_and(|resolved| resolved == *equipment && self.electrical == *electrical)
    }
}

pub fn validate_catalog(technologies: &BTreeMap<Id, Technology>) -> Result<()> {
    let mut revisions = BTreeSet::new();
    for (id, technology) in technologies {
        if *id != technology.id || !revisions.insert((technology.family_id, technology.revision)) {
            return Err(Error::Invalid(
                "index ou révision de catalogue réutilisé".into(),
            ));
        }
        technology.validate()?;
    }
    Ok(())
}

fn id(value: u128) -> Id {
    Id::from_u128(value)
}

/// Educational values only; users must replace them with validated company data.
pub fn educational_catalog() -> BTreeMap<Id, Technology> {
    let line = |version: u128,
                family: u128,
                designation: &str,
                construction: LineConstruction,
                material: &str,
                section: f64,
                r: f64,
                x: f64,
                limit: f64| {
        Technology {
            id: id(version),
            family_id: id(family),
            revision: 1,
            designation: designation.into(),
            provenance: "Valeurs synthétiques de démonstration — non Enedis".into(),
            domain: "Démonstration HTA/BT, caractéristiques à valider avant toute étude".into(),
            data: TechnologyData::Line {
                construction,
                conductor_material: material.into(),
                section_mm2: section,
                r_ohm_per_km: r,
                x_ohm_per_km: x,
                max_current_a: limit,
            },
            electrical: None,
        }
    };
    let values = vec![
        line(
            0x2001,
            0x2101,
            "95AL — démonstration",
            LineConstruction::Underground,
            "Aluminium",
            95.0,
            0.320,
            0.080,
            200.0,
        ),
        line(
            0x2002,
            0x2102,
            "150AL — démonstration",
            LineConstruction::Underground,
            "Aluminium",
            150.0,
            0.206,
            0.080,
            260.0,
        ),
        line(
            0x2003,
            0x2103,
            "150CU — démonstration",
            LineConstruction::Underground,
            "Cuivre",
            150.0,
            0.124,
            0.080,
            320.0,
        ),
        line(
            0x2004,
            0x2104,
            "148AA — démonstration",
            LineConstruction::Overhead,
            "Alliage aluminium",
            148.0,
            0.225,
            0.350,
            300.0,
        ),
        Technology {
            id: id(0x2005),
            family_id: id(0x2105),
            revision: 1,
            designation: "Transformateur HTA/BT 250 kVA — démonstration".into(),
            provenance: "Valeurs synthétiques de démonstration — non Enedis".into(),
            domain: "20 kV / 400 V, paramétrage pédagogique".into(),
            data: TechnologyData::Transformer {
                primary_voltage_v: 20_000.0,
                secondary_voltage_v: 400.0,
                rated_va: 250_000.0,
                uk_percent: 4.0,
                vector_group: "Dyn11".into(),
            },
            electrical: None,
        },
    ];
    values
        .into_iter()
        .map(|technology| (technology.id, technology))
        .collect()
}
