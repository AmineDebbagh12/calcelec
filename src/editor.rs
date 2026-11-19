//! Versioned editing commands shared by the desktop and non-graphical clients.
use crate::{
    geometry::{Coordinate, Geometry},
    model::{Equipment, Error, Id, Network, Object, Phase, Result, SwitchInstallation},
    project::{Operation, Project},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    BusHta,
    BusBt,
    Station,
    Load,
    Generator,
    Source,
}
impl Placement {
    pub fn label(self) -> &'static str {
        match self {
            Self::BusHta => "Nœud HTA",
            Self::BusBt => "Nœud BT",
            Self::Station => "Poste HTA/BT",
            Self::Load => "Consommation",
            Self::Generator => "Production PV",
            Self::Source => "Source amont",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchKind {
    Line,
    Switch,
    Transformer,
}

fn bus(network: &Network, id: Option<Id>) -> Result<&Object> {
    id.and_then(|id| network.objects.get(&id))
        .filter(|o| matches!(o.equipment, Equipment::Bus { .. }))
        .ok_or_else(|| Error::Invalid("choisir un jeu de barres de raccordement".into()))
}
fn located_bus(network: &Network, id: Option<Id>) -> Result<&Object> {
    let object = bus(network, id)?;
    if !matches!(object.geometry, Geometry::Point(_)) {
        return Err(Error::Invalid(format!(
            "{} : position à renseigner avant de tracer une branche",
            object.name
        )));
    }
    Ok(object)
}
pub fn bus_point(network: &Network, id: Option<Id>) -> Result<Coordinate> {
    match located_bus(network, id)?.geometry {
        Geometry::Point(p) => Ok(p),
        _ => unreachable!(),
    }
}
/// Provisional electrical parameters are visible in the inspector and must be reviewed.
pub fn placement(
    network: &Network,
    kind: Placement,
    point: Coordinate,
    attachment: Option<Id>,
) -> Result<Vec<Operation>> {
    Geometry::Point(point).validate()?;
    let name = format!("{} {}", kind.label(), network.objects.len() + 1);
    let mut objects = vec![];
    if kind == Placement::Station {
        let mut hta = Object::new(
            &format!("{name} — HTA"),
            Equipment::Bus { voltage_v: 20000.0 },
        );
        hta.geometry = Geometry::Point(point);
        let mut bt = Object::new(&format!("{name} — BT"), Equipment::Bus { voltage_v: 400.0 });
        bt.geometry = Geometry::Point(point);
        let mut transformer = Object::new(
            &name,
            Equipment::Transformer {
                from: hta.id,
                to: bt.id,
                rated_va: 250000.0,
                uk_percent: 4.0,
            },
        );
        transformer.geometry = Geometry::Point(point);
        objects.extend([hta, bt, transformer]);
    } else {
        let equipment = match kind {
            Placement::BusHta => Equipment::Bus { voltage_v: 20000.0 },
            Placement::BusBt => Equipment::Bus { voltage_v: 400.0 },
            Placement::Load => Equipment::Load {
                bus: bus(network, attachment)?.id,
                p_w: 10000.0,
                q_var: 0.0,
                phase: Phase::Abc,
            },
            Placement::Generator => Equipment::Generator {
                bus: bus(network, attachment)?.id,
                p_w: 10000.0,
                q_var: 0.0,
                phase: Phase::Abc,
            },
            Placement::Source => Equipment::Source {
                bus: bus(network, attachment)?.id,
                voltage_pu: 1.0,
            },
            Placement::Station => unreachable!(),
        };
        let mut object = Object::new(&name, equipment);
        object.geometry = Geometry::Point(point);
        objects.push(object);
    }
    Ok(objects
        .into_iter()
        .map(|object| Operation::Create { object })
        .collect())
}

pub fn branch(
    network: &Network,
    kind: BranchKind,
    from: Option<Id>,
    to: Option<Id>,
    intermediate: &[Coordinate],
) -> Result<Operation> {
    let a = located_bus(network, from)?;
    let b = located_bus(network, to)?;
    if a.id == b.id {
        return Err(Error::Invalid(
            "choisir deux jeux de barres distincts".into(),
        ));
    }
    let start = bus_point(network, from)?;
    let end = bus_point(network, to)?;
    let (name, equipment, geometry) = match kind {
        BranchKind::Line => {
            let mut points = vec![start];
            points.extend_from_slice(intermediate);
            points.push(end);
            points.dedup();
            let geometry = Geometry::Polyline(points);
            geometry.validate()?;
            let length_m = geometry.length_m().unwrap();
            (
                "Tronçon",
                Equipment::Line {
                    from: a.id,
                    to: b.id,
                    length_m,
                    r_ohm_per_km: 0.32,
                    x_ohm_per_km: 0.08,
                    max_current_a: 150.0,
                },
                geometry,
            )
        }
        BranchKind::Switch => (
            "Interrupteur aérien HTA",
            Equipment::Switch {
                from: a.id,
                to: b.id,
                closed: true,
                installation: SwitchInstallation::AerialHta,
            },
            Geometry::Point(Coordinate((start.0 + end.0) / 2.0, (start.1 + end.1) / 2.0)),
        ),
        BranchKind::Transformer => (
            "Transformateur",
            Equipment::Transformer {
                from: a.id,
                to: b.id,
                rated_va: 250000.0,
                uk_percent: 4.0,
            },
            Geometry::Point(start),
        ),
    };
    if kind != BranchKind::Transformer && a.equipment != b.equipment {
        return Err(Error::Invalid(
            "un tronçon ou interrupteur exige deux jeux de barres de même tension".into(),
        ));
    }
    if kind == BranchKind::Switch
        && !matches!(a.equipment, Equipment::Bus { voltage_v } if voltage_v >= 1_000.0)
    {
        return Err(Error::Invalid(
            "un interrupteur aérien doit être raccordé entre deux nœuds HTA".into(),
        ));
    }
    let mut object = Object::new(&format!("{name} {}", network.objects.len() + 1), equipment);
    object.geometry = geometry;
    Ok(Operation::Create { object })
}

/// Dependencies are listed explicitly; the caller chooses whether to include them.
pub fn dependents(network: &Network, id: Id) -> Vec<Id> {
    network
        .objects
        .values()
        .filter(|o| o.id != id && o.buses().contains(&id))
        .map(|o| o.id)
        .collect()
}
pub fn deletion(network: &Network, id: Id, include_dependents: bool) -> Result<Vec<Operation>> {
    if !network.objects.contains_key(&id) {
        return Err(Error::Invalid("objet à supprimer absent".into()));
    }
    let linked = dependents(network, id);
    if !include_dependents && !linked.is_empty() {
        return Err(Error::Invalid(
            "objet encore référencé : supprimer explicitement ses dépendances".into(),
        ));
    }
    let mut operations = linked
        .into_iter()
        .map(|id| Operation::Delete { id })
        .collect::<Vec<_>>();
    operations.push(Operation::Delete { id });
    Ok(operations)
}

fn difference(from: &Network, to: &Network) -> Vec<Operation> {
    let mut operations = vec![];
    for (id, target) in &to.objects {
        if let Some(source) = from.objects.get(id) {
            if source.name != target.name {
                operations.push(Operation::Rename {
                    id: *id,
                    name: target.name.clone(),
                });
            }
            if source.in_service != target.in_service {
                operations.push(Operation::SetService {
                    id: *id,
                    in_service: target.in_service,
                });
            }
            if source.equipment != target.equipment {
                operations.push(Operation::SetEquipment {
                    id: *id,
                    equipment: target.equipment.clone(),
                });
            }
            if source.electrical != target.electrical {
                operations.push(Operation::SetElectrical {
                    id: *id,
                    electrical: target.electrical.clone(),
                });
            }
            if source.geometry != target.geometry {
                operations.push(Operation::SetGeometry {
                    id: *id,
                    geometry: target.geometry.clone(),
                });
            }
        } else {
            operations.push(Operation::Create {
                object: target.clone(),
            });
        }
    }
    for id in from
        .objects
        .keys()
        .filter(|id| !to.objects.contains_key(id))
    {
        operations.push(Operation::Delete { id: *id });
    }
    operations
}
struct Entry {
    undo: Vec<Operation>,
    redo: Vec<Operation>,
    message: String,
}
#[derive(Default)]
pub struct EditHistory {
    context: Option<(Id, Id)>,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}
impl EditHistory {
    pub fn sync(&mut self, project: &Project) {
        let context = Some((project.id, project.head()));
        if self.context != context {
            self.undo.clear();
            self.redo.clear();
            self.context = context;
        }
    }
    pub fn can_undo(&self, project: &Project) -> bool {
        self.context == Some((project.id, project.head())) && !self.undo.is_empty()
    }
    pub fn can_redo(&self, project: &Project) -> bool {
        self.context == Some((project.id, project.head())) && !self.redo.is_empty()
    }
    pub fn commit(
        &mut self,
        project: &mut Project,
        author: &str,
        message: &str,
        operations: Vec<Operation>,
    ) -> Result<()> {
        self.sync(project);
        let before = project.network()?;
        let change = project.prepare(author, message, operations);
        let mut candidate = project.clone();
        candidate.apply(change)?;
        let after = candidate.network()?;
        if before == after {
            return Err(Error::Invalid("aucune modification effective".into()));
        }
        *project = candidate;
        self.undo.push(Entry {
            undo: difference(&after, &before),
            redo: difference(&before, &after),
            message: message.into(),
        });
        self.redo.clear();
        self.context = Some((project.id, project.head()));
        Ok(())
    }
    pub fn undo(&mut self, project: &mut Project, author: &str) -> Result<()> {
        if !self.can_undo(project) {
            return Err(Error::Conflict(
                "aucune action locale annulable sur cette version".into(),
            ));
        }
        let entry = self.undo.last().unwrap();
        let change = project.prepare(
            author,
            &format!("Annuler : {}", entry.message),
            entry.undo.clone(),
        );
        project.apply(change)?;
        self.redo.push(self.undo.pop().unwrap());
        self.context = Some((project.id, project.head()));
        Ok(())
    }
    pub fn redo(&mut self, project: &mut Project, author: &str) -> Result<()> {
        if !self.can_redo(project) {
            return Err(Error::Conflict(
                "aucune action locale à rétablir sur cette version".into(),
            ));
        }
        let entry = self.redo.last().unwrap();
        let change = project.prepare(
            author,
            &format!("Rétablir : {}", entry.message),
            entry.redo.clone(),
        );
        project.apply(change)?;
        self.undo.push(self.redo.pop().unwrap());
        self.context = Some((project.id, project.head()));
        Ok(())
    }
}
