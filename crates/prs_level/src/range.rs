//! What the training range has beyond its lanes: a house for clearing rooms, boards that mark the lane
//! distances, places to practise resting the rifle, firing through windows and lying down, and a lane of
//! things to climb. Where each goes is data here, in one place.

use crate::archetypes::kill_house;
use crate::spec::{BuildStyle, Compound, Ground};
use crate::{Level, Paint, Window, smoothstep};
use glam::{Quat, Vec2, Vec3};
use std::f32::consts::FRAC_PI_2;

/// The kill house, turned so its side door and start line face the firing line, east of it.
pub(crate) const KILL_HOUSE_AT: Vec2 = Vec2::new(-70.0, -5.0);
const KILL_HOUSE_YAW: f32 = FRAC_PI_2;
/// The line to cross going in, this far out from the side door, and as wide as the doorway's approach.
const START_LINE_OUT: f32 = 4.0;
const START_LINE_HALF: Vec3 = Vec3::new(0.08, 0.015, 1.6);

/// Distances down the lanes that get a board each side, and how far out from the middle the boards stand.
const LANE_MARKS: [f32; 5] = [25.0, 50.0, 100.0, 200.0, 300.0];
const MARK_X: f32 = 25.0;
/// One stripe per this many metres up to a hundred, then one red board per hundred.
const MARK_STEP: f32 = 25.0;
const MARK_HUNDRED: f32 = 100.0;

/// Things to rest the rifle on along the firing line: (x, half size), standing on the ground at its z.
/// A bench at standing height, a box at kneeling height and a low bag for lying behind.
const FIRING_LINE_Z: f32 = -6.5;
/// Clear of the vehicles parked behind the firing line, which drive off straight down range.
const RESTS: [(f32, Vec3, Paint); 3] = [
    (17.5, Vec3::new(0.9, 0.56, 0.3), Paint::Wood),
    (20.5, Vec3::new(0.8, 0.38, 0.3), Paint::Wood),
    (24.0, Vec3::new(1.2, 0.15, 0.3), Paint::Sandbag),
];

/// A wall to fire through, beside the firing line and out of the parked vehicles' way: openings for standing, kneeling and lying down, as
/// (along the wall from its west end, width, sill, head).
const WINDOW_WALL_FROM: Vec2 = Vec2::new(-32.0, -5.0);
const WINDOW_WALL_LENGTH: f32 = 8.0;
const WINDOW_WALL_HEIGHT: f32 = 2.6;
const WINDOW_WALL_THICKNESS: f32 = 0.3;
const WINDOW_OPENINGS: [(f32, f32, f32, f32); 3] = [(1.5, 1.2, 1.15, 1.8), (4.0, 1.2, 0.7, 1.25), (6.5, 1.4, 0.12, 0.42)];

/// A lane of things to climb, one after another going away from the firing line: (z, half size of
/// what you go over, and whether it has a window to go through instead).
const CLIMB_LANE_X: f32 = 53.0;
const CLIMB_LANE: [(f32, Vec3, bool); 6] = [
    (0.0, Vec3::new(1.5, 0.3, 0.2), false),
    (-6.0, Vec3::new(1.5, 0.5, 0.15), false),
    (-12.0, Vec3::new(1.5, 0.72, 0.15), false),
    (-18.0, Vec3::new(1.5, 0.95, 0.8), false),
    (-25.0, Vec3::new(1.5, 1.3, 0.15), true),
    (-31.0, Vec3::new(1.5, 1.4, 0.15), false),
];
const CLIMB_WINDOW: (f32, f32, f32) = (1.0, 0.95, 2.0);

/// Ground levelled for the house and the climbing lane: (middle, half size, how far it eases back).
pub(crate) const FLATS: [(Vec2, Vec2, f32); 2] = [
    (KILL_HOUSE_AT, Vec2::new(14.0, 18.0), 14.0),
    (Vec2::new(CLIMB_LANE_X, -15.0), Vec2::new(5.0, 22.0), 12.0),
];

/// Kept clear of trees and rocks: (min, max) on the ground.
pub(crate) const CLEARINGS: [(Vec2, Vec2); 2] = [(Vec2::new(-105.0, -40.0), Vec2::new(-35.0, 30.0)), (Vec2::new(40.0, -55.0), Vec2::new(68.0, 15.0))];

/// The land with level patches let in where things are built.
pub(crate) fn flattened(p: Vec2, natural: impl Fn(Vec2) -> f32) -> f32 {
    let mut h = natural(p);
    for (at, half, fade) in FLATS {
        let outside = ((p - at).abs() - half).max(Vec2::ZERO).length();
        let weight = 1.0 - smoothstep(0.0, fade, outside);
        if weight > 0.0 {
            h += (natural(at) - h) * weight;
        }
    }
    h
}

impl Level {
    pub(crate) fn add_training_grounds(&mut self) {
        self.add_kill_house();
        self.add_lane_marks();
        self.add_rests();
        self.add_window_wall();
        self.add_climb_lane();
    }

    fn add_kill_house(&mut self) {
        let style = BuildStyle::default();
        let house = kill_house(&style, (0.0, 0.0), 0.0, Paint::Cement);
        let half = house.half + Vec2::splat(1.0);
        let compound = Compound {
            at: KILL_HOUSE_AT,
            yaw: KILL_HOUSE_YAW,
            half,
            lift: 0.0,
            ground: Ground::Yard,
            wall: None,
            buildings: vec![house],
            props: Vec::new(),
        };
        let ground = self.ground(KILL_HOUSE_AT.x, KILL_HOUSE_AT.y);
        crate::build::Mason { style: &style, level: self }.compound(&compound, ground);
        // The start line, out from the side door.
        let turn = Quat::from_rotation_y(KILL_HOUSE_YAW);
        let door = turn * Vec3::new(-8.0, 0.0, half.y - 1.0 + START_LINE_OUT);
        let at = Vec3::new(KILL_HOUSE_AT.x + door.x, ground + START_LINE_HALF.y, KILL_HOUSE_AT.y + door.z);
        self.add(at, START_LINE_HALF, turn * Quat::from_rotation_y(FRAC_PI_2), Paint::Limewash);
    }

    fn add_lane_marks(&mut self) {
        for distance in LANE_MARKS {
            let z = -distance;
            for x in [-MARK_X, MARK_X] {
                let g = self.ground(x, z);
                self.add(Vec3::new(x, g + 0.9, z), Vec3::new(0.06, 0.95, 0.06), Quat::IDENTITY, Paint::Wood);
                let (count, paint) = if distance < MARK_HUNDRED {
                    ((distance / MARK_STEP) as u32, Paint::Limewash)
                } else {
                    ((distance / MARK_HUNDRED) as u32, Paint::Brick)
                };
                for i in 0..count {
                    let y = g + 1.75 - i as f32 * 0.3;
                    self.add(Vec3::new(x, y, z + 0.07), Vec3::new(0.45, 0.11, 0.02), Quat::IDENTITY, paint);
                }
            }
        }
    }

    fn add_rests(&mut self) {
        for (x, half, paint) in RESTS {
            self.place(x, FIRING_LINE_Z, half, 0.0, paint);
        }
    }

    fn add_window_wall(&mut self) {
        let (x0, z) = (WINDOW_WALL_FROM.x, WINDOW_WALL_FROM.y);
        let g = (0..=4).map(|i| self.ground(x0 + WINDOW_WALL_LENGTH * i as f32 / 4.0, z)).fold(f32::MAX, f32::min);
        let foot = 0.4;
        let t = WINDOW_WALL_THICKNESS * 0.5;
        let piece = |level: &mut Level, from: f32, to: f32, bottom: f32, top: f32| {
            if to - from > 0.01 && top - bottom > 0.01 {
                let center = Vec3::new(x0 + (from + to) * 0.5, g + (bottom + top) * 0.5, z);
                level.add(center, Vec3::new((to - from) * 0.5, (top - bottom) * 0.5, t), Quat::IDENTITY, Paint::Concrete);
            }
        };
        let mut cursor = 0.0;
        for (along, width, sill, head) in WINDOW_OPENINGS {
            let (left, right) = (along - width * 0.5, along + width * 0.5);
            piece(self, cursor, left, -foot, WINDOW_WALL_HEIGHT);
            piece(self, left, right, -foot, sill);
            piece(self, left, right, head, WINDOW_WALL_HEIGHT);
            cursor = right;
            self.windows.push(Window {
                at: Vec3::new(x0 + along, g + (sill + head) * 0.5, z),
                outward: Vec3::Z,
                width,
                height: head - sill,
            });
        }
        piece(self, cursor, WINDOW_WALL_LENGTH, -foot, WINDOW_WALL_HEIGHT);
    }

    fn add_climb_lane(&mut self) {
        for (z, half, window) in CLIMB_LANE {
            let g = self.ground(CLIMB_LANE_X, z);
            if !window {
                self.add(Vec3::new(CLIMB_LANE_X, g + half.y - 0.05, z), half + Vec3::Y * 0.05, Quat::IDENTITY, Paint::Concrete);
                continue;
            }
            let (width, sill, head) = CLIMB_WINDOW;
            let top = half.y * 2.0;
            let side = (half.x * 2.0 - width) * 0.5;
            for dx in [-(width + side) * 0.5, (width + side) * 0.5] {
                self.add(Vec3::new(CLIMB_LANE_X + dx, g + top * 0.5 - 0.2, z), Vec3::new(side * 0.5, top * 0.5 + 0.2, half.z), Quat::IDENTITY, Paint::Concrete);
            }
            self.add(Vec3::new(CLIMB_LANE_X, g + (sill - 0.2) * 0.5, z), Vec3::new(width * 0.5, (sill + 0.2) * 0.5, half.z), Quat::IDENTITY, Paint::Concrete);
            self.add(Vec3::new(CLIMB_LANE_X, g + (head + top) * 0.5, z), Vec3::new(width * 0.5, (top - head) * 0.5, half.z), Quat::IDENTITY, Paint::Concrete);
            self.windows.push(Window { at: Vec3::new(CLIMB_LANE_X, g + (sill + head) * 0.5, z), outward: Vec3::Z, width, height: head - sill });
        }
    }
}
