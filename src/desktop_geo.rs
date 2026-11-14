use calcelec::{
    geometry::{Coordinate, Geometry},
    model::{Equipment, Id, Network},
};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

#[derive(Default)]
pub struct MapView {
    camera: Option<Camera>,
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
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        network: &Network,
        selected: Option<Id>,
    ) -> Option<Id> {
        let mut fit = false;
        ui.horizontal(|ui| {
            fit = ui.button("Cadrer le réseau").clicked();
            ui.label(network.coordinate_reference.label());
        });
        ui.small("Molette : zoom · Glisser : déplacement · Clic : sélection · Clics répétés : objets superposés");
        let height = (ui.available_height() * 0.55).clamp(220.0, 450.0);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), height),
            Sense::click_and_drag(),
        );
        if fit || self.camera.is_none() {
            self.camera = Camera::fit(network, rect);
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(19, 29, 38));
        let unlocated = network
            .objects
            .values()
            .filter(|o| matches!(o.geometry, Geometry::Unlocated))
            .count();
        let Some(mut camera) = self.camera else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Aucun objet localisé — renseignez les géométries dans les propriétés.",
                FontId::proportional(13.0),
                Color32::LIGHT_GRAY,
            );
            return None;
        };
        let drag = response.drag_delta();
        camera.center.0 -= drag.x as f64 / camera.pixels_per_m;
        camera.center.1 += drag.y as f64 / camera.pixels_per_m;
        if response.hovered()
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
            let points: Vec<_> = o
                .geometry
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
            let Geometry::Point(point) = o.geometry else {
                continue;
            };
            let p = camera.screen(point, rect);
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
                .map(|(_, id)| network.objects[id].name.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            response.clone().on_hover_text(label);
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
        if response.clicked() && !hits.is_empty() {
            let index = selected
                .and_then(|id| hits.iter().position(|(_, candidate)| *candidate == id))
                .map_or(0, |i| (i + 1) % hits.len());
            return Some(hits[index].1);
        }
        None
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
}
