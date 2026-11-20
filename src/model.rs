use crate::geometry::{Coordinate, CoordinateReference, Geometry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub type Id = Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Données invalides : {0}")]
    Invalid(String),
    #[error("Conflit : {0}")]
    Conflict(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

/// SI units throughout: V, A, W, var, m, ohm. No implicit catalogue defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Equipment {
    Bus {
        voltage_v: f64,
    },
    Source {
        bus: Id,
        voltage_pu: f64,
    },
    Line {
        from: Id,
        to: Id,
        length_m: f64,
        r_ohm_per_km: f64,
        x_ohm_per_km: f64,
        max_current_a: f64,
    },
    Transformer {
        from: Id,
        to: Id,
        rated_va: f64,
        uk_percent: f64,
    },
    Load {
        bus: Id,
        p_w: f64,
        q_var: f64,
        phase: Phase,
    },
    Generator {
        bus: Id,
        p_w: f64,
        q_var: f64,
        phase: Phase,
    },
    Switch {
        from: Id,
        to: Id,
        closed: bool,
        /// Physical installation. Missing in older projects, which remain generic.
        #[serde(default)]
        installation: SwitchInstallation,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwitchInstallation {
    #[default]
    Generic,
    AerialHta,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    Abc,
    A,
    B,
    C,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Object {
    pub id: Id,
    pub name: String,
    pub in_service: bool,
    pub equipment: Equipment,
    #[serde(default)]
    pub geometry: Geometry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electrical: Option<crate::electrical::ElectricalData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub technology: Option<crate::catalog::TechnologyBinding>,
}

impl Object {
    pub fn new(name: &str, equipment: Equipment) -> Self {
        Self {
            id: Id::new_v4(),
            name: name.into(),
            in_service: true,
            equipment,
            geometry: Geometry::Unlocated,
            electrical: None,
            technology: None,
        }
    }
    pub fn kind(&self) -> &'static str {
        match self.equipment {
            Equipment::Bus { .. } => "Jeu de barres",
            Equipment::Source { .. } => "Source amont",
            Equipment::Line { .. } => "Ligne / câble",
            Equipment::Transformer { .. } => "Transformateur",
            Equipment::Load { .. } => "Consommation",
            Equipment::Generator { .. } => "Production PV",
            Equipment::Switch {
                installation: SwitchInstallation::AerialHta,
                ..
            } => "Interrupteur aérien HTA",
            Equipment::Switch { .. } => "Interrupteur",
        }
    }
    pub fn power(&self) -> Option<(f64, f64)> {
        match self.equipment {
            Equipment::Load { p_w, q_var, .. } | Equipment::Generator { p_w, q_var, .. } => {
                Some((p_w, q_var))
            }
            _ => None,
        }
    }
    pub fn buses(&self) -> Vec<Id> {
        match self.equipment {
            Equipment::Bus { .. } => vec![],
            Equipment::Source { bus, .. }
            | Equipment::Load { bus, .. }
            | Equipment::Generator { bus, .. } => vec![bus],
            Equipment::Line { from, to, .. }
            | Equipment::Transformer { from, to, .. }
            | Equipment::Switch { from, to, .. } => vec![from, to],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub name: String,
    pub frequency_hz: f64,
    #[serde(default)]
    pub coordinate_reference: CoordinateReference,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub technologies: BTreeMap<Id, crate::catalog::Technology>,
    pub objects: BTreeMap<Id, Object>,
}

fn positive(value: f64, field: &str) -> Result<()> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(Error::Invalid(format!("{field} doit être fini et positif")))
    }
}

impl Network {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::Invalid("nom du réseau vide".into()));
        }
        positive(self.frequency_hz, "fréquence")?;
        crate::catalog::validate_catalog(&self.technologies)?;
        for (id, object) in &self.objects {
            object.geometry.validate()?;
            match (&object.equipment, &object.geometry) {
                (_, Geometry::Unlocated) | (Equipment::Line { .. }, Geometry::Polyline(_)) => {}
                (Equipment::Line { .. }, _) => {
                    return Err(Error::Invalid(format!(
                        "{} : un tronçon doit avoir une polyligne",
                        object.name
                    )));
                }
                (_, Geometry::Point(_)) => {}
                _ => {
                    return Err(Error::Invalid(format!(
                        "{} : cet équipement doit avoir une géométrie ponctuelle",
                        object.name
                    )));
                }
            }
            if *id != object.id || id.is_nil() || object.name.trim().is_empty() {
                return Err(Error::Invalid(format!("identité ou nom invalide : {id}")));
            }
            for bus in object.buses() {
                if !matches!(
                    self.objects.get(&bus).map(|o| &o.equipment),
                    Some(Equipment::Bus { .. })
                ) {
                    return Err(Error::Invalid(format!(
                        "{} : jeu de barres {bus} absent",
                        object.name
                    )));
                }
            }
            if let Some(data) = &object.electrical {
                data.validate_for(object, self)
                    .map_err(|error| Error::Invalid(format!("{} : {error}", object.name)))?;
            }
            if let Some(binding) = &object.technology {
                let technology =
                    self.technologies
                        .get(&binding.technology_id)
                        .ok_or_else(|| {
                            Error::Invalid(format!(
                                "{} : technologie {} absente du projet",
                                object.name, binding.technology_id
                            ))
                        })?;
                if !technology.applies_to(&object.equipment) {
                    return Err(Error::Invalid(format!(
                        "{} : technologie incompatible avec l'ouvrage",
                        object.name
                    )));
                }
                if let (
                    crate::catalog::TechnologyData::Transformer {
                        primary_voltage_v,
                        secondary_voltage_v,
                        ..
                    },
                    Equipment::Transformer { from, to, .. },
                ) = (&technology.data, &object.equipment)
                {
                    let Equipment::Bus {
                        voltage_v: from_voltage,
                    } = self.objects[from].equipment
                    else {
                        unreachable!("validated transformer endpoint")
                    };
                    let Equipment::Bus {
                        voltage_v: to_voltage,
                    } = self.objects[to].equipment
                    else {
                        unreachable!("validated transformer endpoint")
                    };
                    if from_voltage != *primary_voltage_v || to_voltage != *secondary_voltage_v {
                        return Err(Error::Invalid(format!(
                            "{} : tensions des bornes incompatibles avec la technologie {}",
                            object.name, technology.designation
                        )));
                    }
                }
                if !binding.locally_modified
                    && !technology.resolved_matches(&object.equipment, &object.electrical)
                {
                    return Err(Error::Invalid(format!(
                        "{} : paramètres différents de la technologie sans variante locale explicite",
                        object.name
                    )));
                }
            }
            match object.equipment {
                Equipment::Bus { voltage_v } => positive(voltage_v, "tension nominale")?,
                Equipment::Source { voltage_pu, .. } => positive(voltage_pu, "tension source")?,
                Equipment::Line {
                    from,
                    to,
                    length_m,
                    r_ohm_per_km,
                    x_ohm_per_km,
                    max_current_a,
                } => {
                    positive(length_m, "longueur")?;
                    positive(max_current_a, "courant admissible")?;
                    if !r_ohm_per_km.is_finite() || r_ohm_per_km < 0.0 || !x_ohm_per_km.is_finite()
                    {
                        return Err(Error::Invalid("impédance de ligne invalide".into()));
                    }
                    self.same_voltage(from, to)?;
                }
                Equipment::Transformer {
                    from,
                    to,
                    rated_va,
                    uk_percent,
                } => {
                    positive(rated_va, "puissance assignée")?;
                    positive(uk_percent, "tension de court-circuit")?;
                    if from == to {
                        return Err(Error::Invalid("transformateur raccordé à lui-même".into()));
                    }
                }
                Equipment::Load { p_w, q_var, .. } | Equipment::Generator { p_w, q_var, .. } => {
                    if !p_w.is_finite() || p_w < 0.0 || !q_var.is_finite() {
                        return Err(Error::Invalid(format!(
                            "{} : puissance invalide",
                            object.name
                        )));
                    }
                }
                Equipment::Switch {
                    from,
                    to,
                    installation,
                    ..
                } => {
                    self.same_voltage(from, to)?;
                    if installation == SwitchInstallation::AerialHta {
                        let Equipment::Bus { voltage_v } = self.objects[&from].equipment else {
                            unreachable!("switch endpoints were validated as buses")
                        };
                        if voltage_v < 1_000.0 {
                            return Err(Error::Invalid(format!(
                                "{} : un interrupteur aérien HTA exige deux bornes HTA",
                                object.name
                            )));
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn same_voltage(&self, from: Id, to: Id) -> Result<()> {
        if from == to {
            return Err(Error::Invalid("branche raccordée à elle-même".into()));
        }
        if self.objects[&from].equipment != self.objects[&to].equipment {
            return Err(Error::Invalid(
                "branche entre tensions nominales différentes".into(),
            ));
        }
        Ok(())
    }
    /// Structural connectivity only; this is not an electrical simulation.
    pub fn unpowered_buses(&self) -> Vec<Id> {
        let mut reached = BTreeSet::new();
        for o in self.objects.values().filter(|o| o.in_service) {
            if let Equipment::Source { bus, .. } = o.equipment
                && self.objects.get(&bus).is_some_and(|b| b.in_service)
            {
                reached.insert(bus);
            }
        }
        loop {
            let previous = reached.len();
            for o in self.objects.values().filter(|o| o.in_service) {
                let pair = match o.equipment {
                    Equipment::Line { from, to, .. }
                    | Equipment::Transformer { from, to, .. }
                    | Equipment::Switch {
                        from,
                        to,
                        closed: true,
                        ..
                    } => Some((from, to)),
                    _ => None,
                };
                if let Some((a, b)) = pair
                    && self.objects.get(&a).is_some_and(|o| o.in_service)
                    && self.objects.get(&b).is_some_and(|o| o.in_service)
                    && (reached.contains(&a) || reached.contains(&b))
                {
                    reached.insert(a);
                    reached.insert(b);
                }
            }
            if previous == reached.len() {
                break;
            }
        }
        self.objects
            .values()
            .filter(|o| {
                o.in_service
                    && matches!(o.equipment, Equipment::Bus { .. })
                    && !reached.contains(&o.id)
            })
            .map(|o| o.id)
            .collect()
    }
}

/// Synthetic educational values, not Enedis asset catalogue data.
pub fn rural_example() -> Network {
    let mut objects = BTreeMap::new();
    let mut add = |name: &str, equipment: Equipment| {
        let o = Object::new(name, equipment);
        let id = o.id;
        objects.insert(id, o);
        id
    };
    let source = add(
        "Poste source — HTA",
        Equipment::Bus {
            voltage_v: 20_000.0,
        },
    );
    add(
        "Équivalent amont",
        Equipment::Source {
            bus: source,
            voltage_pu: 1.0,
        },
    );
    let mut previous = source;
    for n in 1..=3 {
        let hta = add(
            &format!("Poste rural {n} — HTA"),
            Equipment::Bus {
                voltage_v: 20_000.0,
            },
        );
        add(
            &format!("Départ aérien — tronçon {n}"),
            Equipment::Line {
                from: previous,
                to: hta,
                length_m: 1500.0,
                r_ohm_per_km: 0.4,
                x_ohm_per_km: 0.35,
                max_current_a: 200.0,
            },
        );
        let bt = add(
            &format!("Poste rural {n} — BT"),
            Equipment::Bus { voltage_v: 400.0 },
        );
        add(
            &format!("Transformateur {n}"),
            Equipment::Transformer {
                from: hta,
                to: bt,
                rated_va: 250_000.0,
                uk_percent: 4.0,
            },
        );
        if n == 1 {
            let client = add("Extrémité départ BT", Equipment::Bus { voltage_v: 400.0 });
            add(
                "Câble BT",
                Equipment::Line {
                    from: bt,
                    to: client,
                    length_m: 200.0,
                    r_ohm_per_km: 0.32,
                    x_ohm_per_km: 0.08,
                    max_current_a: 150.0,
                },
            );
            for (phase, p_w) in [
                (Phase::A, 12_000.0),
                (Phase::B, 8_000.0),
                (Phase::C, 10_000.0),
            ] {
                add(
                    &format!("Clients BT — {phase:?}"),
                    Equipment::Load {
                        bus: client,
                        p_w,
                        q_var: 2000.0,
                        phase,
                    },
                );
            }
            add(
                "Photovoltaïque BT",
                Equipment::Generator {
                    bus: client,
                    p_w: 18_000.0,
                    q_var: 0.0,
                    phase: Phase::Abc,
                },
            );
        } else {
            add(
                &format!("Consommation agrégée {n}"),
                Equipment::Load {
                    bus: bt,
                    p_w: 90_000.0,
                    q_var: 20_000.0,
                    phase: Phase::Abc,
                },
            );
        }
        previous = hta;
    }
    // Fictional local survey: no association with an actual Enedis network.
    // Both voltage-level buses and transformer at each station are co-located.
    let mut locations = BTreeMap::new();
    locations.insert(source, Coordinate(0.0, 0.0));
    for object in objects.values() {
        if matches!(object.equipment, Equipment::Bus { .. }) && object.id != source {
            let point = if object.name == "Extrémité départ BT" {
                Coordinate(1370.0, 320.0)
            } else if object.name.contains("rural 1") {
                Coordinate(1200.0, 400.0)
            } else if object.name.contains("rural 2") {
                Coordinate(2500.0, -200.0)
            } else {
                Coordinate(3800.0, 500.0)
            };
            locations.insert(object.id, point);
        }
    }
    let mut charge_index = 0;
    for object in objects.values_mut() {
        object.geometry = match object.equipment {
            Equipment::Bus { .. } => Geometry::Point(locations[&object.id]),
            Equipment::Line { from, to, .. } => {
                let a = locations[&from];
                let b = locations[&to];
                Geometry::Polyline(vec![
                    a,
                    Coordinate((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0 + 60.0),
                    b,
                ])
            }
            Equipment::Transformer { from, .. } | Equipment::Switch { from, .. } => {
                Geometry::Point(locations[&from])
            }
            Equipment::Source { bus, .. } => Geometry::Point(locations[&bus]),
            Equipment::Load { bus, .. } | Equipment::Generator { bus, .. } => {
                charge_index += 1;
                let a = locations[&bus];
                Geometry::Point(Coordinate(a.0 + 30.0 * charge_index as f64, a.1 - 50.0))
            }
        };
    }
    Network {
        name: "Exemple rural France — HTA / BT".into(),
        frequency_hz: 50.0,
        coordinate_reference: CoordinateReference::LocalMeters,
        technologies: crate::catalog::educational_catalog(),
        objects,
    }
}
