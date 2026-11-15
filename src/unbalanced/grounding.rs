//! Exact neutral-ground compensation for a radial conductor tree, on a common earth reference.
use super::{Edge, check_cancel, unsupported};
use crate::{complex::C, load_flow::Result};
use std::sync::atomic::AtomicBool;

const MAX_GROUNDINGS: usize = 256;
pub(super) struct GroundSystem {
    pub nodes: Vec<usize>,
    pub impedances: Vec<C>,
    factor: Option<ComplexLu>,
}
impl GroundSystem {
    pub fn build(
        earth: Vec<(usize, C)>,
        edges: &[Edge],
        parent: &[Option<(usize, usize)>],
        cancel: &AtomicBool,
    ) -> Result<Self> {
        if earth.len() > MAX_GROUNDINGS {
            return Err(unsupported(format!(
                "ce moteur accepte au plus {MAX_GROUNDINGS} prises de terre aval par calcul"
            )));
        }
        let mut paths = vec![];
        for &(node, _) in &earth {
            let mut path = vec![];
            let mut cursor = node;
            while let Some((p, e)) = parent[cursor] {
                check_cancel(cancel)?;
                path.push(e);
                cursor = p;
            }
            path.reverse();
            paths.push(path);
        }
        let mut matrix = vec![vec![C::default(); earth.len()]; earth.len()];
        for (i, row) in matrix.iter_mut().enumerate() {
            check_cancel(cancel)?;
            for (j, value) in row.iter_mut().enumerate() {
                let common = paths[i]
                    .iter()
                    .zip(&paths[j])
                    .take_while(|(a, b)| a == b)
                    .fold(C::default(), |sum, (e, _)| sum + edges[*e].z[3][3]);
                *value = common + if i == j { earth[i].1 } else { C::default() };
            }
        }
        let factor = if earth.is_empty() {
            None
        } else {
            Some(ComplexLu::factor(matrix, cancel)?)
        };
        Ok(Self {
            nodes: earth.iter().map(|(i, _)| *i).collect(),
            impedances: earth.iter().map(|(_, z)| *z).collect(),
            factor,
        })
    }
    pub fn currents(&self, neutral_voltage: &[C], cancel: &AtomicBool) -> Result<Vec<C>> {
        match &self.factor {
            None => Ok(vec![]),
            Some(factor) => factor.solve(neutral_voltage, cancel),
        }
    }
}

/// Row-scaled LU with partial pivoting, reused at every load-flow iteration.
struct ComplexLu {
    matrix: Vec<Vec<C>>,
    permutation: Vec<usize>,
    scales: Vec<f64>,
}
fn singular() -> crate::load_flow::CalculationError {
    unsupported(
        "système des mises à la terre singulier ou trop mal conditionné : vérifier les connexions idéales redondantes, les impédances et les compensations",
    )
}
impl ComplexLu {
    fn factor(mut matrix: Vec<Vec<C>>, cancel: &AtomicBool) -> Result<Self> {
        let size = matrix.len();
        let mut scales = Vec::with_capacity(size);
        for row in &mut matrix {
            check_cancel(cancel)?;
            if row.len() != size {
                return Err(singular());
            }
            let scale = row.iter().map(|v| v.abs()).fold(0.0, f64::max);
            if !scale.is_finite() || scale <= 0.0 || !(1.0 / scale).is_finite() {
                return Err(singular());
            }
            for value in row {
                *value = *value * (1.0 / scale);
                if !value.finite() {
                    return Err(singular());
                }
            }
            scales.push(scale);
        }
        let mut permutation: Vec<_> = (0..size).collect();
        for k in 0..size {
            check_cancel(cancel)?;
            let pivot = (k..size)
                .max_by(|a, b| matrix[*a][k].abs().total_cmp(&matrix[*b][k].abs()))
                .unwrap();
            if matrix[pivot][k].abs() <= 1e-12 {
                return Err(singular());
            }
            matrix.swap(k, pivot);
            permutation.swap(k, pivot);
            let pivot_row = matrix[k].clone();
            for row in matrix.iter_mut().skip(k + 1) {
                let factor = row[k] / pivot_row[k];
                row[k] = factor;
                for (j, value) in row.iter_mut().enumerate().skip(k + 1) {
                    *value = *value - factor * pivot_row[j];
                    if !value.finite() {
                        return Err(singular());
                    }
                }
            }
        }
        Ok(Self {
            matrix,
            permutation,
            scales,
        })
    }
    fn solve(&self, rhs: &[C], cancel: &AtomicBool) -> Result<Vec<C>> {
        if rhs.len() != self.matrix.len() {
            return Err(singular());
        }
        let mut result = vec![C::default(); rhs.len()];
        for (i, &original) in self.permutation.iter().enumerate() {
            check_cancel(cancel)?;
            result[i] = rhs[original] * (1.0 / self.scales[original])
                - (0..i).fold(C::default(), |sum, j| sum + self.matrix[i][j] * result[j]);
        }
        for i in (0..result.len()).rev() {
            check_cancel(cancel)?;
            result[i] = (result[i]
                - (i + 1..result.len())
                    .fold(C::default(), |sum, j| sum + self.matrix[i][j] * result[j]))
                / self.matrix[i][i];
            if !result[i].finite() || !result[i].abs().is_finite() {
                return Err(singular());
            }
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scaled_complex_lu_pivots_and_recovers_a_known_solution() {
        let matrix = vec![
            vec![C::new(0.0, 0.0), C::new(1e8, 2e8)],
            vec![C::new(2e-6, 1e-6), C::new(3e-6, -1e-6)],
        ];
        let expected = [C::new(2.0, -1.0), C::new(-3.0, 0.5)];
        let rhs: Vec<_> = matrix
            .iter()
            .map(|row| {
                row.iter()
                    .zip(expected)
                    .fold(C::default(), |s, (a, x)| s + *a * x)
            })
            .collect();
        let cancel = AtomicBool::new(false);
        let factor = ComplexLu::factor(matrix, &cancel).unwrap();
        let actual = factor.solve(&rhs, &cancel).unwrap();
        for (a, b) in actual.iter().zip(expected) {
            assert!((*a - b).abs() < 1e-12);
        }
    }
    #[test]
    fn singular_grounding_connections_return_an_error() {
        let matrix = vec![vec![C::new(1.0, 0.0); 2]; 2];
        assert!(ComplexLu::factor(matrix, &AtomicBool::new(false)).is_err());
    }
}
