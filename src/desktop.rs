use crate::desktop_geo::MapView;
use calcelec::geometry::{Coordinate, Geometry};
use calcelec::{
    model::{Equipment, Id, Network, Result, rural_example},
    project::{ChangeSet, Operation, Project},
    storage::{self, Store},
};
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use std::collections::BTreeMap;

pub struct Desktop {
    project: Project,
    network: Network,
    selected: Option<Id>,
    geometry: Geometry,
    geographic: bool,
    map: MapView,
    name: String,
    service: bool,
    p_kw: f64,
    q_kvar: f64,
    author: String,
    message: String,
    file_path: String,
    delta_path: String,
    database_path: String,
    database_projects: Vec<(Id, String)>,
    status: String,
    error: bool,
}
impl Desktop {
    pub fn new() -> Self {
        let network = rural_example();
        Self {
            project: Project::new(network.clone()).expect("valid built-in example"),
            network,
            selected: None,
            geometry: Geometry::Unlocated,
            geographic: true,
            map: MapView::default(),
            name: String::new(),
            service: true,
            p_kw: 0.0,
            q_kvar: 0.0,
            author: "Utilisateur local".into(),
            message: "Modification du réseau".into(),
            file_path: "reseau.calcelec.json".into(),
            delta_path: "modification.calcelec.json".into(),
            database_path: "calcelec.sqlite".into(),
            database_projects: vec![],
            status: "Exemple synthétique chargé en mémoire. Enregistrez-le avant de quitter."
                .into(),
            error: false,
        }
    }
    fn report(&mut self, result: Result<String>) {
        self.error = result.is_err();
        self.status = result.unwrap_or_else(|e| e.to_string());
    }
    fn select(&mut self, id: Id) {
        let object = &self.network.objects[&id];
        self.selected = Some(id);
        self.name = object.name.clone();
        self.geometry = object.geometry.clone();
        self.service = object.in_service;
        let (p, q) = object.power().unwrap_or_default();
        self.p_kw = p / 1000.0;
        self.q_kvar = q / 1000.0;
    }
    fn refresh(&mut self) -> Result<()> {
        self.network = self.project.network()?;
        if let Some(id) = self.selected {
            if self.network.objects.contains_key(&id) {
                self.select(id);
            } else {
                self.selected = None;
            }
        }
        Ok(())
    }
    fn record(&mut self) -> Result<String> {
        let Some(id) = self.selected else {
            return Ok("Sélectionnez un objet.".into());
        };
        let object = &self.network.objects[&id];
        let mut operations = vec![];
        if self.name != object.name {
            operations.push(Operation::Rename {
                id,
                name: self.name.clone(),
            });
        }
        if self.service != object.in_service {
            operations.push(Operation::SetService {
                id,
                in_service: self.service,
            });
        }
        if let Some((p, q)) = object.power()
            && ((p / 1000.0) != self.p_kw || (q / 1000.0) != self.q_kvar)
        {
            operations.push(Operation::SetPower {
                id,
                p_w: self.p_kw * 1000.0,
                q_var: self.q_kvar * 1000.0,
            });
        }
        if self.geometry != object.geometry {
            operations.push(Operation::SetGeometry {
                id,
                geometry: self.geometry.clone(),
            });
        }
        if operations.is_empty() {
            return Ok("Aucune propriété modifiée.".into());
        }
        let change = self
            .project
            .prepare(&self.author, &self.message, operations);
        self.project.apply(change)?;
        self.refresh()?;
        Ok("Version créée en mémoire. Sauvegardez le projet ou la base pour la conserver.".into())
    }
    fn files(&mut self, ui: &mut egui::Ui) {
        ui.heading("Projet et échanges");
        ui.label("Fichier projet (instantané initial + historique)");
        ui.text_edit_singleline(&mut self.file_path);
        ui.horizontal(|ui| {
            if ui.button("Exporter le projet").clicked() {
                let result = self
                    .project
                    .to_json()
                    .and_then(|s| storage::export_new(&self.file_path, &s))
                    .map(|_| "Projet exporté.".into());
                self.report(result);
            }
            if ui.button("Ouvrir").clicked() {
                let result = storage::read_project(&self.file_path).and_then(|project| {
                    self.map.reset();
                    self.project = project;
                    self.selected = None;
                    self.refresh()?;
                    Ok("Projet ouvert.".into())
                });
                self.report(result);
            }
        });
        ui.label("Fichier de modifications");
        ui.text_edit_singleline(&mut self.delta_path);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.project.changes.is_empty(),
                    egui::Button::new("Exporter le dernier lot"),
                )
                .clicked()
            {
                let result = serde_json::to_string(self.project.changes.last().unwrap())
                    .map_err(Into::into)
                    .and_then(|s| storage::export_new(&self.delta_path, &s))
                    .map(|_| {
                        "Lot exporté ; le destinataire doit posséder sa version de base.".into()
                    });
                self.report(result);
            }
            if ui.button("Importer le lot").clicked() {
                let result = (|| -> Result<String> {
                    let json = std::fs::read_to_string(&self.delta_path)?;
                    let change: ChangeSet = serde_json::from_str(&json)?;
                    let applied = self.project.apply(change)?;
                    self.refresh()?;
                    Ok(if applied {
                        "Lot appliqué en mémoire."
                    } else {
                        "Lot déjà présent, aucune modification."
                    }
                    .into())
                })();
                self.report(result);
            }
        });
        ui.label("Base SQLite locale");
        ui.text_edit_singleline(&mut self.database_path);
        ui.horizontal(|ui| {
            if ui.button("Enregistrer en base").clicked() {
                let result = Store::open(&self.database_path)
                    .and_then(|mut db| db.save(&self.project))
                    .map(|_| "Réseau et historique enregistrés en base.".into());
                self.report(result);
            }
            if ui.button("Relire ce projet").clicked() {
                let result = Store::open(&self.database_path)
                    .and_then(|db| db.load(self.project.id))
                    .and_then(|project| {
                        self.map.reset();
                        self.project = project;
                        self.refresh()?;
                        Ok("Projet relu depuis SQLite.".into())
                    });
                self.report(result);
            }
        });
        if ui.button("Lister les projets de la base").clicked() {
            let result = Store::open(&self.database_path)
                .and_then(|db| db.projects())
                .map(|projects| {
                    self.database_projects = projects;
                    format!("{} projets en base.", self.database_projects.len())
                });
            self.report(result);
        }
        let mut requested = None;
        for (id, name) in &self.database_projects {
            if ui
                .button(format!("Charger {name} ({})", &id.to_string()[..8]))
                .clicked()
            {
                requested = Some(*id);
            }
        }
        if let Some(id) = requested {
            let result = Store::open(&self.database_path)
                .and_then(|db| db.load(id))
                .and_then(|project| {
                    self.map.reset();
                    self.project = project;
                    self.selected = None;
                    self.refresh()?;
                    Ok("Projet chargé depuis SQLite.".into())
                });
            self.report(result);
        }
        ui.small("Les exports refusent d'écraser les fichiers existants. Utilisez un nouveau nom.");
        ui.small(
            "Ouvrir ou relire remplace le projet en mémoire. Enregistrez votre travail auparavant.",
        );
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading("Propriétés");
        if let Some(id) = self.selected {
            let object = self.network.objects[&id].clone();
            ui.label(object.kind());
            ui.small(id.to_string());
            ui.label("Nom");
            ui.text_edit_singleline(&mut self.name);
            ui.checkbox(&mut self.service, "En service");
            if object.power().is_some() {
                ui.horizontal(|ui| {
                    ui.label("P (kW)");
                    ui.add(egui::DragValue::new(&mut self.p_kw).speed(0.5));
                });
                ui.horizontal(|ui| {
                    ui.label("Q (kvar)");
                    ui.add(egui::DragValue::new(&mut self.q_kvar).speed(0.5));
                });
                ui.small(
                    "P positive : consommation pour une charge, injection pour un producteur.",
                );
            }
            self.geometry_editor(ui, &object);
            ui.collapsing("Paramètres du modèle (SI)", |ui| {
                ui.monospace(serde_json::to_string_pretty(&object.equipment).unwrap_or_default());
            });
            ui.separator();
            ui.label("Auteur déclaré");
            ui.text_edit_singleline(&mut self.author);
            ui.label("Message de version");
            ui.text_edit_singleline(&mut self.message);
            if ui
                .button("Créer une version avec ces changements")
                .clicked()
            {
                let r = self.record();
                self.report(r);
            }
            if ui.button("Réinitialiser les champs").clicked() {
                self.select(id);
            }
        } else {
            ui.label("Sélectionnez un objet dans le tableau ou le schéma.");
        }
    }
    fn geometry_editor(&mut self, ui: &mut egui::Ui, object: &calcelec::model::Object) {
        ui.separator();
        ui.label("Géométrie géographique");
        ui.small(self.network.coordinate_reference.label());
        let mut remove = None;
        match &mut self.geometry {
            Geometry::Unlocated => {
                ui.label("Objet non localisé");
                if ui.button("Initialiser une géométrie").clicked() {
                    self.geometry = if matches!(object.equipment, Equipment::Line { .. }) {
                        Geometry::Polyline(vec![Coordinate(0.0, 0.0), Coordinate(10.0, 0.0)])
                    } else {
                        Geometry::Point(Coordinate(0.0, 0.0))
                    };
                }
            }
            Geometry::Point(point) => {
                ui.horizontal(|ui| {
                    ui.label("X (m)");
                    ui.add(
                        egui::DragValue::new(&mut point.0)
                            .speed(1.0)
                            .max_decimals(2),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Y (m)");
                    ui.add(
                        egui::DragValue::new(&mut point.1)
                            .speed(1.0)
                            .max_decimals(2),
                    );
                });
            }
            Geometry::Polyline(points) => {
                ui.label("Sommets : X / Y en mètres");
                let can_remove = points.len() > 2;
                for (index, point) in points.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}", index + 1));
                            ui.add(
                                egui::DragValue::new(&mut point.0)
                                    .speed(1.0)
                                    .max_decimals(2),
                            );
                            ui.add(
                                egui::DragValue::new(&mut point.1)
                                    .speed(1.0)
                                    .max_decimals(2),
                            );
                            if ui.add_enabled(can_remove, egui::Button::new("−")).clicked() {
                                remove = Some(index);
                            }
                        });
                    });
                }
                if let Some(index) = remove {
                    points.remove(index);
                }
                if ui.button("Ajouter un sommet").clicked() {
                    let last = points.last().copied().unwrap_or(Coordinate(0.0, 0.0));
                    points.push(Coordinate(last.0 + 10.0, last.1));
                }
            }
        }
        if let Some(length) = self.geometry.length_m() {
            ui.small(format!("Longueur du tracé : {length:.1} m"));
            if let Equipment::Line { length_m, .. } = object.equipment {
                ui.small(format!(
                    "Longueur électrique renseignée : {length_m:.1} m (indépendante du tracé)"
                ));
            }
        }
        ui.small("Validez les coordonnées en créant une version. La géométrie ne change pas les raccordements.");
    }
    fn diagram(&mut self, ui: &mut egui::Ui) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 300.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        // Tree layout of the example; disconnected buses are placed in a final column.
        let mut depth = BTreeMap::new();
        for o in self.network.objects.values() {
            if let Equipment::Source { bus, .. } = o.equipment {
                depth.insert(bus, 0usize);
            }
        }
        for _ in 0..self.network.objects.len() {
            let mut added = false;
            for o in self.network.objects.values() {
                let pair = match o.equipment {
                    Equipment::Line { from, to, .. }
                    | Equipment::Transformer { from, to, .. }
                    | Equipment::Switch { from, to, .. } => Some((from, to)),
                    _ => None,
                };
                if let Some((a, b)) = pair {
                    for (a, b) in [(a, b), (b, a)] {
                        if let Some(d) = depth.get(&a).copied()
                            && !depth.contains_key(&b)
                        {
                            depth.insert(b, d + 1);
                            added = true;
                        }
                    }
                }
            }
            if !added {
                break;
            }
        }
        let last = depth.values().copied().max().unwrap_or(0) + 1;
        let mut columns: BTreeMap<usize, Vec<Id>> = BTreeMap::new();
        let mut buses: Vec<_> = self
            .network
            .objects
            .values()
            .filter(|o| matches!(o.equipment, Equipment::Bus { .. }))
            .collect();
        buses.sort_by_key(|o| &o.name);
        for o in buses {
            columns
                .entry(depth.get(&o.id).copied().unwrap_or(last))
                .or_default()
                .push(o.id);
        }
        let max_depth = columns.keys().copied().max().unwrap_or(1).max(1);
        let mut positions = BTreeMap::new();
        for (d, ids) in columns {
            for (row, id) in ids.iter().enumerate() {
                positions.insert(
                    *id,
                    Pos2::new(
                        rect.left()
                            + 70.0
                            + (rect.width() - 140.0).max(1.0) * d as f32 / max_depth as f32,
                        rect.top() + 40.0 + 210.0 * (row + 1) as f32 / (ids.len() + 1) as f32,
                    ),
                );
            }
        }
        for o in self.network.objects.values() {
            let ids = o.buses();
            if let [a, b] = ids.as_slice()
                && let (Some(a), Some(b)) = (positions.get(a), positions.get(b))
            {
                let color = if o.in_service {
                    Color32::from_rgb(90, 140, 160)
                } else {
                    Color32::DARK_GRAY
                };
                painter.line_segment([*a, *b], Stroke::new(2.0, color));
                if matches!(o.equipment, Equipment::Transformer { .. }) {
                    painter.circle_stroke(a.lerp(*b, 0.5), 9.0, Stroke::new(2.0, color));
                }
            }
        }
        let mut clicked = None;
        for (id, pos) in positions {
            let o = &self.network.objects[&id];
            let color = if Some(id) == self.selected {
                Color32::YELLOW
            } else if o.in_service {
                Color32::from_rgb(70, 190, 175)
            } else {
                Color32::GRAY
            };
            painter.circle_filled(pos, 6.0, color);
            painter.text(
                pos + Vec2::new(0.0, 14.0),
                egui::Align2::CENTER_TOP,
                &o.name,
                egui::FontId::proportional(11.0),
                color,
            );
            if ui
                .interact(
                    egui::Rect::from_center_size(pos, Vec2::splat(24.0)),
                    ui.scope_id().with(id),
                    egui::Sense::click(),
                )
                .on_hover_text(&o.name)
                .clicked()
            {
                clicked = Some(id);
            }
        }
        if let Some(id) = clicked {
            self.select(id);
        }
    }
}
impl Desktop {
    fn render(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("header").show(ui, |ui| {
            ui.heading("CalcElec / Distribution publique");
            ui.label(&self.network.name);
            ui.horizontal(|ui| {
                ui.label(format!("{} objets · {} versions enregistrées en mémoire",self.network.objects.len(),self.project.changes.len()+1));
                ui.small(format!("Version {}",self.project.head()));
            });
            ui.colored_label(Color32::from_rgb(230,180,75),"Prototype de modélisation et de partage — calculs électriques à venir. Données d'exemple synthétiques.");
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.colored_label(
                if self.error {
                    Color32::LIGHT_RED
                } else {
                    Color32::LIGHT_GREEN
                },
                &self.status,
            );
        });
        egui::Panel::right("inspector")
            .default_size(335.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.inspector(ui);
                    ui.separator();
                    self.files(ui);
                    ui.separator();
                    ui.heading("Historique");
                    for change in self.project.changes.iter().rev() {
                        ui.label(format!("{} — {}", change.author, change.message));
                        ui.small(format!(
                            "{} opérations · {}",
                            change.operations.len(),
                            change.version
                        ));
                    }
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.geographic, true, "Vue géographique");
                ui.selectable_value(&mut self.geographic, false, "Vue topologique");
            });
            if self.geographic {
                if let Some(id) = self.map.show(ui, &self.network, self.selected) {
                    self.select(id);
                }
            } else {
                self.diagram(ui);
            }
            ui.label(format!(
                "Connectivité : {} jeux de barres en service sans source accessible",
                self.network.unpowered_buses().len()
            ));
            ui.separator();
            let mut clicked = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("objects")
                    .striped(true)
                    .min_col_width(90.0)
                    .show(ui, |ui| {
                        ui.strong("Équipement");
                        ui.strong("Type");
                        ui.strong("État");
                        ui.strong("P / Q");
                        ui.end_row();
                        let mut objects: Vec<_> = self.network.objects.values().collect();
                        objects.sort_by_key(|o| &o.name);
                        for o in objects {
                            if ui
                                .selectable_label(self.selected == Some(o.id), &o.name)
                                .clicked()
                            {
                                clicked = Some(o.id);
                            }
                            ui.label(o.kind());
                            ui.label(if o.in_service {
                                "En service"
                            } else {
                                "Hors service"
                            });
                            ui.label(
                                o.power()
                                    .map(|(p, q)| {
                                        format!("{:.1} kW / {:.1} kvar", p / 1000.0, q / 1000.0)
                                    })
                                    .unwrap_or_else(|| "—".into()),
                            );
                            ui.end_row();
                        }
                    });
            });
            if let Some(id) = clicked {
                self.select(id);
            }
        });
    }
}

impl eframe::App for Desktop {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.render(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_renders_and_records_an_edit_without_display_server() {
        let mut app = Desktop::new();
        let id = app
            .network
            .objects
            .values()
            .find(|o| o.power().is_some())
            .unwrap()
            .id;
        app.select(id);
        app.p_kw = 12.5;
        app.record().unwrap();
        assert_eq!(app.network.objects[&id].power().unwrap().0, 12500.0);
        let context = egui::Context::default();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1280.0, 820.0),
                )),
                ..Default::default()
            },
            |ui| app.render(ui),
        );
        assert!(!output.shapes.is_empty());
        output.textures_delta.clear();
        app.geographic = false;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| app.render(ui));
        output.textures_delta.clear();
    }
}
