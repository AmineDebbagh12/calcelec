//! Geographic coordinates are model data, independent of screen positions and connectivity.
use crate::model::{Error, Result};
use serde::{Deserialize, Serialize};

/// Coordinates use x=easting, y=northing, in metres. No implicit reprojection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinateReference {
    #[default]
    LocalMeters,
    Lambert93,
}
impl CoordinateReference {
    pub fn label(self) -> &'static str {
        match self {
            Self::LocalMeters => "Repère local — mètres",
            Self::Lambert93 => "Lambert-93 — EPSG:2154 — mètres",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coordinate(pub f64, pub f64);
impl Coordinate {
    pub fn distance(self, other: Self) -> f64 {
        (self.0 - other.0).hypot(self.1 - other.1)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "coordinates",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Geometry {
    /// Legacy or newly created object awaiting survey/location. Never drawn at (0,0).
    #[default]
    Unlocated,
    Point(Coordinate),
    Polyline(Vec<Coordinate>),
}
impl Geometry {
    pub fn coordinates(&self) -> &[Coordinate] {
        match self {
            Self::Unlocated => &[],
            Self::Point(point) => std::slice::from_ref(point),
            Self::Polyline(points) => points,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self
            .coordinates()
            .iter()
            .any(|p| !p.0.is_finite() || !p.1.is_finite() || p.0.abs() > 1e9 || p.1.abs() > 1e9)
        {
            return Err(Error::Invalid(
                "coordonnées non finies ou hors domaine métrique supporté (±10⁹ m)".into(),
            ));
        }
        if let Self::Polyline(points) = self
            && (points.len() < 2 || points.windows(2).any(|w| w[0] == w[1]))
        {
            return Err(Error::Invalid(
                "une polyligne exige au moins deux sommets, sans sommets consécutifs identiques"
                    .into(),
            ));
        }
        Ok(())
    }
    /// Planar geometric length; deliberately does not modify electrical length_m.
    pub fn length_m(&self) -> Option<f64> {
        match self {
            Self::Polyline(points) => Some(points.windows(2).map(|w| w[0].distance(w[1])).sum()),
            _ => None,
        }
    }
}
