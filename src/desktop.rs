use crate::desktop_geo::{MapEvent, MapMode, MapResults, MapTools, MapView};
use calcelec::editor::{self, BranchKind, EditHistory, Placement};
use calcelec::geometry::{Coordinate, Geometry};
use calcelec::load_flow::{self, Options, StudyResult};
use calcelec::{
    model::{Equipment, Id, Network, Result, rural_example},
    project::{ChangeSet, Operation, Project},
    storage::{self, Store},
};
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

fn numeric(ui: &mut egui::Ui, label: &str, value: &mut f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(0.1).max_decimals(6));
    });
}
fn diagram_northing(object: &calcelec::model::Object) -> f64 {
    let coordinates = object.geometry.coordinates();
    if coordinates.is_empty() {
        0.0
    } else {
        coordinates.iter().map(|p| p.1).sum::<f64>() / coordinates.len() as f64
    }
}
fn diagram_northing_order(
    a: &calcelec::model::Object,
    b: &calcelec::model::Object,
) -> std::cmp::Ordering {
    diagram_northing(b)
        .total_cmp(&diagram_northing(a))
        .then_with(|| a.name.cmp(&b.name))
        .then_with(|| a.id.cmp(&b.id))
}
fn bus_choice(ui: &mut egui::Ui, network: &Network, label: &str, value: &mut Option<Id>) {
    let selected = value
        .and_then(|id| network.objects.get(&id))
        .map(|o| o.name.as_str())
        .unwrap_or("Choisir une borne");
    egui::ComboBox::from_id_salt(label)
        .selected_text(format!("{label} : {selected}"))
        .show_ui(ui, |ui| {
            let mut buses: Vec<_> = network
                .objects
                .values()
                .filter(|o| matches!(o.equipment, Equipment::Bus { .. }))
                .collect();
            buses.sort_by_key(|o| &o.name);
            for bus in buses {
                ui.selectable_value(value, Some(bus.id), &bus.name);
            }
        });
}
fn readonly_bus(ui: &mut egui::Ui, network: &Network, label: &str, id: Id) {
    let name = network
        .objects
        .get(&id)
        .map(|o| o.name.as_str())
        .unwrap_or("borne absente");
    ui.label(format!("{label} : {name}"));
}
fn required_bus_choice(ui: &mut egui::Ui, network: &Network, label: &str, value: &mut Id) {
    let mut optional = Some(*value);
    bus_choice(ui, network, label, &mut optional);
    if let Some(id) = optional {
        *value = id;
    }
}

enum CalculationResult {
    Balanced(StudyResult),
    Unbalanced(calcelec::unbalanced::StudyResult),
}
struct CalculationTask {
    receiver: mpsc::Receiver<load_flow::Result<CalculationResult>>,
    cancel: Arc<AtomicBool>,
}

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
    study: Option<StudyResult>,
    unbalanced_study: Option<calcelec::unbalanced::StudyResult>,
    use_unbalanced: bool,
    calculation: Option<CalculationTask>,
    simplified_calculation: bool,
    results_view: bool,
    result_path: String,
    history: EditHistory,
    tool: MapMode,
    placement: Placement,
    branch_kind: BranchKind,
    attachment: Option<Id>,
    from_bus: Option<Id>,
    to_bus: Option<Id>,
    draft: Option<Vec<Coordinate>>,
    equipment: Equipment,
    electrical_text: String,
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
            study: None,
            unbalanced_study: None,
            use_unbalanced: false,
            calculation: None,
            simplified_calculation: false,
            results_view: false,
            result_path: "resultats.calcelec.json".into(),
            history: EditHistory::default(),
            tool: MapMode::Select,
            placement: Placement::Station,
            branch_kind: BranchKind::Line,
            attachment: None,
            from_bus: None,
            to_bus: None,
            draft: None,
            equipment: Equipment::Bus { voltage_v: 400.0 },
            electrical_text: String::new(),
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
        self.equipment = object.equipment.clone();
        self.electrical_text = object
            .electrical
            .as_ref()
            .map(|data| serde_json::to_string_pretty(data).expect("validated electrical data"))
            .unwrap_or_default();
        self.service = object.in_service;
        let (p, q) = object.power().unwrap_or_default();
        self.p_kw = p / 1000.0;
        self.q_kvar = q / 1000.0;
    }
    fn refresh(&mut self) -> Result<()> {
        self.network = self.project.network()?;
        self.history.sync(&self.project);
        self.map.cancel_drag();
        self.draft = None;
        for endpoint in [&mut self.attachment, &mut self.from_bus, &mut self.to_bus] {
            if endpoint.is_some_and(|id| !self.network.objects.contains_key(&id)) {
                *endpoint = None;
            }
        }
        if self.study.as_ref().is_some_and(|s| {
            s.project_id != self.project.id || s.network_version != self.project.head()
        }) {
            self.study = None;
        }
        if self.unbalanced_study.as_ref().is_some_and(|s| {
            s.project_id != self.project.id || s.network_version != self.project.head()
        }) {
            self.unbalanced_study = None;
        }
        if let Some(task) = &self.calculation {
            task.cancel.store(true, Ordering::Relaxed);
        }
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
        if self.equipment != object.equipment {
            operations.push(Operation::SetEquipment {
                id,
                equipment: self.equipment.clone(),
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
        let electrical = if self.electrical_text.trim().is_empty() {
            None
        } else {
            Some(
                serde_json::from_str::<calcelec::electrical::ElectricalData>(
                    &self.electrical_text,
                )?,
            )
        };
        if electrical != object.electrical {
            operations.push(Operation::SetElectrical { id, electrical });
        }
        if operations.is_empty() {
            return Ok("Aucune propriété modifiée.".into());
        }
        self.history
            .commit(&mut self.project, &self.author, &self.message, operations)?;
        self.refresh()?;
        Ok("Version créée en mémoire. Sauvegardez le projet ou la base pour la conserver.".into())
    }
    fn commit_edit(&mut self, message: &str, operations: Vec<Operation>) -> Result<String> {
        let created = operations.iter().rev().find_map(|op| {
            if let Operation::Create { object } = op {
                Some(object.id)
            } else {
                None
            }
        });
        self.history
            .commit(&mut self.project, &self.author, message, operations)?;
        self.refresh()?;
        if let Some(id) = created {
            self.select(id);
        }
        Ok(format!(
            "{message} — version créée en mémoire, pensez à enregistrer."
        ))
    }
    fn map_event(&mut self, event: MapEvent) {
        let result = match event {
            MapEvent::Select(id) => {
                self.select(id);
                return;
            }
            MapEvent::SetGeometry { id, geometry } => self.commit_edit(
                "Modification géographique",
                vec![Operation::SetGeometry { id, geometry }],
            ),
            MapEvent::Click(point) if self.tool == MapMode::Place => {
                editor::placement(&self.network, self.placement, point, self.attachment)
                    .and_then(|ops| self.commit_edit("Placement d'un ouvrage", ops))
            }
            MapEvent::Click(point) if self.tool == MapMode::Trace => {
                if let Some(draft) = &mut self.draft {
                    if draft.last() != Some(&point) {
                        draft.push(point);
                    }
                    self.status="Sommet ajouté au brouillon ; terminez le tronçon avec le bouton de la toolbox.".into();
                    self.error = false;
                } else {
                    self.status =
                        "Choisissez les bornes puis commencez le tracé dans la toolbox.".into();
                }
                return;
            }
            _ => return,
        };
        self.report(result);
    }
    fn toolbox(&mut self, ui: &mut egui::Ui) {
        ui.heading("Toolbox");
        if self.geographic {
            ui.horizontal_wrapped(|ui| {
                for (mode, label) in [
                    (MapMode::Select, "Sélection"),
                    (MapMode::Move, "Déplacer"),
                    (MapMode::Vertex, "Sommets"),
                    (MapMode::Place, "Placer"),
                    (MapMode::Trace, "Raccorder"),
                ] {
                    if ui.selectable_value(&mut self.tool, mode, label).changed() {
                        self.draft = None;
                        self.map.cancel_drag();
                    }
                }
            });
        } else {
            self.tool = MapMode::Select;
            self.draft = None;
            self.map.cancel_drag();
            ui.small("Vue unifilaire dérivée : sélection et propriétés uniquement. Les raccordements et la géométrie se modifient dans la vue géographique.");
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.history.can_undo(&self.project),
                    egui::Button::new("Annuler"),
                )
                .clicked()
            {
                let r = self
                    .history
                    .undo(&mut self.project, &self.author)
                    .and_then(|()| {
                        self.refresh()?;
                        Ok("Action annulée par une nouvelle version.".into())
                    });
                self.report(r);
            }
            if ui
                .add_enabled(
                    self.history.can_redo(&self.project),
                    egui::Button::new("Rétablir"),
                )
                .clicked()
            {
                let r = self
                    .history
                    .redo(&mut self.project, &self.author)
                    .and_then(|()| {
                        self.refresh()?;
                        Ok("Action rétablie par une nouvelle version.".into())
                    });
                self.report(r);
            }
        });
        if !self.geographic {
            return;
        }
        match self.tool {
            MapMode::Select => {
                ui.small("Sélectionnez un objet sur la carte ou dans le tableau.");
            }
            MapMode::Move => {
                ui.small("Sélectionnez l'ouvrage, puis faites-le glisser avec le bouton gauche. Les raccordements et longueurs électriques sont conservés.");
            }
            MapMode::Vertex => {
                ui.small("Sélectionnez un tronçon, puis glissez ses poignées. Ajoutez ou supprimez des sommets dans ses propriétés.");
            }
            MapMode::Place => {
                egui::ComboBox::from_id_salt("placement_kind")
                    .selected_text(self.placement.label())
                    .show_ui(ui, |ui| {
                        for kind in [
                            Placement::Station,
                            Placement::BusHta,
                            Placement::BusBt,
                            Placement::Load,
                            Placement::Generator,
                            Placement::Source,
                        ] {
                            ui.selectable_value(&mut self.placement, kind, kind.label());
                        }
                    });
                if matches!(
                    self.placement,
                    Placement::Load | Placement::Generator | Placement::Source
                ) {
                    bus_choice(ui, &self.network, "Raccordement", &mut self.attachment);
                }
                ui.small("Cliquez sur la carte pour placer. Les paramètres initiaux sont provisoires : vérifiez-les dans les propriétés.");
            }
            MapMode::Trace => {
                egui::ComboBox::from_id_salt("branch_kind")
                    .selected_text(match self.branch_kind {
                        BranchKind::Line => "Tronçon",
                        BranchKind::Switch => "Interrupteur",
                        BranchKind::Transformer => "Transformateur",
                    })
                    .show_ui(ui, |ui| {
                        for (kind, label) in [
                            (BranchKind::Line, "Tronçon"),
                            (BranchKind::Switch, "Interrupteur"),
                            (BranchKind::Transformer, "Transformateur"),
                        ] {
                            if ui
                                .selectable_value(&mut self.branch_kind, kind, label)
                                .changed()
                            {
                                self.draft = None;
                            }
                        }
                    });
                ui.add_enabled_ui(self.draft.is_none(), |ui| {
                    bus_choice(ui, &self.network, "Borne départ", &mut self.from_bus);
                    bus_choice(ui, &self.network, "Borne arrivée", &mut self.to_bus);
                });
                if self.branch_kind == BranchKind::Line {
                    if self.draft.is_none() && ui.button("Commencer le tracé").clicked() {
                        let r = editor::bus_point(&self.network, self.from_bus).and_then(|start| {
                            editor::bus_point(&self.network, self.to_bus)?;
                            if self.from_bus == self.to_bus {
                                return Err(calcelec::model::Error::Invalid(
                                    "deux bornes distinctes sont requises".into(),
                                ));
                            }
                            self.draft = Some(vec![start]);
                            Ok(
                                "Cliquez les sommets intermédiaires, puis terminez le tracé."
                                    .into(),
                            )
                        });
                        self.report(r);
                    }
                    if let Some(draft) = &mut self.draft {
                        ui.label(format!("{} sommets intermédiaires", draft.len() - 1));
                        if ui
                            .add_enabled(
                                draft.len() > 1,
                                egui::Button::new("Retirer le dernier sommet"),
                            )
                            .clicked()
                        {
                            draft.pop();
                        }
                    }
                    if self.draft.is_some() && ui.button("Terminer le tronçon").clicked() {
                        let intermediate = self.draft.as_ref().unwrap()[1..].to_vec();
                        let r = editor::branch(
                            &self.network,
                            self.branch_kind,
                            self.from_bus,
                            self.to_bus,
                            &intermediate,
                        )
                        .and_then(|op| self.commit_edit("Création d'un tronçon", vec![op]));
                        self.report(r);
                    }
                    ui.small("Les extrémités viennent des bornes choisies. À la création, la longueur électrique est initialisée à celle du tracé ; elle reste ensuite indépendante.");
                } else if ui.button("Créer la liaison entre les bornes").clicked() {
                    let r = editor::branch(
                        &self.network,
                        self.branch_kind,
                        self.from_bus,
                        self.to_bus,
                        &[],
                    )
                    .and_then(|op| self.commit_edit("Création d'une liaison", vec![op]));
                    self.report(r);
                }
                if self.draft.is_some() && ui.button("Abandonner le tracé").clicked() {
                    self.draft = None;
                }
                ui.small("Raccordements explicites ; aucun croisement de tracés ne connecte les ouvrages.");
            }
        }
        ui.separator();
    }
    fn start_calculation(&mut self) {
        if self.calculation.is_some() {
            return;
        }
        let project = self.project.clone();
        let use_unbalanced = self.use_unbalanced;
        let options = if self.simplified_calculation {
            Options::simplified()
        } else {
            Options::default()
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        self.study = None;
        self.unbalanced_study = None;
        self.error = false;
        self.status = "Calcul en cours sur la version enregistrée en mémoire ; les champs non validés sont exclus.".into();
        self.calculation = Some(CalculationTask { receiver, cancel });
        std::thread::spawn(move || {
            let result = if use_unbalanced {
                calcelec::unbalanced::study(
                    &project,
                    &calcelec::unbalanced::Options::default(),
                    &worker_cancel,
                )
                .map(CalculationResult::Unbalanced)
            } else {
                load_flow::study(&project, &options, &worker_cancel)
                    .map(CalculationResult::Balanced)
            };
            let _ = sender.send(result);
        });
    }
    fn poll_calculation(&mut self) {
        let Some(task) = &self.calculation else {
            return;
        };
        let (result, was_cancelled) = match task.receiver.try_recv() {
            Ok(result) => (result, task.cancel.load(Ordering::Relaxed)),
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.calculation = None;
                self.error = true;
                self.status = "Le moteur de calcul s'est interrompu.".into();
                return;
            }
        };
        self.calculation = None;
        if was_cancelled {
            self.error = false;
            self.status = "Calcul annulé ou réseau modifié ; résultat écarté.".into();
            return;
        }
        match result {
            Ok(CalculationResult::Balanced(study))
                if study.project_id == self.project.id
                    && study.network_version == self.project.head() =>
            {
                self.status = format!(
                    "Calcul convergé en {} itérations · résidu {:.2e} pu · pertes {:.3} kW",
                    study.result.iterations,
                    study.result.residual_pu,
                    study.result.total_loss_w / 1000.0
                );
                self.error = false;
                self.results_view = true;
                self.study = Some(study);
            }
            Ok(CalculationResult::Unbalanced(study))
                if study.project_id == self.project.id
                    && study.network_version == self.project.head() =>
            {
                self.status = format!(
                    "Calcul ABCN convergé en {} itérations · résidu {:.2e} pu · pertes {:.3} kW",
                    study.result.iterations,
                    study.result.residual_pu,
                    study.result.total_loss_w / 1000.0
                );
                self.error = false;
                self.results_view = true;
                self.unbalanced_study = Some(study);
            }
            Ok(_) => {
                self.error = false;
                self.status = "Résultat écarté : le projet ou sa version a changé.".into();
            }
            Err(error) => {
                self.error = true;
                self.status = error.to_string();
            }
        }
    }
    fn calculation_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(self.calculation.is_none(),|ui| {
                ui.selectable_value(&mut self.use_unbalanced,false,"Équilibré");
                ui.selectable_value(&mut self.use_unbalanced,true,"Déséquilibré ABCN");
                ui.add_enabled(!self.use_unbalanced,egui::Checkbox::new(&mut self.simplified_calculation,"Autoriser les modèles simplifiés")).on_hover_text("Répartit les puissances monophasées sur trois phases et assimile uk des transformateurs à une réactance, sans pertes actives ni déphasage.");
                if ui.button("Calculer le régime permanent").clicked() {self.start_calculation();}
            });
            if let Some(task)=&self.calculation {
                ui.spinner();
                if ui.button("Annuler le calcul").clicked() {task.cancel.store(true,Ordering::Relaxed);}
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            }
        });
        if self.use_unbalanced {
            ui.small("ABC/ABCN radial : lignes quatre fils et transformateurs Dyn HTA/BT à neutre secondaire directement mis à la terre. Exemples : reference-grounded-bt.json et reference-dyn-transformer.json.");
        } else if self.simplified_calculation {
            ui.small("Hypothèses : équivalents équilibrés des charges monophasées ; transformateurs purement réactifs, rapport nominal, sans déphasage. Aucun calcul du neutre.");
        }
    }
    fn unbalanced_results_table(&mut self, ui: &mut egui::Ui) {
        let Some(study) = &self.unbalanced_study else {
            return;
        };
        let r = &study.result;
        ui.strong("Résultats déséquilibrés — A / B / C / N");
        ui.label(format!(
            "Source : {:.3} kW / {:.3} kvar · Demande nette : {:.3} kW · Pertes : {:.3} kW",
            r.source_p_w / 1000.0,
            r.source_q_var / 1000.0,
            r.net_demand_p_w / 1000.0,
            r.total_loss_w / 1000.0
        ));
        ui.small(format!(
            "{} itérations · résidu {:.2e} pu · bilan {:.3e} VA",
            r.iterations, r.residual_pu, r.balance_error_va
        ));
        ui.small(format!(
            "Pertes des lignes : {:.3} W · transformateurs : {:.3} W · prises de terre : {:.3} W",
            r.line_loss_w, r.transformer_loss_w, r.grounding_loss_w
        ));
        ui.collapsing("Hypothèses et traçabilité du calcul ABCN", |ui| {
            ui.label(format!(
                "{} · réseau {} · tolérance {:.1e} pu",
                study.solver_version, study.network_version, study.options.tolerance_pu
            ));
            for assumption in &r.assumptions {
                ui.label(assumption);
            }
        });
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.result_path);
            if ui.button("Exporter les résultats JSON").clicked() {
                let result = serde_json::to_string(study)
                    .map_err(Into::into)
                    .and_then(|json| storage::export_new(&self.result_path, &json));
                self.error = result.is_err();
                self.status = match result {
                    Ok(()) => {
                        "Résultats ABCN exportés avec phaseurs, version et hypothèses.".into()
                    }
                    Err(e) => e.to_string(),
                };
            }
        });
        let mut clicked = None;
        ui.small("Tensions phase-neutre, ou phase-terre sur un bus ABC : V / angle (°) ; |VN| : potentiel du neutre lorsqu'il existe.");
        egui::Grid::new("unbalanced_buses")
            .striped(true)
            .show(ui, |ui| {
                for title in [
                    "Jeu de barres",
                    "A-N/terre",
                    "B-N/terre",
                    "C-N/terre",
                    "|VN| (V)",
                    "U2/U1 (%)",
                ] {
                    ui.strong(title);
                }
                ui.end_row();
                let mut buses: Vec<_> = r.buses.iter().collect();
                buses.sort_by_key(|b| &self.network.objects[&b.id].name);
                for bus in buses {
                    if ui
                        .selectable_label(
                            self.selected == Some(bus.id),
                            &self.network.objects[&bus.id].name,
                        )
                        .clicked()
                    {
                        clicked = Some(bus.id);
                    }
                    for v in &bus.phase_neutral {
                        ui.colored_label(
                            if v.outside_voltage_limits {
                                Color32::LIGHT_RED
                            } else {
                                Color32::LIGHT_GREEN
                            },
                            format!("{:.2} / {:.1}°", v.magnitude_v, v.angle_deg),
                        );
                    }
                    ui.label(format!("{:.3}", bus.neutral_displacement_v));
                    ui.label(format!("{:.3}", bus.negative_sequence_percent));
                    ui.end_row();
                }
            });
        ui.separator();
        ui.strong("Courants par conducteur côté aval — sens source vers aval");
        egui::Grid::new("unbalanced_branches")
            .striped(true)
            .show(ui, |ui| {
                for title in [
                    "Branche",
                    "IA (A)",
                    "IB (A)",
                    "IC (A)",
                    "IN (A)",
                    "Charge (%)",
                    "Pertes (W)",
                ] {
                    ui.strong(title);
                }
                ui.end_row();
                let mut branches: Vec<_> = r.branches.iter().collect();
                branches.sort_by_key(|b| &self.network.objects[&b.id].name);
                for b in branches {
                    if ui
                        .selectable_label(
                            self.selected == Some(b.id),
                            &self.network.objects[&b.id].name,
                        )
                        .clicked()
                    {
                        clicked = Some(b.id);
                    }
                    for current in b.currents_a {
                        ui.label(format!("{:.2}", current.magnitude()));
                    }
                    ui.colored_label(
                        if b.loading_percent.is_some_and(|p| p > 100.0) {
                            Color32::LIGHT_RED
                        } else {
                            Color32::LIGHT_GREEN
                        },
                        b.loading_percent
                            .map(|p| format!("{p:.1}"))
                            .unwrap_or_else(|| "—".into()),
                    );
                    ui.label(format!("{:.2}", b.loss_w));
                    ui.end_row();
                }
            });
        ui.separator();
        ui.strong("Prises de terre du neutre");
        ui.small("Courant positif du neutre vers la référence de terre commune ; angles relatifs à la phase A source.");
        egui::Grid::new("neutral_groundings")
            .striped(true)
            .show(ui, |ui| {
                for title in [
                    "Jeu de barres",
                    "I terre (A)",
                    "Angle (°)",
                    "|VN| (V)",
                    "Pertes (W)",
                ] {
                    ui.strong(title);
                }
                ui.end_row();
                let mut groundings: Vec<_> = r.groundings.iter().collect();
                groundings.sort_by_key(|g| &self.network.objects[&g.bus_id].name);
                for g in groundings {
                    if ui
                        .selectable_label(
                            self.selected == Some(g.bus_id),
                            &self.network.objects[&g.bus_id].name,
                        )
                        .clicked()
                    {
                        clicked = Some(g.bus_id);
                    }
                    let magnitude = g.current_to_earth_a.magnitude();
                    let angle = if magnitude < 1e-12 {
                        0.0
                    } else {
                        g.current_to_earth_a
                            .im
                            .atan2(g.current_to_earth_a.re)
                            .to_degrees()
                    };
                    ui.label(format!("{magnitude:.3}"));
                    ui.label(format!("{angle:.2}"));
                    ui.label(format!("{:.3}", g.voltage_v.magnitude()));
                    ui.label(format!("{:.3}", g.loss_w));
                    ui.end_row();
                }
            });
        if let Some(id) = clicked {
            self.select(id);
        }
    }
    fn results_table(&mut self, ui: &mut egui::Ui) {
        if self.unbalanced_study.is_some() {
            self.unbalanced_results_table(ui);
            return;
        }
        let Some(study) = &self.study else {
            ui.label("Aucun résultat pour cette version. Lancez un calcul ; l'exemple rural nécessite les modèles simplifiés.");
            return;
        };
        let r = &study.result;
        ui.label(format!(
            "Source : {:.3} kW / {:.3} kvar · Demande nette : {:.3} kW · Pertes : {:.3} kW",
            r.source_p_w / 1000.0,
            r.source_q_var / 1000.0,
            r.net_demand_p_w / 1000.0,
            r.total_loss_w / 1000.0
        ));
        ui.small(format!(
            "{} itérations · résidu {:.2e} pu · erreur de bilan {:.3e} VA",
            r.iterations, r.residual_pu, r.balance_error_va
        ));
        ui.collapsing("Hypothèses et traçabilité du résultat", |ui| {
            ui.label(format!("Version réseau : {}", study.network_version));
            ui.label(format!(
                "Moteur : {} · tolérance {:.1e} pu",
                study.solver_version, study.options.tolerance_pu
            ));
            for assumption in &r.assumptions {
                ui.label(assumption);
            }
        });
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.result_path);
            if ui.button("Exporter les résultats JSON").clicked() {
                let result = serde_json::to_string(study)
                    .map_err(Into::into)
                    .and_then(|json| storage::export_new(&self.result_path, &json));
                self.error = result.is_err();
                self.status = match result {
                    Ok(()) => "Résultats exportés avec version, options et hypothèses.".into(),
                    Err(e) => e.to_string(),
                };
            }
        });
        let mut clicked = None;
        ui.strong("Tensions composées — équivalent équilibré");
        egui::Grid::new("bus_results").striped(true).show(ui, |ui| {
            for title in ["Jeu de barres", "U (V)", "U (pu)", "Angle (°)", "État"] {
                ui.strong(title);
            }
            ui.end_row();
            let mut buses: Vec<_> = r.buses.iter().collect();
            buses.sort_by_key(|b| &self.network.objects[&b.id].name);
            for b in buses {
                if ui
                    .selectable_label(
                        self.selected == Some(b.id),
                        &self.network.objects[&b.id].name,
                    )
                    .clicked()
                {
                    clicked = Some(b.id);
                }
                ui.label(format!("{:.2}", b.voltage_ll_v));
                ui.label(format!("{:.5}", b.voltage_pu));
                ui.label(format!("{:.3}", b.angle_deg));
                ui.colored_label(
                    if b.outside_voltage_limits {
                        Color32::LIGHT_RED
                    } else {
                        Color32::LIGHT_GREEN
                    },
                    if b.outside_voltage_limits {
                        "Hors seuils"
                    } else {
                        "Dans les seuils"
                    },
                );
                ui.end_row();
            }
        });
        ui.separator();
        ui.strong("Branches — sens de la source vers l'aval");
        egui::Grid::new("branch_results")
            .striped(true)
            .show(ui, |ui| {
                for title in [
                    "Branche",
                    "I amont (A)",
                    "I aval (A)",
                    "Charge (%)",
                    "Pertes (W)",
                ] {
                    ui.strong(title);
                }
                ui.end_row();
                let mut branches: Vec<_> = r.branches.iter().collect();
                branches.sort_by_key(|b| &self.network.objects[&b.id].name);
                for b in branches {
                    if ui
                        .selectable_label(
                            self.selected == Some(b.id),
                            &self.network.objects[&b.id].name,
                        )
                        .clicked()
                    {
                        clicked = Some(b.id);
                    }
                    ui.label(format!("{:.2}", b.current_from_a));
                    ui.label(format!("{:.2}", b.current_to_a));
                    ui.colored_label(
                        if b.loading_percent.is_some_and(|p| p > 100.0) {
                            Color32::LIGHT_RED
                        } else {
                            Color32::LIGHT_GREEN
                        },
                        b.loading_percent
                            .map(|p| format!("{p:.1}"))
                            .unwrap_or_else(|| "—".into()),
                    );
                    ui.label(format!("{:.2}", b.loss_w));
                    ui.end_row();
                }
            });
        if let Some(id) = clicked {
            self.select(id);
        }
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
            self.equipment_editor(ui);
            ui.collapsing("Modèle électrique détaillé — édition avancée",|ui| {
                ui.small("Conducteurs, matrices, enroulements et mises à la terre. Les valeurs préremplies sont à vérifier avant validation.");
                if ui.button("Préremplir une structure à compléter").clicked() {
                    let mut draft_object=object.clone();draft_object.equipment=self.equipment.clone();
                    self.electrical_text=serde_json::to_string_pretty(&calcelec::electrical::editing_template(&draft_object,&self.network)).unwrap();
                }
                ui.add(egui::TextEdit::multiline(&mut self.electrical_text).code_editor().desired_rows(12).desired_width(f32::INFINITY));
                if ui.button("Retirer du brouillon les données détaillées").clicked() {self.electrical_text.clear();}
                ui.small("Valider avec Créer une version. Un champ vide conserve seulement le modèle historique simplifié.");
                for missing in calcelec::electrical::missing_data(&self.network).iter().filter(|m|m.object_id==id) {ui.label(&missing.message);}
            });
            if self.geographic {
                self.geometry_editor(ui, &object);
            } else {
                ui.separator();
                ui.label("Géométrie géographique");
                ui.small("Lecture seule dans la vue unifilaire. Revenez à la vue géographique pour déplacer l’ouvrage ou modifier son tracé.");
            }
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
            ui.collapsing("Supprimer l'ouvrage", |ui| {
                let linked = editor::dependents(&self.network, id);
                if linked.is_empty() {
                    if ui.button("Supprimer cet ouvrage").clicked() {
                        let r = editor::deletion(&self.network, id, false)
                            .and_then(|ops| self.commit_edit("Suppression d'un ouvrage", ops));
                        self.report(r);
                    }
                } else {
                    ui.label("Cet ouvrage est référencé par :");
                    for other in &linked {
                        ui.label(&self.network.objects[other].name);
                    }
                    if ui
                        .button("Supprimer l'ouvrage ET les dépendances listées")
                        .clicked()
                    {
                        let r = editor::deletion(&self.network, id, true).and_then(|ops| {
                            self.commit_edit("Suppression avec dépendances explicites", ops)
                        });
                        self.report(r);
                    }
                }
            });
        } else {
            ui.label("Sélectionnez un objet dans le tableau ou le schéma.");
        }
    }
    fn equipment_editor(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Paramètres électriques et raccordements", |ui| {
            match &mut self.equipment {
                Equipment::Bus { voltage_v } => {
                    numeric(ui, "Tension composée nominale (V)", voltage_v)
                }
                Equipment::Source { bus, voltage_pu } => {
                    if self.geographic {
                        required_bus_choice(ui, &self.network, "Jeu de barres", bus);
                    } else {
                        readonly_bus(ui, &self.network, "Jeu de barres", *bus);
                    }
                    numeric(ui, "Tension source (pu)", voltage_pu);
                }
                Equipment::Line {
                    from,
                    to,
                    length_m,
                    r_ohm_per_km,
                    x_ohm_per_km,
                    max_current_a,
                } => {
                    if self.geographic {
                        required_bus_choice(ui, &self.network, "Départ", from);
                        required_bus_choice(ui, &self.network, "Arrivée", to);
                    } else {
                        readonly_bus(ui, &self.network, "Départ", *from);
                        readonly_bus(ui, &self.network, "Arrivée", *to);
                    }
                    numeric(ui, "Longueur électrique (m)", length_m);
                    numeric(ui, "R (ohm/km)", r_ohm_per_km);
                    numeric(ui, "X (ohm/km)", x_ohm_per_km);
                    numeric(ui, "Courant admissible (A)", max_current_a);
                }
                Equipment::Transformer {
                    from,
                    to,
                    rated_va,
                    uk_percent,
                } => {
                    if self.geographic {
                        required_bus_choice(ui, &self.network, "Côté 1", from);
                        required_bus_choice(ui, &self.network, "Côté 2", to);
                    } else {
                        readonly_bus(ui, &self.network, "Côté 1", *from);
                        readonly_bus(ui, &self.network, "Côté 2", *to);
                    }
                    numeric(ui, "Puissance assignée (VA)", rated_va);
                    numeric(ui, "uk (%)", uk_percent);
                }
                Equipment::Load { bus, phase, .. } | Equipment::Generator { bus, phase, .. } => {
                    if self.geographic {
                        required_bus_choice(ui, &self.network, "Jeu de barres", bus);
                    } else {
                        readonly_bus(ui, &self.network, "Jeu de barres", *bus);
                    }
                    egui::ComboBox::from_id_salt("phase")
                        .selected_text(format!("{phase:?}"))
                        .show_ui(ui, |ui| {
                            for value in [
                                calcelec::model::Phase::Abc,
                                calcelec::model::Phase::A,
                                calcelec::model::Phase::B,
                                calcelec::model::Phase::C,
                            ] {
                                ui.selectable_value(phase, value, format!("{value:?}"));
                            }
                        });
                }
                Equipment::Switch { from, to, closed } => {
                    if self.geographic {
                        required_bus_choice(ui, &self.network, "Départ", from);
                        required_bus_choice(ui, &self.network, "Arrivée", to);
                    } else {
                        readonly_bus(ui, &self.network, "Départ", *from);
                        readonly_bus(ui, &self.network, "Arrivée", *to);
                    }
                    ui.checkbox(closed, "Fermé");
                }
            }
            ui.small("Les changements de bornes sont explicites et ne déplacent pas la géométrie.");
        });
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
        // The layout is derived on every frame: topology determines columns and
        // geographic northing determines the order inside each column. Nothing
        // is persisted as a second, independently editable geometry.
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
        buses.sort_by(|a, b| diagram_northing_order(a, b));
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
        let mut object_positions = BTreeMap::new();
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
                object_positions.insert(o.id, a.lerp(*b, 0.5));
            }
        }
        let mut attachments: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
        for object in self.network.objects.values() {
            if !matches!(object.equipment, Equipment::Bus { .. }) && object.buses().len() == 1 {
                attachments
                    .entry(object.buses()[0])
                    .or_default()
                    .push(object.id);
            }
        }
        for (bus, ids) in &mut attachments {
            ids.sort_by(|a, b| {
                diagram_northing(&self.network.objects[b])
                    .total_cmp(&diagram_northing(&self.network.objects[a]))
                    .then_with(|| {
                        self.network.objects[a]
                            .name
                            .cmp(&self.network.objects[b].name)
                    })
            });
            let Some(bus_pos) = positions.get(bus) else {
                continue;
            };
            for (index, id) in ids.iter().enumerate() {
                let offset = (index as f32 - (ids.len() - 1) as f32 / 2.0) * 24.0;
                let pos = *bus_pos + Vec2::new(0.0, offset - 38.0);
                painter.line_segment([*bus_pos, pos], Stroke::new(1.5, Color32::DARK_GRAY));
                object_positions.insert(*id, pos);
            }
        }
        let mut clicked = None;
        for (id, pos) in &positions {
            let o = &self.network.objects[id];
            let color = if Some(*id) == self.selected {
                Color32::YELLOW
            } else if o.in_service {
                Color32::from_rgb(70, 190, 175)
            } else {
                Color32::GRAY
            };
            painter.circle_filled(*pos, 6.0, color);
            painter.text(
                *pos + Vec2::new(0.0, 14.0),
                egui::Align2::CENTER_TOP,
                &o.name,
                egui::FontId::proportional(11.0),
                color,
            );
            if ui
                .interact(
                    egui::Rect::from_center_size(*pos, Vec2::splat(24.0)),
                    ui.scope_id().with(*id),
                    egui::Sense::click(),
                )
                .on_hover_text(&o.name)
                .clicked()
            {
                clicked = Some(*id);
            }
        }
        for (id, pos) in object_positions {
            let object = &self.network.objects[&id];
            let color = if Some(id) == self.selected {
                Color32::YELLOW
            } else if object.in_service {
                Color32::from_rgb(110, 180, 220)
            } else {
                Color32::GRAY
            };
            if matches!(object.equipment, Equipment::Transformer { .. }) {
                painter.circle_stroke(pos + Vec2::new(-5.0, 0.0), 7.0, Stroke::new(2.0, color));
                painter.circle_stroke(pos + Vec2::new(5.0, 0.0), 7.0, Stroke::new(2.0, color));
            } else {
                painter.rect_filled(
                    egui::Rect::from_center_size(pos, Vec2::splat(10.0)),
                    2.0,
                    color,
                );
            }
            let response = ui
                .interact(
                    egui::Rect::from_center_size(pos, Vec2::splat(24.0)),
                    ui.scope_id().with(("unifilaire", id)),
                    egui::Sense::click(),
                )
                .on_hover_text(format!("{} · {}", object.name, object.kind()));
            if response.clicked() {
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
        self.poll_calculation();
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.draft = None;
            self.map.cancel_drag();
            self.tool = MapMode::Select;
        }
        egui::Panel::top("header").show(ui, |ui| {
            ui.heading("CalcElec / Distribution publique");
            ui.label(&self.network.name);
            ui.horizontal(|ui| {
                ui.label(format!("{} objets · {} versions enregistrées en mémoire",self.network.objects.len(),self.project.changes.len()+1));
                ui.small(format!("Version {}",self.project.head()));
            });
            ui.colored_label(Color32::from_rgb(230,180,75),"Prototype — calculs AC équilibré radial et BT ABCN disponibles ; courts-circuits à venir. Données synthétiques.");
            self.calculation_controls(ui);
            let missing=calcelec::electrical::missing_data(&self.network);
            ui.collapsing(format!("Données détaillées : {} informations à compléter",missing.len()),|ui| {
                ui.small("Inventaire des données déclarées ; ce diagnostic ne remplace pas la validation d'un moteur déséquilibré ou de défaut.");
                egui::ScrollArea::vertical().max_height(120.0).show(ui,|ui| {
                    for item in missing {ui.label(format!("{} : {}",self.network.objects[&item.object_id].name,item.message));}
                });
            });
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
                    self.toolbox(ui);
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
                let changed = ui
                    .selectable_value(&mut self.geographic, false, "Vue unifilaire dérivée")
                    .changed();
                if changed && !self.geographic {
                    self.tool = MapMode::Select;
                    self.draft = None;
                    self.map.cancel_drag();
                }
            });
            if self.geographic {
                let preview = self.draft.clone().unwrap_or_default();
                if let Some(event) = self.map.show(
                    ui,
                    &self.network,
                    self.selected,
                    self.study
                        .as_ref()
                        .map(|s| MapResults::Balanced(&s.result))
                        .or_else(|| {
                            self.unbalanced_study
                                .as_ref()
                                .map(|s| MapResults::Unbalanced(&s.result))
                        }),
                    MapTools {
                        mode: self.tool,
                        draft: &preview,
                    },
                ) {
                    self.map_event(event);
                }
            } else {
                self.diagram(ui);
            }
            ui.label(format!(
                "Connectivité : {} jeux de barres en service sans source accessible",
                self.network.unpowered_buses().len()
            ));
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.results_view, false, "Équipements");
                ui.selectable_value(&mut self.results_view, true, "Résultats du calcul");
            });
            let mut clicked = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.results_view {
                    self.results_table(ui);
                    return;
                }
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
    fn unifilar_order_is_derived_from_geography_and_has_no_editing_tool() {
        let network = rural_example();
        let mut buses: Vec<_> = network
            .objects
            .values()
            .filter(|object| matches!(object.equipment, Equipment::Bus { .. }))
            .take(2)
            .cloned()
            .collect();
        buses[0].geometry = Geometry::Point(Coordinate(0.0, 10.0));
        buses[1].geometry = Geometry::Point(Coordinate(0.0, 20.0));
        buses.sort_by(diagram_northing_order);
        assert_eq!(diagram_northing(&buses[0]), 20.0);
        buses[0].geometry = Geometry::Point(Coordinate(0.0, -10.0));
        buses.sort_by(diagram_northing_order);
        assert_eq!(diagram_northing(&buses[0]), 10.0);

        let mut app = Desktop::new();
        let project = app.project.clone();
        app.geographic = false;
        app.tool = MapMode::Move;
        let context = egui::Context::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| app.render(ui));
        output.textures_delta.clear();
        assert!(app.tool == MapMode::Select);
        assert_eq!(app.project, project);
    }

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
        app.study = Some(
            load_flow::study(
                &app.project,
                &Options::simplified(),
                &AtomicBool::new(false),
            )
            .unwrap(),
        );
        app.results_view = true;
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
        app.results_view = false;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| app.render(ui));
        output.textures_delta.clear();
    }
    fn deliver(app: &mut Desktop, report: StudyResult, cancelled: bool) {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(CalculationResult::Balanced(report)))
            .unwrap();
        app.calculation = Some(CalculationTask {
            receiver,
            cancel: Arc::new(AtomicBool::new(cancelled)),
        });
        app.poll_calculation();
    }
    #[test]
    fn results_are_invalidated_on_edit_and_late_results_are_discarded() {
        let mut app = Desktop::new();
        let report = load_flow::study(
            &app.project,
            &Options::simplified(),
            &AtomicBool::new(false),
        )
        .unwrap();
        deliver(&mut app, report.clone(), false);
        assert!(app.study.is_some());
        let id = *app.network.objects.keys().next().unwrap();
        app.select(id);
        app.name = "Nom modifié".into();
        app.record().unwrap();
        assert!(app.study.is_none());
        deliver(&mut app, report, false);
        assert!(app.study.is_none());
    }
    #[test]
    fn cancellation_discards_even_a_completed_calculation() {
        let mut app = Desktop::new();
        let report = load_flow::study(
            &app.project,
            &Options::simplified(),
            &AtomicBool::new(false),
        )
        .unwrap();
        deliver(&mut app, report, true);
        assert!(app.study.is_none());
        assert!(app.calculation.is_none());
        assert!(!app.error);
    }
    #[test]
    fn toolbox_placement_and_geometry_edit_use_history_and_invalidate_results() {
        let mut app = Desktop::new();
        let before = app.network.clone();
        app.study = Some(
            load_flow::study(
                &app.project,
                &Options::simplified(),
                &AtomicBool::new(false),
            )
            .unwrap(),
        );
        app.tool = MapMode::Place;
        app.placement = Placement::BusBt;
        app.map_event(MapEvent::Click(Coordinate(5000.0, 2000.0)));
        assert!(!app.error);
        assert!(app.study.is_none());
        assert_eq!(app.network.objects.len(), before.objects.len() + 1);
        let id = app.selected.unwrap();
        app.map_event(MapEvent::SetGeometry {
            id,
            geometry: Geometry::Point(Coordinate(5100.0, 2000.0)),
        });
        assert_eq!(app.project.changes.len(), 2);
        app.history.undo(&mut app.project, &app.author).unwrap();
        app.refresh().unwrap();
        assert_eq!(
            app.network.objects[&id].geometry,
            Geometry::Point(Coordinate(5000.0, 2000.0))
        );
        app.history.undo(&mut app.project, &app.author).unwrap();
        app.refresh().unwrap();
        assert_eq!(app.network, before);
    }
    #[test]
    fn advanced_electrical_edit_is_versioned_and_invalid_json_is_atomic() {
        let mut app = Desktop::new();
        let id = app
            .network
            .objects
            .values()
            .find(|o| o.name == "Consommation agrégée 2")
            .unwrap()
            .id;
        app.select(id);
        app.electrical_text = serde_json::to_string(&calcelec::electrical::editing_template(
            &app.network.objects[&id],
            &app.network,
        ))
        .unwrap();
        app.record().unwrap();
        assert!(app.network.objects[&id].electrical.is_some());
        assert_eq!(app.project.changes.len(), 1);
        let before = app.project.clone();
        app.electrical_text = "{invalide".into();
        assert!(app.record().is_err());
        assert_eq!(app.project, before);
        app.history.undo(&mut app.project, &app.author).unwrap();
        app.refresh().unwrap();
        assert!(app.network.objects[&id].electrical.is_none());
    }
    #[test]
    fn unbalanced_results_render_and_are_discarded_after_a_version_change() {
        let mut app = Desktop::new();
        app.network = calcelec::reference_cases::unbalanced_bt();
        app.project = Project::new(app.network.clone()).unwrap();
        app.use_unbalanced = true;
        let report = calcelec::unbalanced::study(
            &app.project,
            &calcelec::unbalanced::Options::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let deliver = |app: &mut Desktop, report: calcelec::unbalanced::StudyResult| {
            let (sender, receiver) = mpsc::channel();
            sender
                .send(Ok(CalculationResult::Unbalanced(report)))
                .unwrap();
            app.calculation = Some(CalculationTask {
                receiver,
                cancel: Arc::new(AtomicBool::new(false)),
            });
            app.poll_calculation();
        };
        deliver(&mut app, report.clone());
        assert!(app.unbalanced_study.is_some());
        assert!(app.results_view);
        let context = egui::Context::default();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1280.0, 1024.0),
                )),
                ..Default::default()
            },
            |ui| app.render(ui),
        );
        assert!(!output.shapes.is_empty());
        output.textures_delta.clear();
        let id = *app.network.objects.keys().next().unwrap();
        app.select(id);
        app.name = "Nom corrigé".into();
        app.record().unwrap();
        assert!(app.unbalanced_study.is_none());
        deliver(&mut app, report);
        assert!(app.unbalanced_study.is_none());
    }
    #[test]
    fn grounding_results_render_with_separate_losses_and_are_invalidated_after_edit() {
        let mut app = Desktop::new();
        app.network = calcelec::reference_cases::grounded_bt();
        app.project = Project::new(app.network.clone()).unwrap();
        app.use_unbalanced = true;
        app.results_view = true;
        let report = calcelec::unbalanced::study(
            &app.project,
            &calcelec::unbalanced::Options::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(report.result.groundings.len(), 3);
        assert!(report.result.grounding_loss_w > 0.0);
        app.unbalanced_study = Some(report);
        let context = egui::Context::default();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1280.0, 1024.0),
                )),
                ..Default::default()
            },
            |ui| app.render(ui),
        );
        assert!(!output.shapes.is_empty());
        output.textures_delta.clear();
        let id = app
            .network
            .objects
            .values()
            .find(|o| o.name == "Point de terre intermédiaire")
            .unwrap()
            .id;
        app.select(id);
        let mut data = app.network.objects[&id].electrical.clone().unwrap();
        let calcelec::electrical::ElectricalData::Bus(bus) = &mut data else {
            unreachable!()
        };
        bus.neutral_grounding = calcelec::electrical::Grounding::Isolated;
        app.electrical_text = serde_json::to_string(&data).unwrap();
        app.record().unwrap();
        assert!(app.unbalanced_study.is_none());
    }
}
