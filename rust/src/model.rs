//! The plumbing a program builds: tanks that hold a quantity, and pipes that
//! move it between them. This is the analog machine itself; the interpreter
//! (`interpreter.rs`) wires it up and runs it.
//!
//! A pipe only ever moves water from one tank to another, so the total in
//! the tanks changes only through pipes that touch the outside world. That is
//! the property MONIAC's water made visible.

use serde::{Deserialize, Serialize};

/// The pseudo-tank a pipe names to draw from, or drain into, the world
/// outside the model.
pub const OUTSIDE: &str = "OUTSIDE";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tank {
    pub name: String,
    pub level: f64,
    /// The level `TANK` declared, which `REWIND` restores.
    pub initial: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pipe {
    pub name: String,
    /// The block that computes the flow rate, in quantity per unit time.
    pub rate: Vec<crate::value::Value>,
    /// Tank index, or `None` for the outside world.
    pub from: Option<usize>,
    pub to: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub tanks: Vec<Tank>,
    pub pipes: Vec<Pipe>,
    pub time: f64,
}

impl Model {
    pub fn tank(&self, name: &str) -> Option<usize> {
        self.tanks.iter().position(|t| t.name == name)
    }

    pub fn pipe(&self, name: &str) -> Option<usize> {
        self.pipes.iter().position(|p| p.name == name)
    }

    pub fn levels(&self) -> Vec<f64> {
        self.tanks.iter().map(|t| t.level).collect()
    }

    pub fn set_levels(&mut self, levels: &[f64]) {
        for (tank, level) in self.tanks.iter_mut().zip(levels) {
            tank.level = *level;
        }
    }

    pub fn end_name(&self, end: Option<usize>) -> &str {
        end.map_or(OUTSIDE, |i| &self.tanks[i].name)
    }
}

const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
pub const CHART_WIDTH: usize = 40;

/// One row of eighth-block bars, scaled between the series' own extremes.
pub fn sparkline(series: &[f64]) -> String {
    if series.is_empty() {
        return String::new();
    }
    let columns = series.len().min(CHART_WIDTH);
    let (lo, hi) = series
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        });
    (0..columns)
        .map(|c| {
            let x = series[c * (series.len() - 1) / (columns - 1).max(1)];
            let span = hi - lo;
            if span <= f64::EPSILON * hi.abs().max(1.0) {
                return BARS[3];
            }
            let index = ((x - lo) / span * (BARS.len() - 1) as f64).round() as usize;
            BARS[index.min(BARS.len() - 1)]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparkline_spans_the_series_extremes() {
        let line = sparkline(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        assert_eq!(line, "▁▂▃▄▅▆▇█");
        assert_eq!(sparkline(&[2.0, 2.0, 2.0]), "▄▄▄");
    }

    #[test]
    fn sparkline_is_never_wider_than_the_chart() {
        let series: Vec<f64> = (0..1000).map(f64::from).collect();
        let line = sparkline(&series);
        assert_eq!(line.chars().count(), CHART_WIDTH);
        assert!(line.starts_with('▁') && line.ends_with('█'));
    }
}
