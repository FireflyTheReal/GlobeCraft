//! The white symbols drawn on the badges, in a symbol space of (-1, -1) to (1, 1).
use eframe::egui::{Color32, Pos2, Shape, Stroke, Vec2};
use std::f32::consts::PI;

/// The symbol inside the badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app::live_layers) enum Symbol {
    Restaurant,
    Cafe,
    Bar,
    Shop,
    Bed,
    Museum,
    Star,
    Theatre,
    Worship,
    Plus,
    Pill,
    School,
    Book,
    Bank,
    Fuel,
    Parking,
    Bus,
    Train,
    Toilets,
    Viewpoint,
    Tent,
    Tree,
    Ball,
    Water,
    Building,
    Mountain,
    Boat,
    Bicycle,
    Dot,
}

/// Draws in symbol space: (-1, -1) top left to (1, 1) bottom right.
pub(super) struct Pen {
    pub at: Pos2,
    pub size: f32,
    pub stroke: Stroke,
    pub fill: Color32,
}

impl Pen {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.at + Vec2::new(x, y) * self.size
    }
    fn line(&self, out: &mut Vec<Shape>, points: &[(f32, f32)]) {
        out.push(Shape::line(
            points.iter().map(|&(x, y)| self.p(x, y)).collect(),
            self.stroke,
        ));
    }
    fn closed(&self, out: &mut Vec<Shape>, points: &[(f32, f32)]) {
        out.push(Shape::closed_line(
            points.iter().map(|&(x, y)| self.p(x, y)).collect(),
            self.stroke,
        ));
    }
    fn solid(&self, out: &mut Vec<Shape>, points: &[(f32, f32)]) {
        out.push(Shape::convex_polygon(
            points.iter().map(|&(x, y)| self.p(x, y)).collect(),
            self.fill,
            Stroke::NONE,
        ));
    }
    fn dot(&self, out: &mut Vec<Shape>, x: f32, y: f32, radius: f32) {
        out.push(Shape::circle_filled(
            self.p(x, y),
            radius * self.size,
            self.fill,
        ));
    }
    fn ring(&self, out: &mut Vec<Shape>, x: f32, y: f32, radius: f32) {
        out.push(Shape::circle_stroke(
            self.p(x, y),
            radius * self.size,
            self.stroke,
        ));
    }
    fn arc(&self, out: &mut Vec<Shape>, centre: (f32, f32), radius: f32, from: f32, to: f32) {
        const STEPS: usize = 8;
        let points: Vec<(f32, f32)> = (0..=STEPS)
            .map(|i| {
                let a = from + (to - from) * i as f32 / STEPS as f32;
                (centre.0 + radius * a.cos(), centre.1 + radius * a.sin())
            })
            .collect();
        self.line(out, &points);
    }

    pub(super) fn symbol(&self, out: &mut Vec<Shape>, symbol: Symbol) {
        match symbol {
            Symbol::Restaurant => {
                // Fork (three tines) and knife.
                self.line(out, &[(-0.45, -0.9), (-0.45, 0.9)]);
                self.line(
                    out,
                    &[(-0.8, -0.9), (-0.8, -0.25), (-0.1, -0.25), (-0.1, -0.9)],
                );
                self.solid(out, &[(0.45, -0.9), (0.8, -0.5), (0.8, 0.1), (0.45, 0.1)]);
                self.line(out, &[(0.55, 0.0), (0.55, 0.9)]);
            }
            Symbol::Cafe => {
                self.solid(
                    out,
                    &[(-0.8, -0.35), (0.45, -0.35), (0.3, 0.7), (-0.65, 0.7)],
                );
                self.arc(out, (0.5, 0.05), 0.3, -PI / 2.0, PI / 2.0);
                self.line(out, &[(-0.9, 0.9), (0.6, 0.9)]);
            }
            Symbol::Bar => {
                self.solid(out, &[(-0.8, -0.8), (0.8, -0.8), (0.0, 0.15)]);
                self.line(out, &[(0.0, 0.1), (0.0, 0.85)]);
                self.line(out, &[(-0.45, 0.85), (0.45, 0.85)]);
            }
            Symbol::Shop => {
                self.solid(
                    out,
                    &[(-0.75, -0.3), (0.75, -0.3), (0.85, 0.9), (-0.85, 0.9)],
                );
                self.arc(out, (0.0, -0.3), 0.4, PI, 2.0 * PI);
            }
            Symbol::Bed => {
                self.line(out, &[(-0.9, -0.6), (-0.9, 0.75)]);
                self.line(out, &[(-0.9, 0.35), (0.9, 0.35), (0.9, 0.75)]);
                self.dot(out, -0.5, -0.05, 0.25);
                self.solid(out, &[(-0.2, -0.3), (0.85, -0.3), (0.85, 0.2), (-0.2, 0.2)]);
            }
            Symbol::Museum => {
                self.solid(out, &[(-0.95, -0.4), (0.0, -0.95), (0.95, -0.4)]);
                for x in [-0.6, 0.0, 0.6] {
                    self.line(out, &[(x, -0.3), (x, 0.6)]);
                }
                self.line(out, &[(-0.95, 0.8), (0.95, 0.8)]);
            }
            Symbol::Star => {
                let points: Vec<(f32, f32)> = (0..10)
                    .map(|i| {
                        let a = -PI / 2.0 + i as f32 * PI / 5.0;
                        let r = if i % 2 == 0 { 1.0 } else { 0.42 };
                        (r * a.cos(), r * a.sin() + 0.08)
                    })
                    .collect();
                // A star is not convex: fill the centre and draw the outline.
                self.closed(out, &points);
                self.dot(out, 0.0, 0.08, 0.38);
            }
            Symbol::Theatre => {
                // A mask: face, eyes and a smile.
                self.closed(
                    out,
                    &[
                        (-0.8, -0.8),
                        (0.8, -0.8),
                        (0.75, 0.1),
                        (0.0, 0.9),
                        (-0.75, 0.1),
                    ],
                );
                self.dot(out, -0.35, -0.3, 0.16);
                self.dot(out, 0.35, -0.3, 0.16);
                self.arc(out, (0.0, 0.05), 0.35, 0.2, PI - 0.2);
            }
            Symbol::Worship => {
                // A building with a spire.
                self.line(out, &[(0.0, -1.0), (0.0, -0.55)]);
                self.line(out, &[(-0.2, -0.82), (0.2, -0.82)]);
                self.solid(out, &[(-0.45, -0.1), (0.0, -0.5), (0.45, -0.1)]);
                self.solid(out, &[(-0.7, -0.1), (0.7, -0.1), (0.7, 0.9), (-0.7, 0.9)]);
            }
            Symbol::Plus => {
                self.solid(
                    out,
                    &[(-0.25, -0.85), (0.25, -0.85), (0.25, 0.85), (-0.25, 0.85)],
                );
                self.solid(
                    out,
                    &[(-0.85, -0.25), (0.85, -0.25), (0.85, 0.25), (-0.85, 0.25)],
                );
            }
            Symbol::Pill => {
                // A capsule, half filled, at an angle.
                let along = Vec2::angled(-PI / 4.0);
                let side = along.rot90() * 0.32;
                let (a, b) = (along * -0.6, along * 0.6);
                let corners = |c: Vec2| [(c + side), (c - side)];
                let [a1, a2] = corners(a);
                let [m1, m2] = corners(Vec2::ZERO);
                self.solid(
                    out,
                    &[(a1.x, a1.y), (m1.x, m1.y), (m2.x, m2.y), (a2.x, a2.y)],
                );
                self.dot(out, a.x, a.y, 0.32);
                let [b1, b2] = corners(b);
                self.closed(
                    out,
                    &[(m1.x, m1.y), (b1.x, b1.y), (b2.x, b2.y), (m2.x, m2.y)],
                );
                self.ring(out, b.x, b.y, 0.32);
            }
            Symbol::School => {
                // A mortarboard.
                self.solid(out, &[(-1.0, -0.2), (0.0, -0.7), (1.0, -0.2), (0.0, 0.3)]);
                self.line(
                    out,
                    &[
                        (-0.55, 0.0),
                        (-0.55, 0.55),
                        (0.0, 0.8),
                        (0.55, 0.55),
                        (0.55, 0.0),
                    ],
                );
                self.line(out, &[(0.85, -0.15), (0.85, 0.5)]);
            }
            Symbol::Book => {
                self.closed(out, &[(-0.9, -0.7), (0.0, -0.45), (0.0, 0.85), (-0.9, 0.6)]);
                self.closed(out, &[(0.9, -0.7), (0.0, -0.45), (0.0, 0.85), (0.9, 0.6)]);
            }
            Symbol::Bank => {
                self.ring(out, 0.0, 0.0, 0.85);
                self.line(out, &[(0.0, -0.6), (0.0, 0.6)]);
                self.arc(out, (0.0, -0.17), 0.25, PI * 0.1, PI * 1.5);
                self.arc(out, (0.0, 0.17), 0.25, -PI * 0.5, PI * 0.9);
            }
            Symbol::Fuel => {
                self.solid(
                    out,
                    &[(-0.75, -0.85), (0.25, -0.85), (0.25, 0.9), (-0.75, 0.9)],
                );
                self.line(
                    out,
                    &[
                        (0.25, -0.3),
                        (0.65, -0.3),
                        (0.65, 0.45),
                        (0.85, 0.45),
                        (0.85, -0.55),
                        (0.55, -0.85),
                    ],
                );
            }
            Symbol::Parking => {
                self.line(out, &[(-0.4, 0.9), (-0.4, -0.85), (0.1, -0.85)]);
                self.arc(out, (0.1, -0.4), 0.45, -PI / 2.0, PI / 2.0);
                self.line(out, &[(0.1, 0.05), (-0.4, 0.05)]);
            }
            Symbol::Bus => {
                self.closed(
                    out,
                    &[(-0.75, -0.85), (0.75, -0.85), (0.75, 0.55), (-0.75, 0.55)],
                );
                self.line(out, &[(-0.75, 0.0), (0.75, 0.0)]);
                self.dot(out, -0.45, 0.75, 0.2);
                self.dot(out, 0.45, 0.75, 0.2);
            }
            Symbol::Train => {
                self.solid(out, &[(-0.6, -0.9), (0.6, -0.9), (0.7, 0.5), (-0.7, 0.5)]);
                self.line(out, &[(-0.45, 0.6), (-0.8, 0.95)]);
                self.line(out, &[(0.45, 0.6), (0.8, 0.95)]);
            }
            Symbol::Toilets => {
                // Two figures.
                self.dot(out, -0.45, -0.65, 0.22);
                self.solid(
                    out,
                    &[(-0.7, -0.3), (-0.2, -0.3), (-0.25, 0.9), (-0.65, 0.9)],
                );
                self.dot(out, 0.45, -0.65, 0.22);
                self.solid(out, &[(0.45, -0.3), (0.8, 0.6), (0.1, 0.6)]);
                self.line(out, &[(0.45, 0.6), (0.45, 0.9)]);
            }
            Symbol::Viewpoint => {
                // Binoculars.
                self.ring(out, -0.45, 0.25, 0.38);
                self.ring(out, 0.45, 0.25, 0.38);
                self.line(out, &[(-0.7, -0.05), (-0.45, -0.75), (-0.15, -0.05)]);
                self.line(out, &[(0.15, -0.05), (0.45, -0.75), (0.7, -0.05)]);
            }
            Symbol::Tent => {
                self.closed(out, &[(0.0, -0.9), (0.95, 0.85), (-0.95, 0.85)]);
                self.solid(out, &[(0.0, 0.0), (0.35, 0.85), (-0.35, 0.85)]);
            }
            Symbol::Tree => {
                self.solid(out, &[(0.0, -1.0), (0.75, 0.35), (-0.75, 0.35)]);
                self.line(out, &[(0.0, 0.3), (0.0, 0.95)]);
            }
            Symbol::Ball => {
                self.ring(out, 0.0, 0.0, 0.85);
                self.arc(out, (-1.1, 0.0), 0.75, -0.75, 0.75);
                self.arc(out, (1.1, 0.0), 0.75, PI - 0.75, PI + 0.75);
            }
            Symbol::Water => {
                for y in [-0.35, 0.35] {
                    self.line(
                        out,
                        &[
                            (-0.9, y),
                            (-0.45, y - 0.25),
                            (0.0, y),
                            (0.45, y - 0.25),
                            (0.9, y),
                        ],
                    );
                }
            }
            Symbol::Building => {
                self.solid(out, &[(-0.7, -0.4), (0.0, -0.95), (0.7, -0.4)]);
                self.closed(
                    out,
                    &[(-0.6, -0.35), (0.6, -0.35), (0.6, 0.85), (-0.6, 0.85)],
                );
                self.line(out, &[(0.0, 0.85), (0.0, 0.3)]);
            }
            Symbol::Mountain => {
                // Two peaks, the higher one with snow.
                self.solid(out, &[(-1.0, 0.8), (-0.45, -0.1), (0.1, 0.8)]);
                self.solid(out, &[(-0.35, 0.8), (0.3, -0.85), (0.95, 0.8)]);
            }
            Symbol::Boat => {
                // A sail over a hull.
                self.solid(out, &[(0.05, -0.95), (0.05, 0.25), (0.75, 0.25)]);
                self.line(out, &[(-0.05, -0.95), (-0.05, 0.4)]);
                self.solid(out, &[(-0.95, 0.4), (0.95, 0.4), (0.6, 0.85), (-0.6, 0.85)]);
            }
            Symbol::Bicycle => {
                self.ring(out, -0.55, 0.4, 0.4);
                self.ring(out, 0.55, 0.4, 0.4);
                self.line(
                    out,
                    &[(-0.55, 0.4), (-0.15, -0.35), (0.35, -0.35), (0.55, 0.4)],
                );
                self.line(out, &[(-0.15, -0.35), (0.05, 0.4), (0.35, -0.35)]);
                self.line(out, &[(-0.3, -0.6), (0.0, -0.6)]);
            }
            Symbol::Dot => self.dot(out, 0.0, 0.0, 0.35),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{BADGE_RADIUS_POINTS, SYMBOL_SHARE};
    use super::*;

    #[test]
    fn every_symbol_stays_inside_its_badge() {
        use Symbol as S;
        let symbols = [
            S::Restaurant,
            S::Cafe,
            S::Bar,
            S::Shop,
            S::Bed,
            S::Museum,
            S::Star,
            S::Theatre,
            S::Worship,
            S::Plus,
            S::Pill,
            S::School,
            S::Book,
            S::Bank,
            S::Fuel,
            S::Parking,
            S::Bus,
            S::Train,
            S::Toilets,
            S::Viewpoint,
            S::Tent,
            S::Tree,
            S::Ball,
            S::Water,
            S::Building,
            S::Mountain,
            S::Boat,
            S::Bicycle,
            S::Dot,
        ];
        let at = Pos2::new(100.0, 100.0);
        let r = BADGE_RADIUS_POINTS;
        let pen = Pen {
            at,
            size: r * SYMBOL_SHARE,
            stroke: Stroke::new(1.0, Color32::WHITE),
            fill: Color32::WHITE,
        };
        for symbol in symbols {
            let mut shapes = Vec::new();
            pen.symbol(&mut shapes, symbol);
            assert!(!shapes.is_empty(), "{symbol:?}");
            let bounds = Shape::Vec(shapes).visual_bounding_rect();
            for corner in [bounds.min, bounds.max] {
                assert!(
                    corner.distance(at) < r * 1.05,
                    "{symbol:?} reaches {corner:?}"
                );
            }
        }
    }
}
