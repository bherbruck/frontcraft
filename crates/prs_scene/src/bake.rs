//! Light worked out once when the level is built and multiplied into vertex colours: how much of the
//! outside a surface in a room can see, how close it is to a corner, how near a wall's foot it is.
//! A flat-shaded box has nowhere to put a gradient, so surfaces that want one are cut into tiles here.

use bevy::prelude::*;
use prs_atmosphere::Bake;
use prs_level::{Level, Room};

/// How high a doorway stands, for how much sky it lets in. Only doors' bottoms are recorded.
const DOOR_HEIGHT: f32 = 2.1;
/// How far an opening's centre may be from a room's wall and still be an opening in it.
const OPENING_REACH: f32 = 0.7;
/// Keeps the light finite right up against an opening, in square metres.
const CLOSE_UP: f32 = 0.5;

/// Shortest stretch worth a tile of its own, in metres.
const MIN_TILE: f32 = 1.4;

#[derive(Clone)]
pub(crate) struct Opening {
    at: Vec3,
    outward: Vec3,
    area: f32,
}

/// Side of a square of the grid that finds the rooms near a point, in metres.
const ROOM_CELL: f32 = 16.0;

/// What lighting the boxes of a level needs worked out once: each room's openings to outside, and a grid
/// of which rooms are near each place, so a box looks at a handful of rooms rather than all of them.
#[derive(Default)]
pub(crate) struct RoomCache {
    openings: Vec<Vec<Opening>>,
    cells: std::collections::HashMap<(i32, i32), Vec<u32>>,
}

/// The rooms of a level with the openings to outside that light them.
pub(crate) struct RoomLight<'a> {
    rooms: &'a [Room],
    cache: std::borrow::Cow<'a, RoomCache>,
    bake: Bake,
}

impl Clone for RoomCache {
    fn clone(&self) -> Self {
        Self { openings: self.openings.clone(), cells: self.cells.clone() }
    }
}

fn room_cell(x: f32, z: f32) -> (i32, i32) {
    ((x / ROOM_CELL).floor() as i32, (z / ROOM_CELL).floor() as i32)
}

impl<'a> RoomLight<'a> {
    #[cfg(test)]
    pub fn new(level: &'a Level, bake: Bake) -> Self {
        Self { rooms: &level.rooms, cache: std::borrow::Cow::Owned(Self::cache(level)), bake }
    }

    /// With what [`RoomLight::cache`] worked out before, which is the slow part, so that pieces of a
    /// level can be lit one at a time as they are needed.
    pub fn with_cache(level: &'a Level, bake: Bake, cache: &'a RoomCache) -> Self {
        Self { rooms: &level.rooms, cache: std::borrow::Cow::Borrowed(cache), bake }
    }

    pub fn cache(level: &Level) -> RoomCache {
        let mut cells: std::collections::HashMap<(i32, i32), Vec<u32>> = std::collections::HashMap::new();
        for (i, room) in level.rooms.iter().enumerate() {
            let reach = Vec2::new(room.half.x, room.half.z).length();
            let (lo, hi) = (room_cell(room.center.x - reach, room.center.z - reach), room_cell(room.center.x + reach, room.center.z + reach));
            for x in lo.0..=hi.0 {
                for z in lo.1..=hi.1 {
                    cells.entry((x, z)).or_default().push(i as u32);
                }
            }
        }
        RoomCache { openings: Self::openings(level), cells }
    }

    /// Rooms that may come within `reach` of `p` across the ground, in the order of the level's list.
    pub fn rooms_near(&self, p: Vec3, reach: f32) -> Vec<usize> {
        let (lo, hi) = (room_cell(p.x - reach, p.z - reach), room_cell(p.x + reach, p.z + reach));
        let mut found: Vec<usize> = Vec::new();
        for x in lo.0..=hi.0 {
            for z in lo.1..=hi.1 {
                found.extend(self.cache.cells.get(&(x, z)).into_iter().flatten().map(|&i| i as usize));
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    }

    /// The openings to outside in each room's outer walls.
    fn openings(level: &Level) -> Vec<Vec<Opening>> {
        let doors = level.doorways.iter().map(|d| Opening { at: d.at + Vec3::Y * DOOR_HEIGHT * 0.5, outward: d.outward, area: d.width * DOOR_HEIGHT });
        let windows = level.windows.iter().map(|w| Opening { at: w.at, outward: w.outward, area: w.width * w.height });
        let all: Vec<Opening> = doors.chain(windows).collect();
        level
            .rooms
            .iter()
            .map(|room| {
                all.iter()
                    .filter(|o| {
                        // In this room's outer wall, not a door between two parts of it.
                        let local = room.rotation.conjugate() * (o.at - room.center);
                        let outward = room.rotation.conjugate() * o.outward;
                        let past = local.abs() - room.half;
                        let in_wall = past.x.max(past.z).abs() < OPENING_REACH && past.x.min(past.z) < OPENING_REACH;
                        in_wall && past.y < 0.0 && local.dot(outward) > 0.0
                    })
                    .map(|o| Opening { at: o.at, outward: o.outward, area: o.area })
                    .collect()
            })
            .collect()
    }

    /// The same bake with no rooms in it, for things that stand outside whatever is nearby.
    pub fn outdoors(other: &RoomLight<'a>) -> Self {
        Self { rooms: &[], cache: std::borrow::Cow::Owned(RoomCache::default()), bake: other.bake }
    }

    pub fn bake(&self) -> &Bake {
        &self.bake
    }

    pub fn rooms(&self) -> &'a [Room] {
        self.rooms
    }

    /// The room this bit of surface faces into, if any.
    pub fn room_facing(&self, at: Vec3, normal: Vec3) -> Option<usize> {
        let p = at + normal * 0.1;
        self.rooms_near(p, 0.0).into_iter().find(|&i| self.rooms[i].contains(p))
    }

    /// What to multiply a surface's colour by at this point, in this room.
    pub fn at(&self, room: usize, p: Vec3, normal: Vec3) -> Vec3 {
        let mut seen = 0.0;
        for opening in &self.cache.openings[room] {
            let to = opening.at - p;
            let d2 = to.length_squared().max(1e-4);
            let along = to / d2.sqrt();
            // A surface lit by an opening, each by how squarely it faces the other. The wall an
            // opening is in sees nothing of it, and a little bounces around to it anyway.
            let facing = (normal.dot(along).max(0.0) + 0.12) * along.dot(opening.outward).max(0.0);
            seen += opening.area * facing / (std::f32::consts::PI * (d2 + CLOSE_UP));
        }
        let lit = 1.0 - (-seen * self.bake.room_gain).exp();
        let light = self.bake.room_deep.lerp(self.bake.room_near, lit);
        light * self.corner(room, p, normal)
    }

    /// Darker towards the other surfaces of the room. The one this surface belongs to does not count.
    fn corner(&self, room: usize, p: Vec3, normal: Vec3) -> f32 {
        let room = &self.rooms[room];
        let local = room.rotation.conjugate() * (p - room.center);
        let own = (room.rotation.conjugate() * normal).abs();
        let depth = room.half - local.abs();
        [(depth.x, own.x), (depth.y, own.y), (depth.z, own.z)].into_iter().fold(1.0, |shade, (depth, own)| {
            if own > 0.5 {
                return shade;
            }
            let t = (depth / self.bake.corner_reach).clamp(0.0, 1.0);
            shade * (self.bake.corner_dark + (1.0 - self.bake.corner_dark) * t * (2.0 - t))
        })
    }
}

/// Where to cut an edge this long so a tile starts close to each end and none is over `cell` long.
pub(crate) fn room_cuts(length: f32, reach: f32, cell: f32) -> Vec<f32> {
    let middle = length - reach * 2.0;
    if middle < MIN_TILE {
        return vec![0.0, 1.0];
    }
    let tiles = (middle / cell).ceil().max(1.0) as usize;
    let mut cuts = vec![0.0];
    for i in 0..=tiles {
        cuts.push((reach + middle * i as f32 / tiles as f32) / length);
    }
    cuts.push(1.0);
    cuts
}

/// Where to cut a wall on its way up: once, near the floor. Whatever is above fades to the ceiling on its own.
pub(crate) fn wall_rise_cuts(edge: Vec3, reach: f32) -> Vec<f32> {
    let length = edge.length();
    if length < reach * 2.0 {
        return vec![0.0, 1.0];
    }
    let near_foot = reach / length;
    vec![0.0, if edge.y > 0.0 { near_foot } else { 1.0 - near_foot }, 1.0]
}

/// How much light a wall still gets this far above its foot.
pub(crate) fn foot_shade(bake: &Bake, above: f32) -> Vec3 {
    let t = (above / bake.foot_height).clamp(0.0, 1.0);
    bake.foot_dark.lerp(Vec3::ONE, t * (2.0 - t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> Level {
        let mut level = Level::flat(64.0);
        level.rooms.push(Room { center: Vec3::new(0.0, 1.5, 0.0), half: Vec3::new(5.0, 1.5, 4.0), rotation: Quat::IDENTITY });
        level.doorways.push(prs_level::Doorway { at: Vec3::new(5.1, 0.0, 0.0), outward: Vec3::X, width: 1.2 });
        level
    }

    #[test]
    fn a_room_is_brightest_by_its_door_and_never_black() {
        let level = room();
        let light = RoomLight::new(&level, Bake::default());
        let by_door = light.at(0, Vec3::new(3.5, 0.0, 0.0), Vec3::Y);
        let far_end = light.at(0, Vec3::new(-3.5, 0.0, 0.0), Vec3::Y);
        assert!(by_door.length() > far_end.length() * 1.5, "{by_door} {far_end}");
        assert!(far_end.min_element() > 0.1, "{far_end}");
    }

    #[test]
    fn corners_are_darker_than_the_middle_of_a_wall() {
        let level = room();
        let light = RoomLight::new(&level, Bake::default());
        let middle = light.at(0, Vec3::new(-5.0, 1.5, 0.0), Vec3::X);
        let corner = light.at(0, Vec3::new(-5.0, 0.0, 3.9), Vec3::X);
        assert!(corner.length() < middle.length() * 0.8, "{corner} {middle}");
    }

    #[test]
    fn a_door_inside_a_room_lets_in_no_daylight() {
        let mut level = room();
        level.doorways.clear();
        level.doorways.push(prs_level::Doorway { at: Vec3::new(0.0, 0.0, 0.0), outward: Vec3::X, width: 1.2 });
        let light = RoomLight::new(&level, Bake::default());
        assert!(light.cache.openings[0].is_empty());
    }

    #[test]
    fn cuts_stay_in_order_and_cover_the_edge() {
        for length in [0.5, 1.7, 3.1, 3.5, 12.4] {
            let cuts = room_cuts(length, 0.8, 2.6);
            assert_eq!((cuts[0], *cuts.last().unwrap()), (0.0, 1.0));
            assert!(cuts.windows(2).all(|w| w[0] < w[1] && (w[1] - w[0]) * length <= 2.6 + 1e-3), "{cuts:?}");
        }
    }
}
