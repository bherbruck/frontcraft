//! How a sound gets from where it was made to your ear, worked out in seven bands from 125 Hz to 8 kHz
//! and heard as a volume and a cut to its top end.
//!
//! Out of doors nothing blocks a sound outright: it bends over a hill or a roof, and the further it has
//! to bend for its wavelength the more it loses (Maekawa's barrier, from the extra length of the path over
//! the top). Low notes wrap round almost for free and high ones are cut, so a slight crest takes off a few
//! decibels and some edge, and a firefight over the hill is still a firefight. Air takes the highs off
//! with distance too. Real muffling is for rooms: two places in different rooms hear each other through
//! their doors and windows, or else through the walls, which only the lows get through.

use bevy::prelude::*;
use prs_runtime::prs_sim::{CollisionWorld, Level};
use std::collections::HashMap;

pub const BANDS: usize = 7;

/// Everything about how sound gets around, for a mod to change.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Propagation {
    /// The middle of each band, in Hz.
    pub bands_hz: [f32; BANDS],
    /// How much the air takes off in each band, in dB a kilometre: ISO 9613-1 at 20 °C and 70% humidity.
    pub air_db_per_km: [f32; BANDS],
    /// The pitch whose loss is the volume, low so that what comes through is heard at its weight, in Hz.
    pub gain_hz: f32,
    /// Where the top end is cut: the pitch at which a sound has lost this much more than at `gain_hz`.
    pub cutoff_excess_db: f32,
    /// The cut is never lower than the first, and above the second there is nothing to cut.
    pub cutoff_range_hz: (f32, f32),
    pub speed_of_sound: f32,

    /// How much of Maekawa's loss to take. Under 1 because the loud things a game plays from afar are already
    /// far quieter against what is near than they are in life, so the full loss on top buries them.
    pub edge_strength: f32,
    /// Most any one edge can take off at 125 Hz, in dB, and most all of them together can, each this much
    /// more an octave up: in life scattering stops the loss growing, but the top end still goes first.
    pub edge_cap_db: f32,
    pub total_cap_db: f32,
    pub cap_db_per_octave: f32,
    /// How much each edge after the worst adds, as a share of what it would on its own.
    pub other_edges_share: f32,
    /// How far an edge has to stand above the straight line to count at all, and to count in full, in
    /// metres. In between it comes on smoothly, so nothing pops as someone crosses a crest.
    pub rise_counts_from: f32,
    pub rise_counts_fully: f32,
    /// Ground this close to either end is what the sound or the ear stands on, not something in the way.
    pub ends_clear: f32,

    /// Metres between looks at the height of the land along the way, and the most looks.
    pub land_step: f32,
    pub land_looks: usize,
    /// Metres between looks at the top of something built, as the way crosses it.
    pub top_step: f32,
    /// Most rays for one sound, after which what is left of the way counts as open.
    pub rays: usize,

    /// Loss through one wall at 125 Hz, how much more each octave up, and the most, in dB. This is the
    /// mass law, which is why only the lows get through.
    pub wall_db_at_125: f32,
    pub wall_db_per_octave: f32,
    pub wall_cap_db: f32,
    /// How many doors and windows a way between rooms may go through, and the most ways tried.
    pub openings: usize,
    pub ways: usize,
    pub door_height: f32,
    /// An opening smaller than this loses in proportion, in square metres.
    pub opening_full_area: f32,

    /// How long one answer serves, in seconds, and for how far each end can move in that time, in metres.
    pub remember_for: f32,
    pub source_cell: f32,
    pub ear_cell: f32,
    /// Most new answers a frame. Past that a sound takes its last answer, or is heard as in the open.
    pub per_frame: u32,
    /// For sounds mixed from a clear and a dull copy: a cut this low is all dull, and this high all clear.
    pub clarity_hz: (f32, f32),
}

impl Default for Propagation {
    fn default() -> Self {
        Self {
            bands_hz: [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0],
            air_db_per_km: [0.4, 1.0, 2.8, 5.0, 9.0, 23.0, 78.0],
            gain_hz: 125.0,
            cutoff_excess_db: 3.0,
            cutoff_range_hz: (180.0, 16_000.0),
            speed_of_sound: 343.0,
            edge_strength: 0.7,
            edge_cap_db: 14.0,
            total_cap_db: 18.0,
            cap_db_per_octave: 3.0,
            other_edges_share: 0.3,
            rise_counts_from: 0.1,
            rise_counts_fully: 1.0,
            ends_clear: 1.5,
            land_step: 4.0,
            land_looks: 64,
            top_step: 0.6,
            rays: 160,
            wall_db_at_125: 22.0,
            wall_db_per_octave: 5.0,
            wall_cap_db: 45.0,
            openings: 3,
            ways: 16,
            door_height: 2.1,
            opening_full_area: 1.8,
            remember_for: 0.5,
            source_cell: 3.0,
            ear_cell: 1.5,
            per_frame: 24,
            clarity_hz: (500.0, 4000.0),
        }
    }
}

/// What reaches the ear: the volume to play it at, and where to cut its top end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heard {
    pub gain: f32,
    pub cutoff_hz: f32,
}

impl Heard {
    pub const CLEAR: Heard = Heard { gain: 1.0, cutoff_hz: 20_000.0 };

    /// Whether there is any top end to take off, past what the ear would notice.
    pub fn cuts(&self, cfg: &Propagation) -> bool {
        self.cutoff_hz < cfg.cutoff_range_hz.1
    }

    /// How much of the top end is left, from 0 with it all gone at `dull_hz` to 1 with it all there at
    /// `clear_hz`, for sounds that cannot be filtered as they play and are mixed from a clear and a dull copy.
    pub fn clarity(&self, (dull_hz, clear_hz): (f32, f32)) -> f32 {
        ((self.cutoff_hz.ln() - dull_hz.ln()) / (clear_hz.ln() - dull_hz.ln()).max(1e-3)).clamp(0.0, 1.0)
    }
}

/// Loss in each band, in dB.
pub type Bands = [f32; BANDS];

/// The two things this needs to know about the world, so that a test can make up its own.
pub trait Ground {
    fn ray(&self, origin: Vec3, dir: Vec3, max_distance: f32) -> Option<f32>;
    fn land(&self, x: f32, z: f32) -> Option<f32>;
}

impl Ground for CollisionWorld {
    fn ray(&self, origin: Vec3, dir: Vec3, max_distance: f32) -> Option<f32> {
        self.raycast(origin, dir, max_distance).map(|hit| hit.distance)
    }

    fn land(&self, x: f32, z: f32) -> Option<f32> {
        self.land_height(x, z)
    }
}

/// One place sound has to bend over or through: how much longer the way is for it, and how far it stands
/// off the straight line. A whole stretch of ground between two points is one edge, its corners `minor`
/// ones that only add a little each.
#[derive(Clone, Copy, Debug)]
struct Edge {
    longer: f32,
    rise: f32,
    minor: bool,
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Propagation {
    /// Maekawa: 10 log10(3 + 20 N) for Fresnel number N = 2 δ / λ, coming on with how far the edge stands up.
    fn edge_loss(&self, edge: Edge) -> Bands {
        let counts = smoothstep((edge.rise - self.rise_counts_from) / (self.rise_counts_fully - self.rise_counts_from).max(1e-3));
        self.bands_hz.map(|hz| {
            let fresnel = 2.0 * edge.longer.max(0.0) * hz / self.speed_of_sound;
            (10.0 * (3.0 + 20.0 * fresnel).log10() * self.edge_strength).min(self.edge_cap_db + self.octaves(hz) * self.cap_db_per_octave) * counts
        })
    }

    /// The worst edge in each band, and a share of each of the others: several hills together are not
    /// the sum of each alone. Of the corners of one stretch, the worst is already in the stretch.
    fn edges_loss(&self, edges: &[Edge]) -> Bands {
        let each: Vec<(bool, Bands)> = edges.iter().map(|&edge| (edge.minor, self.edge_loss(edge))).collect();
        std::array::from_fn(|band| {
            let sorted = |minor: bool| {
                let mut losses: Vec<f32> = each.iter().filter(|e| e.0 == minor).map(|e| e.1[band]).collect();
                losses.sort_by(|a, b| b.total_cmp(a));
                losses
            };
            let (major, minor) = (sorted(false), sorted(true));
            let worst = major.first().copied().unwrap_or(0.0);
            let rest: f32 = major.iter().skip(1).sum::<f32>() + minor.iter().skip(1).sum::<f32>();
            (worst + self.other_edges_share * rest).min(self.total_cap_db + self.octaves(self.bands_hz[band]) * self.cap_db_per_octave)
        })
    }

    fn octaves(&self, hz: f32) -> f32 {
        (hz / self.bands_hz[0]).log2().max(0.0)
    }

    fn air(&self, metres: f32) -> Bands {
        self.air_db_per_km.map(|per_km| per_km * metres / 1000.0)
    }

    fn walls(&self, count: u32) -> Bands {
        self.bands_hz.map(|hz| {
            let one = self.wall_db_at_125 + self.wall_db_per_octave * (hz / 125.0).log2().max(0.0);
            (one * count as f32).min(self.wall_cap_db)
        })
    }

    /// The loss at a pitch between bands, along a straight line in log frequency.
    fn loss_at(&self, loss: &Bands, hz: f32) -> f32 {
        let at = |i: usize| (self.bands_hz[i].ln(), loss[i]);
        if hz <= self.bands_hz[0] {
            return loss[0];
        }
        for i in 1..BANDS {
            if hz <= self.bands_hz[i] {
                let ((f0, l0), (f1, l1)) = (at(i - 1), at(i));
                return l0 + (l1 - l0) * (hz.ln() - f0) / (f1 - f0);
            }
        }
        loss[BANDS - 1]
    }

    /// As a volume and a cut: the volume from the loss at `gain_hz`, the cut where it is
    /// `cutoff_excess_db` worse than that.
    pub fn heard(&self, loss: &Bands) -> Heard {
        let base = self.loss_at(loss, self.gain_hz);
        let gain = 10f32.powf(-base / 20.0);
        let (lowest, highest) = self.cutoff_range_hz;
        let wanted = base + self.cutoff_excess_db;
        let mut cutoff = highest.max(20_000.0);
        let mut previous = (self.gain_hz, base);
        for (&hz, &l) in self.bands_hz.iter().zip(loss) {
            if hz <= self.gain_hz {
                continue;
            }
            if l >= wanted {
                let (h0, l0) = previous;
                let share = ((wanted - l0) / (l - l0).max(1e-6)).clamp(0.0, 1.0);
                cutoff = (h0.ln() + (hz.ln() - h0.ln()) * share).exp();
                break;
            }
            previous = (hz, l);
        }
        Heard { gain, cutoff_hz: cutoff.max(lowest) }
    }

    /// What lies between `a` and `b` out of doors: the corners of a band stretched over the ground and
    /// over anything built on it, each an edge the sound bends over.
    fn outdoor_edges(&self, ground: &impl Ground, a: Vec3, b: Vec3, rays: &mut usize, edges: &mut Vec<Edge>) {
        let flat = Vec2::new(b.x - a.x, b.z - a.z);
        let across = flat.length();
        if across < self.ends_clear * 2.0 {
            return;
        }
        let length = a.distance(b);
        let line = |s: f32| a.y + (b.y - a.y) * s / across;
        let spot = |s: f32| Vec2::new(a.x, a.z) + flat * (s / across);
        let mut profile: Vec<(f32, f32)> = Vec::new();

        let looks = ((across / self.land_step.max(0.5)).ceil() as usize).clamp(2, self.land_looks.max(2));
        for i in 1..looks {
            let s = across * i as f32 / looks as f32;
            if s < self.ends_clear || s > across - self.ends_clear {
                continue;
            }
            let at = spot(s);
            if let Some(height) = ground.land(at.x, at.y) {
                profile.push((s, height));
            }
        }

        // Anything built in the way: where the line runs into it, then its top all the way across it.
        let ceiling = a.y.max(b.y) + 80.0;
        let top_at = |at: Vec2, rays: &mut usize| {
            *rays += 1;
            ground.ray(Vec3::new(at.x, ceiling, at.y), Vec3::NEG_Y, ceiling + 1000.0).map(|down| ceiling - down)
        };
        let dir = (b - a) / length.max(1e-3);
        let mut s = 0.0;
        while *rays < self.rays && s < across {
            *rays += 1;
            let from = a + dir * (s * length / across);
            let Some(hit) = ground.ray(from, dir, length - s * length / across) else {
                break;
            };
            let hit_s = s + hit * across / length;
            let at = spot(hit_s);
            let here = line(hit_s);
            let land = ground.land(at.x, at.y);
            let hit_y = a.y + (b.y - a.y) * hit_s / across;
            let _ = here;
            // Into the land, unless something stands on it there.
            let bare = land.is_some_and(|land| hit_y - land < 0.3 && top_at(spot(hit_s + 0.1), rays).is_none_or(|top| top - land < 0.3));
            if bare {
                // Into the land, which the looks at its height have already. On to where it is clear again.
                let mut next = hit_s;
                loop {
                    next += self.land_step.max(0.5);
                    let at = spot(next);
                    if next >= across || ground.land(at.x, at.y).is_none_or(|land| land < line(next) - 0.05) {
                        break;
                    }
                }
                s = next;
                continue;
            }
            let mut next = hit_s + 0.05;
            while next < across && *rays < self.rays {
                let Some(top) = top_at(spot(next), rays) else {
                    break;
                };
                profile.push((next, top));
                if top < line(next) {
                    break;
                }
                next += self.top_step.max(0.1);
            }
            s = next + 0.01;
        }

        // The band over the top: the upper hull of the ends and whatever stands above the line between them.
        profile.retain(|&(s, height)| height > line(s));
        if profile.is_empty() {
            return;
        }
        profile.sort_by(|p, q| p.0.total_cmp(&q.0));
        let mut hull: Vec<(f32, f32)> = vec![(0.0, a.y)];
        for point in profile.into_iter().chain(std::iter::once((across, b.y))) {
            while hull.len() >= 2 {
                let (o, p) = (hull[hull.len() - 2], hull[hull.len() - 1]);
                let turn = (p.0 - o.0) * (point.1 - o.1) - (p.1 - o.1) * (point.0 - o.0);
                if turn >= 0.0 {
                    hull.pop();
                } else {
                    break;
                }
            }
            hull.push(point);
        }
        let far = |p: (f32, f32), q: (f32, f32)| ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt();
        let over: f32 = hull.windows(2).map(|w| far(w[0], w[1])).sum();
        let highest = hull.iter().map(|&(s, height)| height - line(s)).fold(0.0, f32::max);
        edges.push(Edge { longer: over - far(hull[0], hull[hull.len() - 1]), rise: highest, minor: false });
        for w in hull.windows(3) {
            let (before, corner, after) = (w[0], w[1], w[2]);
            let longer = far(before, corner) + far(corner, after) - far(before, after);
            let under = before.1 + (after.1 - before.1) * (corner.0 - before.0) / (after.0 - before.0).max(1e-3);
            edges.push(Edge { longer, rise: corner.1 - under, minor: true });
        }
    }
}

/// An opening between two spaces, each a room or the outside.
#[derive(Clone, Copy, Debug)]
struct Gap {
    center: Vec3,
    outward: Vec3,
    half_width: f32,
    bottom: f32,
    top: f32,
    area: f32,
    /// The room behind it and the one it opens onto, None being outside.
    sides: [Option<u32>; 2],
}

impl Gap {
    fn other(&self, space: Option<u32>) -> Option<u32> {
        if self.sides[0] == space { self.sides[1] } else { self.sides[0] }
    }

    /// The point of the opening nearest the straight way from `before` to `after`, and how far off that
    /// way it is.
    fn through(&self, before: Vec3, after: Vec3) -> (Vec3, f32) {
        let across = after - before;
        let facing = across.dot(self.outward);
        let crossing = if facing.abs() > 1e-4 { before + across * ((self.center - before).dot(self.outward) / facing).clamp(0.0, 1.0) } else { self.center };
        let along = self.outward.cross(Vec3::Y).normalize_or(Vec3::X);
        let sideways = (crossing - self.center).dot(along).clamp(-self.half_width, self.half_width);
        let point = Vec3::new(self.center.x, crossing.y.clamp(self.bottom, self.top), self.center.z) + along * sideways;
        (point, point.distance(crossing))
    }
}

#[derive(Clone, Copy)]
struct Space {
    center: Vec3,
    half: Vec3,
    turn_back: Quat,
}

impl Space {
    fn contains(&self, p: Vec3) -> bool {
        (self.turn_back * (p - self.center)).abs().cmple(self.half).all()
    }
}

/// The rooms of a level and how their doors and windows join them, worked out once.
#[derive(Default)]
pub struct Spaces {
    rooms: Vec<Space>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    gaps: Vec<Gap>,
    room_gaps: Vec<Vec<u32>>,
}

const SPACE_CELL: f32 = 16.0;

fn cell_of(p: Vec3) -> (i32, i32) {
    ((p.x / SPACE_CELL).floor() as i32, (p.z / SPACE_CELL).floor() as i32)
}

impl Spaces {
    pub fn new(level: &Level, cfg: &Propagation) -> Self {
        let mut spaces = Spaces::default();
        for (i, room) in level.rooms.iter().enumerate() {
            spaces.rooms.push(Space { center: room.center, half: room.half, turn_back: room.rotation.conjugate() });
            let reach = Vec2::new(room.half.x, room.half.z).length();
            let (low, high) = (cell_of(room.center - Vec3::splat(reach)), cell_of(room.center + Vec3::splat(reach)));
            for x in low.0..=high.0 {
                for z in low.1..=high.1 {
                    spaces.cells.entry((x, z)).or_default().push(i as u32);
                }
            }
        }
        spaces.room_gaps = vec![Vec::new(); spaces.rooms.len()];
        let doors = level.doorways.iter().map(|d| (d.at + Vec3::Y * cfg.door_height * 0.5, d.outward, d.width, cfg.door_height));
        let windows = level.windows.iter().map(|w| (w.at, w.outward, w.width, w.height));
        for (center, outward, width, height) in doors.chain(windows) {
            let outward = Vec3::new(outward.x, 0.0, outward.z).normalize_or(Vec3::X);
            let sides = [spaces.space_of(center - outward * 0.4), spaces.space_of(center + outward * 0.4)];
            if sides[0] == sides[1] {
                continue;
            }
            let index = spaces.gaps.len() as u32;
            spaces.gaps.push(Gap {
                center,
                outward,
                half_width: width * 0.5,
                bottom: center.y - height * 0.5,
                top: center.y + height * 0.5,
                area: width * height,
                sides,
            });
            for room in sides.into_iter().flatten() {
                spaces.room_gaps[room as usize].push(index);
            }
        }
        spaces
    }

    /// The room this is in, if any.
    pub fn space_of(&self, p: Vec3) -> Option<u32> {
        self.cells.get(&cell_of(p))?.iter().copied().find(|&room| self.rooms[room as usize].contains(p))
    }

    /// The ways from `start` through doors and windows to `goal`, as the openings each goes through, a
    /// room at most `depth` openings away.
    fn ways(&self, start: u32, goal: Option<u32>, depth: usize, most: usize) -> Vec<Vec<u32>> {
        let mut found = Vec::new();
        let mut visited = vec![start];
        let mut through = Vec::new();
        self.walk(start, goal, depth, most, &mut visited, &mut through, &mut found);
        found
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(&self, room: u32, goal: Option<u32>, depth: usize, most: usize, visited: &mut Vec<u32>, through: &mut Vec<u32>, found: &mut Vec<Vec<u32>>) {
        if through.len() >= depth {
            return;
        }
        for &gap in &self.room_gaps[room as usize] {
            if found.len() >= most {
                return;
            }
            through.push(gap);
            match self.gaps[gap as usize].other(Some(room)) {
                next if next == goal => found.push(through.clone()),
                None => {
                    // Out, and in again by one of the goal's openings onto the outside.
                    if let Some(goal) = goal
                        && through.len() < depth
                    {
                        for &into in &self.room_gaps[goal as usize] {
                            if self.gaps[into as usize].other(Some(goal)).is_none() && found.len() < most {
                                let mut way = through.clone();
                                way.push(into);
                                found.push(way);
                            }
                        }
                    }
                }
                Some(next) if !visited.contains(&next) => {
                    visited.push(next);
                    self.walk(next, goal, depth, most, visited, through, found);
                    visited.pop();
                }
                Some(_) => {}
            }
            through.pop();
        }
    }

    fn adjoining(&self, a: u32, b: u32) -> bool {
        self.room_gaps[a as usize].iter().any(|&gap| self.gaps[gap as usize].other(Some(a)) == Some(b))
    }
}

/// The loss in each band from `source` to `ear`, by every way there is, added up as sound adds up.
pub fn loss(cfg: &Propagation, spaces: &Spaces, ground: &impl Ground, source: Vec3, ear: Vec3) -> Bands {
    let (here, there) = (spaces.space_of(ear), spaces.space_of(source));
    let air = cfg.air(source.distance(ear));
    let mut rays = 0;
    if here == there {
        if here.is_some() {
            return air;
        }
        let mut edges = Vec::new();
        cfg.outdoor_edges(ground, source, ear, &mut rays, &mut edges);
        let over = cfg.edges_loss(&edges);
        return std::array::from_fn(|band| over[band] + air[band]);
    }

    // Through the walls between.
    let walls = match (here, there) {
        (Some(a), Some(b)) if spaces.adjoining(a, b) => 1,
        (Some(_), Some(_)) => 2,
        _ => 1,
    };
    let wall_loss = cfg.walls(walls);
    let mut ways: Vec<Bands> = vec![std::array::from_fn(|band| wall_loss[band] + air[band])];

    // Through the doors and windows. The search starts from whichever end is in a room.
    let (start, goal, from, to) = match here {
        Some(room) => (room, there, ear, source),
        None => (there.expect("one end is in a room"), here, source, ear),
    };
    for way in spaces.ways(start, goal, cfg.openings, cfg.ways) {
        let gaps: Vec<Gap> = way.iter().map(|&gap| spaces.gaps[gap as usize]).collect();
        let mut points: Vec<Vec3> = std::iter::once(from).chain(gaps.iter().map(|gap| gap.center)).chain(std::iter::once(to)).collect();
        let mut rises = vec![0.0; gaps.len()];
        // Twice, as where each opening is passed depends on where the next one is.
        for _ in 0..2 {
            for (i, gap) in gaps.iter().enumerate() {
                let (point, rise) = gap.through(points[i], points[i + 2]);
                points[i + 1] = point;
                rises[i] = rise;
            }
        }
        let mut edges: Vec<Edge> = (0..gaps.len())
            .map(|i| {
                let (before, at, after) = (points[i], points[i + 1], points[i + 2]);
                Edge { longer: before.distance(at) + at.distance(after) - before.distance(after), rise: rises[i], minor: false }
            })
            .collect();
        // Whatever stands between the openings and ends that are out of doors.
        let mut space = Some(start);
        for i in 0..points.len() - 1 {
            if space.is_none() {
                cfg.outdoor_edges(ground, points[i], points[i + 1], &mut rays, &mut edges);
            }
            if let Some(gap) = gaps.get(i) {
                space = gap.other(space);
            }
        }
        let small: f32 = gaps.iter().map(|gap| -10.0 * (gap.area / cfg.opening_full_area.max(1e-3)).min(1.0).log10()).sum();
        let bent = cfg.edges_loss(&edges);
        let length: f32 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
        let air = cfg.air(length);
        ways.push(std::array::from_fn(|band| bent[band] + small + air[band]));
    }
    // Never more than in the open, however many ways there are.
    std::array::from_fn(|band| (-10.0 * ways.iter().map(|way| 10f32.powf(-way[band] / 10.0)).sum::<f32>().log10()).max(air[band]))
}

/// Answers for the sounds playing, kept a moment and limited a frame, over the level being played.
#[derive(Resource, Default)]
pub struct Propagator {
    level: Option<(u64, usize, usize)>,
    spaces: Spaces,
    remembered: HashMap<(IVec3, IVec3), (f32, Bands)>,
    frame: f32,
    worked_out: u32,
}

impl Propagator {
    /// How `source` is heard at `ear` at time `now`, in seconds.
    pub fn heard(&mut self, cfg: &Propagation, level: &Level, ground: &impl Ground, source: Vec3, ear: Vec3, now: f32) -> Heard {
        let key = (level.source, level.rooms.len(), level.doorways.len() + level.windows.len());
        if self.level != Some(key) {
            self.level = Some(key);
            self.spaces = Spaces::new(level, cfg);
            self.remembered.clear();
        }
        if now != self.frame {
            self.frame = now;
            self.worked_out = 0;
            if self.remembered.len() > 4096 {
                self.remembered.retain(|_, (at, _)| now - *at < cfg.remember_for);
            }
        }
        let cell = |p: Vec3, size: f32| (p / size.max(0.1)).floor().as_ivec3();
        let at = (cell(source, cfg.source_cell), cell(ear, cfg.ear_cell));
        let fresh = self.remembered.get(&at).filter(|(when, _)| now - when < cfg.remember_for);
        if let Some((_, bands)) = fresh {
            return cfg.heard(bands);
        }
        if self.worked_out >= cfg.per_frame {
            let open = self.remembered.get(&at).map_or_else(|| cfg.air(source.distance(ear)), |(_, bands)| *bands);
            return cfg.heard(&open);
        }
        self.worked_out += 1;
        let bands = loss(cfg, &self.spaces, ground, source, ear);
        self.remembered.insert(at, (now, bands));
        cfg.heard(&bands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flat ground at 0 with a ridge across it at x = `at`, `high` metres tall and `thick` across.
    struct Ridge {
        at: f32,
        high: f32,
        thick: f32,
    }

    impl Ridge {
        fn height(&self, x: f32) -> f32 {
            if (x - self.at).abs() < self.thick * 0.5 { self.high } else { 0.0 }
        }
    }

    impl Ground for Ridge {
        fn ray(&self, origin: Vec3, dir: Vec3, max_distance: f32) -> Option<f32> {
            let mut t = 0.0;
            while t <= max_distance {
                let p = origin + dir * t;
                if p.y <= self.height(p.x) {
                    return Some(t);
                }
                t += 0.05;
            }
            None
        }

        fn land(&self, x: f32, _z: f32) -> Option<f32> {
            Some(self.height(x))
        }
    }

    fn heard_over(ridge: &Ridge, across: f32) -> Heard {
        let cfg = Propagation::default();
        let bands = loss(&cfg, &Spaces::default(), ridge, Vec3::new(0.0, 1.6, 0.0), Vec3::new(across, 1.6, 0.0));
        cfg.heard(&bands)
    }

    #[test]
    fn open_ground_loses_only_its_top_end_to_the_air_with_distance() {
        let flat = Ridge { at: 1e6, high: 0.0, thick: 0.0 };
        let (near, far) = (heard_over(&flat, 50.0), heard_over(&flat, 600.0));
        assert!(near.gain > 0.99 && far.gain > 0.95, "{near:?} {far:?}");
        assert!(far.cutoff_hz < near.cutoff_hz && far.cutoff_hz > 600.0 && far.cutoff_hz < 3000.0, "{far:?}");
    }

    #[test]
    fn a_slight_crest_takes_a_little_and_a_big_one_takes_more_but_never_all() {
        let slight = heard_over(&Ridge { at: 50.0, high: 2.0, thick: 5.0 }, 100.0);
        let big = heard_over(&Ridge { at: 50.0, high: 8.0, thick: 5.0 }, 100.0);
        assert!(slight.gain > 0.7, "{slight:?}");
        assert!(big.gain < slight.gain && big.cutoff_hz < slight.cutoff_hz, "{slight:?} {big:?}");
        assert!(big.gain > 10f32.powf(-18.0 / 20.0) * 0.99, "the lows always wrap round: {big:?}");
    }

    #[test]
    fn crossing_a_crest_changes_things_gradually() {
        let mut last: Option<Heard> = None;
        for tenth in 0..40 {
            let high = 1.4 + tenth as f32 * 0.05;
            let now = heard_over(&Ridge { at: 50.0, high, thick: 5.0 }, 100.0);
            if let Some(last) = last {
                assert!((20.0 * (now.gain / last.gain).log10()).abs() < 1.0, "a step at {high} m: {last:?} to {now:?}");
            }
            last = Some(now);
        }
    }

    #[test]
    fn more_ridges_take_more_but_less_than_each_alone_added() {
        let cfg = Propagation::default();
        let one = cfg.edges_loss(&[Edge { longer: 0.5, rise: 3.0, minor: false }]);
        let three = cfg.edges_loss(&[Edge { longer: 0.5, rise: 3.0, minor: false }; 3]);
        assert!(three[2] > one[2] && three[2] < one[2] * 3.0);
    }

    #[test]
    fn walls_let_the_lows_through_and_cut_the_highs() {
        let cfg = Propagation::default();
        let heard = cfg.heard(&cfg.walls(1));
        assert!(heard.cutoff_hz < 400.0 && heard.gain < 0.1, "{heard:?}");
    }

    /// What it was before: four straight rays and a count of walls round each end, as a volume and a cut.
    fn before(world: &CollisionWorld, source: Vec3, ear: Vec3) -> (f32, f32) {
        let blocked = |from: Vec3, to: Vec3| {
            let path = to - from;
            world.raycast(from, path.normalize_or(Vec3::Y), path.length()).is_some()
        };
        let indoors = |p: Vec3| {
            blocked(p, p + Vec3::Y * 12.0)
                && (0..8).filter(|i| {
                    let a = *i as f32 * std::f32::consts::FRAC_PI_4;
                    blocked(p, p + Vec3::new(a.cos(), 0.0, a.sin()) * 15.0)
                })
                .count()
                    >= 3
        };
        let toward = (ear - source).normalize_or_zero();
        let from = source + toward * 0.3_f32.min(source.distance(ear) * 0.5);
        let side = toward.cross(Vec3::Y).normalize_or(Vec3::X) * 0.8;
        let hits = [Vec3::ZERO, side, -side, Vec3::Y * 0.8].iter().filter(|&&o| blocked(from + o, ear)).count();
        let score = hits as f32 / 4.0 + if indoors(source) != indoors(ear) { 0.6 } else { 0.0 };
        [(1.25, 260.0, 0.2), (1.0, 600.0, 0.35), (0.25, 1800.0, 0.6)].into_iter().find(|l| score >= l.0).map_or((1.0, 20_000.0), |l| (l.2, l.1))
    }

    struct Map {
        name: &'static str,
        level: Level,
        world: CollisionWorld,
        spaces: Spaces,
    }

    impl Map {
        fn new(name: &'static str) -> Self {
            let level = Level::named(name).unwrap();
            let world = level.collision();
            let spaces = Spaces::new(&level, &Propagation::default());
            Map { name, level, world, spaces }
        }

        /// A standing ear on open ground here, if there is ground and no room.
        fn stand(&self, x: f32, z: f32) -> Option<Vec3> {
            let ground = self.world.land_height(x, z)?;
            let p = Vec3::new(x, ground + 1.6, z);
            let above = self.world.raycast(Vec3::new(x, ground + 60.0, z), Vec3::NEG_Y, 80.0)?;
            (self.spaces.space_of(p).is_none() && (ground + 60.0 - above.distance - ground).abs() < 0.3).then_some(p)
        }

        /// How many separate built things the straight line runs into, the land left out.
        fn things_between(&self, a: Vec3, b: Vec3) -> usize {
            let (dir, length) = ((b - a).normalize(), a.distance(b));
            let (mut t, mut count) = (0.0, 0);
            while let Some(hit) = self.world.raycast(a + dir * t, dir, length - t) {
                let p = a + dir * (t + hit.distance);
                let land = self.world.land_height(p.x, p.z).unwrap_or(f32::MIN);
                if p.y - land > 0.3 {
                    count += 1;
                }
                // On past it: to where the line is clear of it again.
                t += hit.distance + 0.3;
                while t < length && !self.world.raycast(a + dir * t + Vec3::Y * 0.05, Vec3::NEG_Y, 0.1).is_none() {
                    t += 0.3;
                }
                t += 0.3;
                if count > 30 || t >= length {
                    break;
                }
            }
            count
        }

        fn spots(&self) -> Vec<Vec3> {
            let mut out = Vec::new();
            for i in -40..=40 {
                for j in -40..=40 {
                    if let Some(p) = self.stand(i as f32 * 12.0, j as f32 * 12.0) {
                        out.push(p);
                    }
                }
            }
            out
        }

        /// The first pair, `across` metres apart, that `fits`.
        fn pair(&self, across: f32, fits: impl Fn(Vec3, Vec3) -> bool) -> Option<(Vec3, Vec3)> {
            for ear in self.spots() {
                for k in 0..24 {
                    let a = k as f32 * std::f32::consts::TAU / 24.0;
                    let Some(source) = self.stand(ear.x + a.cos() * across, ear.z + a.sin() * across) else { continue };
                    if fits(ear, source) {
                        return Some((ear, source));
                    }
                }
            }
            None
        }

        fn row(&self, what: &str, source: Vec3, ear: Vec3, expect: &str) {
            let cfg = Propagation::default();
            let (old_gain, old_cut) = before(&self.world, source, ear);
            let started = std::time::Instant::now();
            let new = cfg.heard(&loss(&cfg, &self.spaces, &self.world, source, ear));
            let took = started.elapsed().as_secs_f32() * 1000.0;
            let shot = |gain: f32| 20.0 * (10.0 / (10.0 + source.distance(ear)) * gain).log10();
            println!(
                "{:9} {:34} {:5.0} m | before {:6.1} dB {:6.0} Hz  shot {:6.1} dB | after {:6.1} dB {:6.0} Hz  shot {:6.1} dB {took:5.2} ms | {expect}",
                self.name, what, source.distance(ear), 20.0 * old_gain.log10(), old_cut, shot(old_gain), 20.0 * new.gain.log10(), new.cutoff_hz, shot(new.gain)
            );
        }
    }

    #[test]
    #[ignore = "prints how sound gets about on the maps, before and after"]
    fn report() {
        let cfg = Propagation::default();
        let ridge = |high: f32| {
            let r = Ridge { at: 50.0, high, thick: 6.0 };
            cfg.heard(&loss(&cfg, &Spaces::default(), &r, Vec3::new(0.0, 1.6, 0.0), Vec3::new(100.0, 1.6, 0.0)))
        };
        for high in [2.0, 3.6, 6.0, 12.0] {
            let h = ridge(high);
            println!("made up   ridge {high:4.1} m tall half way          100 m | after {:6.1} dB {:6.0} Hz | slight: about a dB and the very top; tall: up to about -12 dB, dull", 20.0 * h.gain.log10(), h.cutoff_hz);
        }
        for name in ["town", "claudabad"] {
            let map = Map::new(name);
            for across in [50.0, 200.0, 600.0] {
                match map.pair(across, |a, b| map.things_between(a, b) == 0 && {
                    let mut edges = Vec::new();
                    cfg.outdoor_edges(&map.world, a, b, &mut 0, &mut edges);
                    edges.iter().all(|e| e.rise < cfg.rise_counts_from)
                }) {
                    Some((ear, source)) => map.row("open ground", source, ear, "nothing in the way: highs fade with range only"),
                    None => println!("{name:9} open ground {across} m: no clear line found"),
                }
            }
            let crest = map.pair(100.0, |a, b| {
                if map.things_between(a, b) == 0 {
                    return false;
                }
                let mut edges = Vec::new();
                cfg.outdoor_edges(&map.world, a, b, &mut 0, &mut edges);
                let worst = edges.iter().map(|e| e.rise).fold(0.0, f32::max);
                (1.0..4.0).contains(&worst) && map.world.raycast(a, (b - a).normalize(), a.distance(b)).is_some_and(|hit| {
                    let p = a + (b - a).normalize() * hit.distance;
                    map.world.land_height(p.x, p.z).is_some_and(|land| (land - p.y).abs() < 0.5)
                })
            });
            match crest {
                Some((ear, source)) => map.row("over a crest of the land", source, ear, "a few dB to about -10 dB, dull, never silent"),
                None => println!("{name:9} no crest 1 to 4 m above the line found at 100 m"),
            }
            let behind = map.level.rooms.iter().find_map(|room| {
                (0..8).find_map(|k| {
                    let turn = k as f32 * std::f32::consts::FRAC_PI_4;
                    let d = Vec3::new(turn.cos(), 0.0, turn.sin());
                    let reach = Vec2::new(room.half.x, room.half.z).length() + 12.0;
                    let ear = map.stand(room.center.x + d.x * reach, room.center.z + d.z * reach)?;
                    let source = map.stand(room.center.x - d.x * reach, room.center.z - d.z * reach)?;
                    (map.things_between(source, ear) == 1).then_some((ear, source))
                })
            });
            match behind {
                Some((ear, source)) => map.row("behind one building", source, ear, "a few dB to about -10 dB, edge gone"),
                None => println!("{name:9} no building with open ground both sides"),
            }
            match map.pair(120.0, |a, b| (3..=8).contains(&map.things_between(a, b))) {
                Some((ear, source)) => map.row("three streets over", source, ear, "about -8 to -15 dB, thumps not snaps"),
                None => println!("{name:9} no line with 3 to 8 things between"),
            }
            let door = map.level.doorways.iter().find(|d| {
                let out = Vec3::new(d.outward.x, 0.0, d.outward.z).normalize();
                map.spaces.space_of(d.at + Vec3::Y - out * 0.4).is_some()
                    && map.spaces.space_of(d.at + Vec3::Y + out * 0.4).is_none()
                    && map.stand(d.at.x + out.x * 15.0, d.at.z + out.z * 15.0).is_some()
            });
            if let Some(door) = door {
                let out = Vec3::new(door.outward.x, 0.0, door.outward.z).normalize();
                let room = map.spaces.space_of(door.at + Vec3::Y - out * 0.4).unwrap();
                let inside = door.at - out * 1.5 + Vec3::Y * 1.6;
                let in_front = map.stand(door.at.x + out.x * 15.0, door.at.z + out.z * 15.0).unwrap();
                map.row("you indoors, it out by the door", in_front, inside, "near open air through the doorway");
                let space = map.spaces.rooms[room as usize];
                let behind = (1..40).map(|m| inside - out * m as f32).find(|p| !space.contains(*p)).map(|p| p - out * 14.0);
                match behind.and_then(|p| map.stand(p.x, p.z)) {
                    Some(behind) => map.row("you indoors, it behind a wall", behind, inside, "lows only, by the door and over the roof or through the wall"),
                    None => println!("{name:9} nothing open behind that room"),
                }
                let across = out.cross(Vec3::Y) * 0.8;
                map.row("both in the same room", inside + across - out, inside - across - out, "as if in the open");
            } else {
                println!("{name:9} no door out of a room with ground in front of it");
            }
        }
    }
}
