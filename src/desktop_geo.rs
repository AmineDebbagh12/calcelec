use calcelec::{
    geometry::{Coordinate, Geometry},
    model::{Equipment, Id, Network},
};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

#[derive(Clone, Copy)]
pub enum MapResults<'a> {
    Balanced(&'a calcelec::load_flow::LoadFlow),
    Unbalanced(&'a calcelec::unbalanced::LoadFlow),
}
impl MapResults<'_> {
    fn overloaded(self, id: Id) -> bool {
        match self {
            Self::Balanced(r) => r
                .branches
                .iter()
                .any(|b| b.id == id && b.loading_percent.is_some_and(|p| p > 100.0)),
            Self::Unbalanced(r) => r
                .branches
                .iter()
                .any(|b| b.id == id && b.loading_percent.is_some_and(|p| p > 100.0)),
        }
    }
    fn outside(self, id: Id) -> bool {
        match self {
            Self::Balanced(r) => r
                .buses
                .iter()
                .any(|b| b.id == id && b.outside_voltage_limits),
            Self::Unbalanced(r) => r
                .buses
                .iter()
                .any(|b| b.id == id && b.phase_neutral.iter().any(|p| p.outside_voltage_limits)),
        }
    }
    fn value(self, id: Id) -> Option<String> {
        match self {
            Self::Balanced(r) => r
                .buses
                .iter()
                .find(|b| b.id == id)
                .map(|b| format!("{:.2} V ({:.4} pu)", b.voltage_ll_v, b.voltage_pu))
                .or_else(|| {
                    r.branches.iter().find(|b| b.id == id).map(|b| {
                        format!(
                            "{:.2} A amont / {:.2} A aval",
                            b.current_from_a, b.current_to_a
                        )
                    })
                }),
            Self::Unbalanced(r) => r
                .buses
                .iter()
                .find(|b| b.id == id)
                .map(|b| {
                    format!(
                        "A-N {:.2} V · B-N {:.2} V · C-N {:.2} V · VN {:.3} V",
                        b.phase_neutral[0].magnitude_v,
                        b.phase_neutral[1].magnitude_v,
                        b.phase_neutral[2].magnitude_v,
                        b.neutral_displacement_v
                    )
                })
                .or_else(|| {
                    r.branches.iter().find(|b| b.id == id).map(|b| {
                        format!(
                            "IA {:.2} A · IB {:.2} A · IC {:.2} A · IN {:.2} A",
                            b.currents_a[0].magnitude(),
                            b.currents_a[1].magnitude(),
                            b.currents_a[2].magnitude(),
                            b.currents_a[3].magnitude()
                        )
                    })
                }),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum MapMode {
    #[default]
    Select,
    Move,
    Vertex,
    Place,
    Trace,
}
pub struct MapTools<'a> {
    pub mode: MapMode,
    pub draft: &'a [Coordinate],
}
#[derive(Debug)]
pub enum MapEvent {
    Select(Id),
    Click(Coordinate),
    SetGeometry { id: Id, geometry: Geometry },
}
struct GeometryDrag {
    id: Id,
    origin: Coordinate,
    before: Geometry,
    preview: Geometry,
    vertex: Option<usize>,
}
#[derive(Default)]
pub struct MapView {
    camera: Option<Camera>,
    drag: Option<GeometryDrag>,
    mode: MapMode,
    #[cfg(test)]
    last_rect: Option<Rect>,
}
#[derive(Clone, Copy)]
struct Camera {
    center: Coordinate,
    pixels_per_m: f64,
}
impl Camera {
    fn screen(self, point: Coordinate, rect: Rect) -> Pos2 {
        Pos2::new(
            rect.center().x + ((point.0 - self.center.0) * self.pixels_per_m) as f32,
            rect.center().y - ((point.1 - self.center.1) * self.pixels_per_m) as f32,
        )
    }
    fn world(self, point: Pos2, rect: Rect) -> Coordinate {
        Coordinate(
            self.center.0 + (point.x - rect.center().x) as f64 / self.pixels_per_m,
            self.center.1 - (point.y - rect.center().y) as f64 / self.pixels_per_m,
        )
    }
    fn fit(network: &Network, rect: Rect) -> Option<Self> {
        let points: Vec<_> = network
            .objects
            .values()
            .flat_map(|o| o.geometry.coordinates())
            .collect();
        let first = points.first()?;
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.0, first.0, first.1, first.1);
        for p in points {
            min_x = min_x.min(p.0);
            max_x = max_x.max(p.0);
            min_y = min_y.min(p.1);
            max_y = max_y.max(p.1);
        }
        Some(Self {
            center: Coordinate((min_x + max_x) / 2.0, (min_y + max_y) / 2.0),
            pixels_per_m: (((rect.width() - 100.0).max(10.0) as f64 / (max_x - min_x).max(100.0))
                .min((rect.height() - 90.0).max(10.0) as f64 / (max_y - min_y).max(100.0)))
            .clamp(1e-7, 100.0),
        })
    }
}
fn segment_distance(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((point - a).dot(ab) / ab.length_sq().max(1e-10)).clamp(0.0, 1.0);
    point.distance(a + ab * t)
}
fn grid_step(target: f64) -> f64 {
    let magnitude = 10.0_f64.powf(target.log10().floor());
    let normalized = target / magnitude;
    magnitude
        * if normalized <= 1.0 {
            1.0
        } else if normalized <= 2.0 {
            2.0
        } else if normalized <= 5.0 {
            5.0
        } else {
            10.0
        }
}
impl MapView {
    pub fn reset(&mut self) {
        self.camera = None;
        self.drag = None;
    }
    pub fn cancel_drag(&mut self) {
        self.drag = None;
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        network: &Network,
        selected: Option<Id>,
        results: Option<MapResults<'_>>,
        tools: MapTools<'_>,
    ) -> Option<MapEvent> {
        if self.mode != tools.mode {
            self.drag = None;
            self.mode = tools.mode;
        }
        let mut event = None;
        let mut fit = false;
        ui.horizontal(|ui| {
            fit = ui.button("Cadrer le réseau").clicked();
            ui.label(network.coordinate_reference.label());
        });
        ui.small("Molette : zoom · Glisser droit : déplacer la vue · Échap : abandonner l'action · Sélection : clics répétés pour les objets superposés");
        let height = (ui.available_height() * 0.55).clamp(220.0, 450.0);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), height),
            Sense::click_and_drag(),
        );
        #[cfg(test)]
        {
            self.last_rect = Some(rect);
        }
        if fit || self.camera.is_none() {
            self.camera = Some(Camera::fit(network, rect).unwrap_or(Camera {
                center: Coordinate(0.0, 0.0),
                pixels_per_m: 1.0,
            }));
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(19, 29, 38));
        let unlocated = network
            .objects
            .values()
            .filter(|o| matches!(o.geometry, Geometry::Unlocated))
            .count();
        let mut camera = self.camera.unwrap();
        if response.dragged_by(egui::PointerButton::Secondary)
            || (!matches!(tools.mode, MapMode::Move | MapMode::Vertex)
                && response.dragged_by(egui::PointerButton::Primary))
        {
            let drag = response.drag_delta();
            camera.center.0 -= drag.x as f64 / camera.pixels_per_m;
            camera.center.1 += drag.y as f64 / camera.pixels_per_m;
        }
        if self.drag.is_none()
            && response.hovered()
            && let Some(pointer) = ui.input(|i| i.pointer.hover_pos())
        {
            let factor = ui.input(|i| {
                let pinch = i.zoom_delta();
                if pinch != 1.0 {
                    pinch as f64
                } else {
                    (i.smooth_scroll_delta.y as f64 * 0.003).exp()
                }
            });
            let anchor = camera.world(pointer, rect);
            camera.pixels_per_m = (camera.pixels_per_m * factor).clamp(1e-7, 100.0);
            let after = camera.world(pointer, rect);
            camera.center.0 += anchor.0 - after.0;
            camera.center.1 += anchor.1 - after.1;
        }
        self.camera = Some(camera);
        if response.drag_started_by(egui::PointerButton::Primary)
            && matches!(tools.mode, MapMode::Move | MapMode::Vertex)
            && let Some(object) = selected.and_then(|id| network.objects.get(&id))
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
        {
            let vertex = if tools.mode == MapMode::Vertex {
                object
                    .geometry
                    .coordinates()
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| camera.screen(**p, rect).distance(press) < 12.0)
                    .min_by(|(_, a), (_, b)| {
                        camera
                            .screen(**a, rect)
                            .distance(press)
                            .total_cmp(&camera.screen(**b, rect).distance(press))
                    })
                    .map(|(i, _)| i)
            } else {
                None
            };
            let hit = match &object.geometry {
                Geometry::Point(p) => camera.screen(*p, rect).distance(press) < 12.0,
                Geometry::Polyline(points) => points.windows(2).any(|s| {
                    segment_distance(press, camera.screen(s[0], rect), camera.screen(s[1], rect))
                        < 10.0
                }),
                Geometry::Unlocated => false,
            };
            if (tools.mode == MapMode::Move && hit)
                || (tools.mode == MapMode::Vertex
                    && vertex.is_some()
                    && matches!(object.geometry, Geometry::Polyline(_)))
            {
                self.drag = Some(GeometryDrag {
                    id: object.id,
                    origin: camera.world(press, rect),
                    before: object.geometry.clone(),
                    preview: object.geometry.clone(),
                    vertex,
                });
            }
        }
        if let Some(drag) = &mut self.drag
            && let Some(cursor) = response.interact_pointer_pos()
        {
            let target = camera.world(cursor, rect);
            let dx = target.0 - drag.origin.0;
            let dy = target.1 - drag.origin.1;
            drag.preview = match &drag.before {
                Geometry::Point(p) => Geometry::Point(Coordinate(p.0 + dx, p.1 + dy)),
                Geometry::Polyline(points) => Geometry::Polyline(
                    points
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            if drag.vertex.is_none_or(|v| v == i) {
                                Coordinate(p.0 + dx, p.1 + dy)
                            } else {
                                *p
                            }
                        })
                        .collect(),
                ),
                Geometry::Unlocated => Geometry::Unlocated,
            };
        }
        let step = grid_step(90.0 / camera.pixels_per_m);
        let left = camera.world(rect.left_bottom(), rect);
        let right = camera.world(rect.right_top(), rect);
        for i in 0..=((right.0 - left.0) / step).ceil().min(100.0) as usize {
            let x = (left.0 / step).ceil() * step + i as f64 * step;
            let p = camera.screen(Coordinate(x, 0.0), rect);
            painter.line_segment(
                [Pos2::new(p.x, rect.top()), Pos2::new(p.x, rect.bottom())],
                Stroke::new(1.0, Color32::from_rgb(31, 44, 55)),
            );
        }
        for i in 0..=((right.1 - left.1) / step).ceil().min(100.0) as usize {
            let y = (left.1 / step).ceil() * step + i as f64 * step;
            let p = camera.screen(Coordinate(0.0, y), rect);
            painter.line_segment(
                [Pos2::new(rect.left(), p.y), Pos2::new(rect.right(), p.y)],
                Stroke::new(1.0, Color32::from_rgb(31, 44, 55)),
            );
        }
        let pointer = response.hover_pos();
        let mut hits = vec![];
        // Lines first so points are visible above them, with exact polyline hit testing.
        for o in network
            .objects
            .values()
            .filter(|o| matches!(o.geometry, Geometry::Polyline(_)))
        {
            let geometry = self
                .drag
                .as_ref()
                .filter(|d| d.id == o.id)
                .map_or(&o.geometry, |d| &d.preview);
            let points: Vec<_> = geometry
                .coordinates()
                .iter()
                .map(|p| camera.screen(*p, rect))
                .collect();
            let color = if Some(o.id) == selected {
                Color32::YELLOW
            } else if !o.in_service {
                Color32::GRAY
            } else {
                let low_voltage=o.buses().first().and_then(|id|network.objects.get(id)).is_some_and(|bus|matches!(bus.equipment,Equipment::Bus { voltage_v } if voltage_v<1000.0));
                if low_voltage {
                    Color32::from_rgb(90, 190, 235)
                } else {
                    Color32::from_rgb(245, 160, 85)
                }
            };
            let color = if Some(o.id) != selected && results.is_some_and(|r| r.overloaded(o.id)) {
                Color32::LIGHT_RED
            } else {
                color
            };
            if selected == Some(o.id) && tools.mode == MapMode::Vertex {
                for p in &points {
                    painter.circle_filled(*p, 5.0, Color32::YELLOW);
                }
            }
            for segment in points.windows(2) {
                painter.line_segment(
                    [segment[0], segment[1]],
                    Stroke::new(if Some(o.id) == selected { 4.0 } else { 2.5 }, color),
                );
            }
            if let Some(p) = pointer
                && points
                    .windows(2)
                    .any(|s| segment_distance(p, s[0], s[1]) < 7.0)
            {
                hits.push((1, o.id));
            }
        }
        for o in network
            .objects
            .values()
            .filter(|o| matches!(o.geometry, Geometry::Point(_)))
        {
            let geometry = self
                .drag
                .as_ref()
                .filter(|d| d.id == o.id)
                .map_or(&o.geometry, |d| &d.preview);
            let Geometry::Point(point) = geometry else {
                continue;
            };
            let p = camera.screen(*point, rect);
            if !rect.expand(20.0).contains(p) {
                continue;
            }
            let color = if Some(o.id) == selected {
                Color32::YELLOW
            } else if !o.in_service {
                Color32::GRAY
            } else {
                Color32::from_rgb(95, 220, 190)
            };
            let color = if Some(o.id) != selected
                && results.is_some_and(|r| r.outside(o.id) || r.overloaded(o.id))
            {
                Color32::LIGHT_RED
            } else {
                color
            };
            if matches!(
                o.equipment,
                Equipment::Transformer { .. } | Equipment::Source { .. }
            ) {
                painter.rect_filled(Rect::from_center_size(p, Vec2::splat(13.0)), 2.0, color);
            } else {
                painter.circle_filled(p, if Some(o.id) == selected { 7.0 } else { 4.0 }, color);
            }
            if Some(o.id) == selected {
                painter.circle_stroke(p, 12.0, Stroke::new(2.0, Color32::YELLOW));
                painter.text(
                    p + Vec2::new(10.0, -14.0),
                    Align2::LEFT_BOTTOM,
                    &o.name,
                    FontId::proportional(12.0),
                    Color32::YELLOW,
                );
            }
            if pointer.is_some_and(|cursor| cursor.distance(p) < 12.0) {
                hits.push((0, o.id));
            }
        }
        hits.sort();
        if !hits.is_empty() {
            let label = hits
                .iter()
                .map(|(_, id)| {
                    let name = &network.objects[id].name;
                    results
                        .and_then(|r| r.value(*id))
                        .map(|value| format!("{name} · {value}"))
                        .unwrap_or_else(|| name.clone())
                })
                .collect::<Vec<_>>()
                .join("\n");
            response.clone().on_hover_text(label);
        }
        if tools.mode == MapMode::Trace {
            let mut preview: Vec<_> = tools
                .draft
                .iter()
                .map(|p| camera.screen(*p, rect))
                .collect();
            if let Some(cursor) = pointer {
                preview.push(cursor);
            }
            for segment in preview.windows(2) {
                painter.line_segment([segment[0], segment[1]], Stroke::new(2.0, Color32::YELLOW));
            }
            for p in tools.draft {
                painter.circle_filled(camera.screen(*p, rect), 4.0, Color32::YELLOW);
            }
        }
        if response.drag_stopped_by(egui::PointerButton::Primary)
            && let Some(drag) = self.drag.take()
            && drag.preview != drag.before
        {
            event = Some(MapEvent::SetGeometry {
                id: drag.id,
                geometry: drag.preview,
            });
        }
        if response.clicked()
            && matches!(tools.mode, MapMode::Place | MapMode::Trace)
            && let Some(p) = pointer
        {
            event = Some(MapEvent::Click(camera.world(p, rect)));
        }
        let scale_m = grid_step(80.0 / camera.pixels_per_m);
        let a = rect.left_bottom() + Vec2::new(20.0, -24.0);
        let b = a + Vec2::new((scale_m * camera.pixels_per_m) as f32, 0.0);
        painter.line_segment([a, b], Stroke::new(2.0, Color32::LIGHT_GRAY));
        painter.text(
            a + Vec2::new(0.0, -4.0),
            Align2::LEFT_BOTTOM,
            format!("{scale_m:.1} m"),
            FontId::proportional(11.0),
            Color32::LIGHT_GRAY,
        );
        painter.text(
            rect.right_top() + Vec2::new(-12.0, 10.0),
            Align2::RIGHT_TOP,
            "N ↑",
            FontId::proportional(13.0),
            Color32::LIGHT_GRAY,
        );
        painter.text(
            rect.right_bottom() + Vec2::new(-12.0, -10.0),
            Align2::RIGHT_BOTTOM,
            format!("{unlocated} objets non localisés · sans fond de carte"),
            FontId::proportional(11.0),
            Color32::LIGHT_GRAY,
        );
        if response.clicked()
            && matches!(
                tools.mode,
                MapMode::Select | MapMode::Move | MapMode::Vertex
            )
            && !hits.is_empty()
        {
            let index = selected
                .and_then(|id| hits.iter().position(|(_, candidate)| *candidate == id))
                .map_or(0, |i| (i + 1) % hits.len());
            return Some(MapEvent::Select(hits[index].1));
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_keeps_metric_aspect_ratio_north_up_and_precision_at_large_coordinates() {
        let camera = Camera {
            center: Coordinate(700000.0, 6600000.0),
            pixels_per_m: 0.25,
        };
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 400.0));
        let p = Coordinate(700200.0, 6600100.0);
        let screen = camera.screen(p, rect);
        assert_eq!(screen, Pos2::new(450.0, 175.0));
        assert_eq!(camera.world(screen, rect), p);
    }
    #[test]
    fn polyline_selection_uses_segments_not_bounding_box() {
        let a = Pos2::new(0.0, 0.0);
        let b = Pos2::new(100.0, 100.0);
        assert!(segment_distance(Pos2::new(50.0, 50.0), a, b) < 0.001);
        assert!(segment_distance(Pos2::new(0.0, 100.0), a, b) > 70.0);
    }
    fn frame(
        map: &mut MapView,
        ctx: &egui::Context,
        n: &Network,
        id: Id,
        mode: MapMode,
        events: Vec<egui::Event>,
    ) -> Option<MapEvent> {
        let mut event = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                events,
                ..Default::default()
            },
            |ui| {
                event = map.show(ui, n, Some(id), None, MapTools { mode, draft: &[] });
            },
        );
        output.textures_delta.clear();
        event
    }
    fn pointer(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn point_drag_emits_one_geometry_edit_only_on_release() {
        let n = calcelec::model::rural_example();
        let id = n
            .objects
            .values()
            .find(|o| o.name == "Poste source — HTA")
            .unwrap()
            .id;
        let mut map = MapView {
            camera: Some(Camera {
                center: Coordinate(0.0, 0.0),
                pixels_per_m: 1.0,
            }),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        frame(&mut map, &ctx, &n, id, MapMode::Move, vec![]);
        let start = map.last_rect.unwrap().center();
        let end = start + Vec2::new(80.0, 20.0);
        assert!(
            frame(
                &mut map,
                &ctx,
                &n,
                id,
                MapMode::Move,
                vec![egui::Event::PointerMoved(start), pointer(start, true)]
            )
            .is_none()
        );
        assert!(
            frame(
                &mut map,
                &ctx,
                &n,
                id,
                MapMode::Move,
                vec![egui::Event::PointerMoved(end)]
            )
            .is_none()
        );
        let event = frame(
            &mut map,
            &ctx,
            &n,
            id,
            MapMode::Move,
            vec![pointer(end, false)],
        )
        .unwrap();
        let MapEvent::SetGeometry {
            id: moved,
            geometry: Geometry::Point(p),
        } = event
        else {
            panic!("unexpected event {event:?}");
        };
        assert_eq!(moved, id);
        assert!((p.0 - 80.0).abs() < 1e-5);
        assert!((p.1 + 20.0).abs() < 1e-5);
        assert_eq!(
            n.objects[&id].geometry,
            Geometry::Point(Coordinate(0.0, 0.0))
        );
    }
    #[test]
    fn vertex_drag_moves_only_the_chosen_vertex() {
        let mut n = calcelec::model::rural_example();
        let line = n
            .objects
            .values_mut()
            .find(|o| matches!(o.equipment, Equipment::Line { .. }))
            .unwrap();
        let id = line.id;
        line.geometry = Geometry::Polyline(vec![Coordinate(0.0, 0.0), Coordinate(100.0, 0.0)]);
        let mut map = MapView {
            camera: Some(Camera {
                center: Coordinate(0.0, 0.0),
                pixels_per_m: 1.0,
            }),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        frame(&mut map, &ctx, &n, id, MapMode::Vertex, vec![]);
        let start = map.last_rect.unwrap().center();
        let end = start + Vec2::new(40.0, 16.0);
        frame(
            &mut map,
            &ctx,
            &n,
            id,
            MapMode::Vertex,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
        );
        frame(
            &mut map,
            &ctx,
            &n,
            id,
            MapMode::Vertex,
            vec![egui::Event::PointerMoved(end)],
        );
        let event = frame(
            &mut map,
            &ctx,
            &n,
            id,
            MapMode::Vertex,
            vec![pointer(end, false)],
        )
        .unwrap();
        let MapEvent::SetGeometry {
            geometry: Geometry::Polyline(points),
            ..
        } = event
        else {
            panic!("unexpected event {event:?}");
        };
        assert_eq!(
            points,
            vec![Coordinate(40.0, -16.0), Coordinate(100.0, 0.0)]
        );
    }
    #[test]
    fn cancelling_a_drag_discards_preview_without_emitting_a_change() {
        let n = calcelec::model::rural_example();
        let id = n
            .objects
            .values()
            .find(|o| o.name == "Poste source — HTA")
            .unwrap()
            .id;
        let mut map = MapView {
            camera: Some(Camera {
                center: Coordinate(0.0, 0.0),
                pixels_per_m: 1.0,
            }),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        frame(&mut map, &ctx, &n, id, MapMode::Move, vec![]);
        let start = map.last_rect.unwrap().center();
        let end = start + Vec2::new(80.0, 20.0);
        frame(
            &mut map,
            &ctx,
            &n,
            id,
            MapMode::Move,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
        );
        frame(
            &mut map,
            &ctx,
            &n,
            id,
            MapMode::Move,
            vec![egui::Event::PointerMoved(end)],
        );
        assert!(map.drag.is_some());
        map.cancel_drag();
        assert!(
            frame(
                &mut map,
                &ctx,
                &n,
                id,
                MapMode::Move,
                vec![pointer(end, false)]
            )
            .is_none()
        );
    }
}
