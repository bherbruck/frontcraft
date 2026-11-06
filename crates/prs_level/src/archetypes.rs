//! Kinds of building, sized to order. Each comes with its door on the side that faces +z, so a map
//! turns it with yaw to face its yard. Sizes smaller than a stair run needs are the caller's error,
//! and the tests that walk every staircase will say so.

use crate::Paint;
use crate::spec::*;
use glam::Vec2;

impl Building {
    pub fn new(at: (f32, f32), yaw: f32, half: (f32, f32), storeys: u32, walls: Paint, floors: Paint, roof: Roof) -> Self {
        Self {
            at: at.into(),
            yaw,
            half: half.into(),
            storeys,
            storey_height: None,
            walls,
            floors,
            stairs: Paint::Stone,
            roof,
            openings: Vec::new(),
            partitions: Vec::new(),
            flights: Vec::new(),
            fixtures: Vec::new(),
        }
    }

    /// Puts a prop on a flat roof, at `at` from the middle.
    pub fn on_roof(mut self, s: &BuildStyle, prop: Prop, at: (f32, f32)) -> Self {
        let top = s.plinth + s.storey * self.storeys as f32;
        self.fixtures.extend(prop.parts().iter().map(|&(p, half, yaw, paint)| ([p[0] + at.0, p[1] + top, p[2] + at.1], half, yaw, paint)));
        self
    }

    /// A pair of shutters folded back beside each window on the two long walls.
    fn shuttered(mut self, s: &BuildStyle) -> Self {
        for o in self.openings.clone() {
            if o.kind != OpeningKind::Window || !matches!(o.side, Side::N | Side::S) {
                continue;
            }
            let z = o.side.normal().y * (self.half.y + 0.03);
            let y = s.plinth + (s.window_sill + s.window_head) * 0.5;
            for x in [o.along - s.window_width * 0.77, o.along + s.window_width * 0.77] {
                self.fixtures.push(([x, y, z], [s.window_width * 0.27, (s.window_head - s.window_sill) * 0.5, 0.03], 0.0, Paint::Wood));
            }
        }
        self
    }

    /// A canvas awning sticking out over part of the south wall.
    fn awning(mut self, s: &BuildStyle, along: f32, half_width: f32) -> Self {
        let z = self.half.y + s.awning_depth * 0.5;
        self.fixtures.push(([along, s.plinth + s.awning_height, z], [half_width, 0.04, s.awning_depth * 0.5], 0.0, Paint::Canvas));
        self
    }

    pub fn opening(mut self, side: Side, storey: u32, along: f32, kind: OpeningKind) -> Self {
        self.openings.push(Opening { side, storey, along, kind });
        self
    }

    pub fn door(self, side: Side, along: f32) -> Self {
        self.opening(side, 0, along, OpeningKind::Door)
    }

    pub fn wide_door(self, side: Side, along: f32) -> Self {
        self.opening(side, 0, along, OpeningKind::WideDoor)
    }

    pub fn window(self, side: Side, storey: u32, along: f32) -> Self {
        self.opening(side, storey, along, OpeningKind::Window)
    }

    pub fn partition(mut self, storey: u32, from: (f32, f32), to: (f32, f32), doors: &[f32]) -> Self {
        self.partitions.push(Partition { storey, from: from.into(), to: to.into(), doors: doors.to_vec() });
        self
    }

    pub fn flight(mut self, flight: Flight) -> Self {
        self.flights.push(flight);
        self
    }

    /// Stairs up the outside of the east wall to a flat roof. They take the east wall's windows' place.
    pub fn roof_stairs(mut self, s: &BuildStyle) -> Self {
        self.openings.retain(|o| o.side != Side::E);
        let height = s.plinth + s.storey * self.storeys as f32;
        let run = (height / s.stair_rise).ceil() * s.stair_run;
        let landing = -self.half.y + 0.3 + s.stair_width * 0.5;
        self.flights.push(Flight {
            foot: Vec2::new(self.half.x + s.stair_width * 0.5 + 0.03, landing + s.stair_width * 0.5 + run),
            ascent: Side::N,
            from: None,
            to: self.storeys,
            finish: FlightFinish::Landing { exit: Side::W },
        });
        self
    }

    /// A partition across the middle of a storey at `x`, with a door halfway along.
    fn divide(self, s: &BuildStyle, storey: u32, x: f32, door_at: f32) -> Self {
        let z = self.half.y - s.wall;
        self.partition(storey, (x, -z), (x, z), &[z + door_at])
    }
}

/// One storey, two rooms and a flat roof.
pub fn house(s: &BuildStyle, at: (f32, f32), yaw: f32, half: (f32, f32), walls: Paint) -> Building {
    let (hx, _) = half;
    Building::new(at, yaw, half, 1, walls, Paint::Packed, Roof::Flat { paint: Paint::Packed })
        .door(Side::S, -hx * 0.45)
        .window(Side::S, 0, hx * 0.55)
        .window(Side::N, 0, -hx * 0.5)
        .window(Side::N, 0, hx * 0.55)
        .window(Side::W, 0, 0.0)
        .window(Side::E, 0, 0.0)
        .divide(s, 0, hx * 0.15, 0.0)
}

/// One storey under a pitched roof.
pub fn cottage(s: &BuildStyle, at: (f32, f32), yaw: f32, half: (f32, f32), walls: Paint, roof: Paint) -> Building {
    let (hx, _) = half;
    Building::new(at, yaw, half, 1, walls, Paint::Planks, Roof::Gable { paint: roof, pitch: 0.5 })
        .door(Side::S, hx * 0.3)
        .window(Side::S, 0, -hx * 0.5)
        .window(Side::N, 0, -hx * 0.45)
        .window(Side::N, 0, hx * 0.5)
        .window(Side::W, 0, 0.0)
        .divide(s, 0, -hx * 0.1, 0.5)
        .shuttered(s)
}

/// Two storeys joined by a flight along the west wall.
pub fn townhouse(s: &BuildStyle, at: (f32, f32), yaw: f32, half: (f32, f32), walls: Paint, roof: Roof) -> Building {
    let (hx, hz) = half;
    let stair_x = -hx + s.wall + s.stair_width * 0.5 + 0.02;
    let px = -hx * 0.08;
    let mut house = Building::new(at, yaw, half, 2, walls, Paint::Planks, roof);
    house.stairs = Paint::Wood;
    house
        .door(Side::S, hx * 0.5)
        .window(Side::S, 0, -hx * 0.3)
        .window(Side::N, 0, hx * 0.5)
        .window(Side::E, 0, -hz * 0.3)
        .window(Side::S, 1, -hx * 0.5)
        .window(Side::S, 1, hx * 0.5)
        .window(Side::N, 1, -hx * 0.5)
        .window(Side::N, 1, hx * 0.5)
        .window(Side::E, 1, 0.0)
        .window(Side::W, 1, hz * 0.45)
        .divide(s, 0, px, 0.0)
        .divide(s, 1, px, 0.0)
        .flight(Flight {
            foot: Vec2::new(stair_x, hz - s.wall - 1.2),
            ascent: Side::N,
            from: Some(0),
            to: 1,
            finish: FlightFinish::Through { open_right: true, open_left: false },
        })
}

/// A row of small shops with wide fronts and a door out the back.
pub fn shop_row(s: &BuildStyle, at: (f32, f32), yaw: f32, half: (f32, f32), walls: Paint, bays: u32) -> Building {
    let (hx, hz) = half;
    let bay = hx * 2.0 / bays as f32;
    let mut row = Building::new(at, yaw, half, 1, walls, Paint::FloorTile, Roof::Flat { paint: Paint::Packed });
    for i in 0..bays {
        let mid = -hx + bay * (i as f32 + 0.5);
        row = row
            .wide_door(Side::S, mid - bay * 0.12)
            .window(Side::N, 0, mid + bay * 0.2)
            .awning(s, mid - bay * 0.12, s.wide_door_width * 0.5 + 0.5);
        if i + 1 < bays {
            let x = -hx + bay * (i + 1) as f32;
            let z = hz - s.wall;
            row = if i % 2 == 0 { row.partition(0, (x, -z), (x, z), &[z * 0.6]) } else { row.partition(0, (x, -z), (x, z), &[]) };
        }
    }
    row
}

/// One open room behind a wide door.
pub fn shed(at: (f32, f32), yaw: f32, half: (f32, f32), walls: Paint, roof: Paint) -> Building {
    Building::new(at, yaw, half, 1, walls, Paint::Packed, Roof::Gable { paint: roof, pitch: 0.34 })
        .wide_door(Side::S, 0.0)
        .window(Side::W, 0, 0.0)
}

/// A tall open hall with doors at both ends.
pub fn barn(s: &BuildStyle, at: (f32, f32), yaw: f32, half: (f32, f32), walls: Paint, roof: Paint) -> Building {
    let (hx, _) = half;
    let mut barn = Building::new(at, yaw, half, 1, walls, Paint::Packed, Roof::Gable { paint: roof, pitch: 0.55 })
        .wide_door(Side::S, -hx * 0.2)
        .wide_door(Side::N, hx * 0.2)
        .window(Side::S, 0, hx * 0.6)
        .window(Side::N, 0, -hx * 0.6)
        .window(Side::W, 0, 0.0)
        .window(Side::E, 0, 0.0);
    barn.storey_height = Some(s.storey + 1.2);
    barn
}

/// The biggest building in town: a corridor down the middle with rooms either side on two floors,
/// a flight up from each end, and a roof to fight from.
pub fn hall(s: &BuildStyle, at: (f32, f32), yaw: f32, half: (f32, f32), walls: Paint) -> Building {
    let (hx, hz) = half;
    let corridor = 1.4;
    let inner_x = hx - s.wall;
    let flight_z = -corridor + s.partition * 0.5 + s.stair_width * 0.5 + 0.02;
    let door_spots = [inner_x * 0.5, inner_x, inner_x * 1.5];
    let mut hall = Building::new(at, yaw, half, 2, walls, Paint::FloorTile, Roof::Flat { paint: Paint::Cement })
        .door(Side::S, 0.0)
        .door(Side::N, hx * 0.4)
        .flight(Flight {
            foot: Vec2::new(-inner_x + 0.9, flight_z),
            ascent: Side::E,
            from: Some(0),
            to: 1,
            finish: FlightFinish::Through { open_right: true, open_left: false },
        })
        .flight(Flight {
            foot: Vec2::new(inner_x - 0.9, flight_z),
            ascent: Side::W,
            from: Some(1),
            to: 2,
            finish: FlightFinish::Through { open_right: false, open_left: true },
        });
    for storey in 0..2 {
        hall = hall
            .partition(storey, (-inner_x, -corridor), (inner_x, -corridor), &door_spots)
            .partition(storey, (-inner_x, corridor), (inner_x, corridor), &door_spots);
        for x in [-inner_x / 3.0, inner_x / 3.0] {
            let z = hz - s.wall;
            hall = hall.partition(storey, (x, -z), (x, -corridor), &[]).partition(storey, (x, corridor), (x, z), &[]);
        }
        let window_z = (hz + corridor) * 0.5;
        for x in [-inner_x * 2.0 / 3.0, inner_x * 2.0 / 3.0] {
            hall = hall.window(Side::S, storey, x).window(Side::N, storey, x);
        }
        hall = hall.window(Side::E, storey, window_z).window(Side::E, storey, -window_z);
        hall = hall.window(Side::W, storey, window_z).window(Side::W, storey, -window_z);
        if storey == 1 {
            hall = hall.window(Side::S, 1, 0.0).window(Side::N, 1, 0.0);
        } else {
            hall = hall.window(Side::N, 0, 0.0);
        }
    }
    hall
}

/// A narrow lookout, three floors of switchback stairs above everything around it.
pub fn tower(s: &BuildStyle, at: (f32, f32), yaw: f32, half: (f32, f32), walls: Paint) -> Building {
    let (hx, hz) = half;
    let x = hx - s.wall - s.stair_width * 0.5 - 0.02;
    let up = hz - s.wall - 1.2;
    let down = -hz + s.wall + 1.2;
    let mut tower = Building::new(at, yaw, half, 3, walls, Paint::FloorTile, Roof::Flat { paint: Paint::Cement }).door(Side::S, x);
    for storey in 0..3u32 {
        let finish_west = storey.is_multiple_of(2);
        tower = tower.flight(if finish_west {
            Flight {
                foot: Vec2::new(-x, up),
                ascent: Side::N,
                from: Some(storey),
                to: storey + 1,
                finish: FlightFinish::Through { open_right: true, open_left: false },
            }
        } else {
            Flight {
                foot: Vec2::new(x, down),
                ascent: Side::S,
                from: Some(storey),
                to: storey + 1,
                finish: FlightFinish::Through { open_right: true, open_left: false },
            }
        });
        if storey > 0 {
            // Not on the side the stairs run up.
            let clear = if finish_west { Side::E } else { Side::W };
            tower = tower.window(Side::N, storey, 0.0).window(Side::S, storey, 0.0).window(clear, storey, 0.0);
        }
    }
    tower
}
