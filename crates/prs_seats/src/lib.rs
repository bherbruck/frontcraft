//! Seats in anything that carries people, whatever moves it. A carrier is a frame in the world and a
//! list of seats. This crate says who sits where, what they can see and shoot from there, and where they
//! get out. It knows nothing of wheels or rotors, and nothing of soldiers beyond an id.

use glam::{Quat, Vec2, Vec3};

/// Spelt this way so that serde does not take it for text borrowed from what it is reading.
pub type StaticStr = &'static str;

/// Names are text the game shows as they are. One from a mod is read once at startup and stays for good.
pub fn leaked_str<'de, D: serde::Deserializer<'de>>(d: D) -> Result<&'static str, D::Error> {
    let text = <String as serde::Deserialize>::deserialize(d)?;
    Ok(Box::leak(text.into_boxed_str()))
}

/// Where a carrier is. Everything about a seat is given in this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub pos: Vec3,
    pub rot: Quat,
}

impl Frame {
    pub const IDENTITY: Self = Self { pos: Vec3::ZERO, rot: Quat::IDENTITY };

    pub fn point(&self, local: Vec3) -> Vec3 {
        self.pos + self.rot * local
    }

    pub fn to_local(&self, world: Vec3) -> Vec3 {
        self.rot.conjugate() * (world - self.pos)
    }

    /// Which way the frame's -Z points, as a yaw on the ground. Yaw 0 looks down -Z.
    pub fn yaw(&self) -> f32 {
        let f = self.rot * Vec3::NEG_Z;
        (-f.x).atan2(-f.z)
    }

    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        Self { pos: self.pos.lerp(other.pos, t), rot: self.rot.slerp(other.rot, t) }
    }
}

/// An arc of look or fire, as radians from the way the seat faces: yaw positive to the left, as a
/// soldier's yaw is, and pitch positive up.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Arc {
    pub yaw: Vec2,
    pub pitch: Vec2,
}

impl Arc {
    pub fn contains(&self, yaw: f32, pitch: f32) -> bool {
        (self.yaw.x..=self.yaw.y).contains(&yaw) && (self.pitch.x..=self.pitch.y).contains(&pitch)
    }

    /// The nearest look inside the arc.
    pub fn clamp(&self, yaw: f32, pitch: f32) -> (f32, f32) {
        (yaw.clamp(self.yaw.x, self.yaw.y), pitch.clamp(self.pitch.x, self.pitch.y))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SeatRole {
    /// Has the controls, so has no hands for a gun.
    Driver,
    Passenger,
}

/// One seat, in the carrier's frame. Plain data, so a mod can move it.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SeatConfig {
    pub role: SeatRole,
    /// The soldier's eye. The body fits itself to this, and the gun hangs off it through `gun_pose`.
    pub eye: Vec3,
    /// Which way the seat faces, as a yaw from the carrier's -Z.
    pub facing: f32,
    /// The whole body turns with the look, as someone stood in a hatch does. Feet go round the hips.
    pub swivel: bool,
    pub hips: Vec3,
    /// Halfway between the feet, at the ankles.
    pub feet: Vec3,
    /// Where the hands rest if they are on controls, left then right. No gun is held then.
    pub hands: Option<[Vec3; 2]>,
    /// How far they can look.
    pub look: Arc,
    /// Where a gun can bear, if they can use one at all.
    pub fire: Option<Arc>,
}

/// A way in: a door, a hatch, a tailgate. You have to go to it. It serves the seats it says, the first
/// free one, and it is the only way in or out of them, so a door on the far side means walking round or
/// going in and shuffling across.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    #[serde(deserialize_with = "leaked_str")]
    pub name: StaticStr,
    /// Where someone stands to use it, at the feet.
    pub stand: Vec3,
    /// Whether that is the ground under it, wherever the ground is. Otherwise it is a place on the vehicle.
    pub on_ground: bool,
    /// What you put a hand to, and have to be facing: the handle, or the middle of the hatch.
    pub hand: Vec3,
    /// Where the hips pass on the way through.
    pub via: Vec3,
    pub seats: Vec<u8>,
    /// Seconds it takes to go through.
    pub time: f32,
    /// What swings open for someone going through, if anything does.
    pub door: Option<DoorPanel>,
}

/// The part of a carrier that opens. Which part that is depends on what the carrier is made of, so it
/// is by number in the carrier's own list.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DoorPanel {
    pub panel: u8,
    pub hinge: Vec3,
    /// Radians about the up axis, fully open, which way being the sign.
    pub open: f32,
    /// Seconds from shut to fully open.
    pub time: f32,
}

/// Two seats you can move between without getting out.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SeatLink {
    pub a: u8,
    pub b: u8,
    pub time: f32,
}

/// How long it takes to get from one seat to another, if there is a way at all.
pub fn link_between(links: &[SeatLink], a: u8, b: u8) -> Option<f32> {
    links.iter().find(|l| (l.a == a && l.b == b) || (l.a == b && l.b == a)).map(|l| l.time)
}

/// Where a body is, in the carrier's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub eye: Vec3,
    pub hips: Vec3,
    pub feet: Vec3,
}

impl Placed {
    pub fn in_seat(seat: &SeatConfig) -> Self {
        Self { eye: seat.eye, hips: seat.hips, feet: seat.feet }
    }

    fn lerp(&self, other: &Self, t: f32) -> Self {
        Self { eye: self.eye.lerp(other.eye, t), hips: self.hips.lerp(other.hips, t), feet: self.feet.lerp(other.feet, t) }
    }
}

/// How high a standing body's eye and hips are off its feet, which is the soldier's to say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Standing {
    pub eye: f32,
    pub hips: f32,
}

/// On the way through a door, or across to another seat. Plain data: this plus the carrier's frame is
/// where the body is, so a body in a door is as exposed as it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransitKind {
    In { entry: u8 },
    Out { entry: u8 },
    /// To the seat they now hold, from this one.
    Across { from: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transit {
    pub kind: TransitKind,
    /// How far along, 0 to 1. Held to that, so it never drifts.
    pub t: f32,
    /// Going back the way they came, for having been hurt or blocked.
    pub back: bool,
}

impl Transit {
    pub fn new(kind: TransitKind) -> Self {
        Self { kind, t: 0.0, back: false }
    }

    /// Advances it. Says whether it has come to an end, one way or the other.
    pub fn advance(&mut self, time: f32, dt: f32) -> bool {
        let step = dt / time.max(1e-3);
        self.t = (self.t + if self.back { -step } else { step }).clamp(0.0, 1.0);
        if self.back { self.t <= 0.0 } else { self.t >= 1.0 }
    }

    /// Where the body is by now, for someone heading for or leaving `seat`.
    pub fn placed(&self, seats: &[SeatConfig], entries: &[Entry], seat: &SeatConfig, standing: Standing) -> Placed {
        let inside = Placed::in_seat(seat);
        let ease = |t: f32| t * t * (3.0 - 2.0 * t);
        match self.kind {
            TransitKind::Across { from } => seats.get(from as usize).map_or(inside, |other| Placed::in_seat(other).lerp(&inside, ease(self.t))),
            TransitKind::In { entry } | TransitKind::Out { entry } => {
                let Some(entry) = entries.get(entry as usize) else { return inside };
                let going_in = if matches!(self.kind, TransitKind::In { .. }) { self.t } else { 1.0 - self.t };
                let outside = Placed {
                    eye: entry.stand + Vec3::Y * standing.eye,
                    hips: entry.stand + Vec3::Y * standing.hips,
                    feet: entry.stand,
                };
                // Bent over in the opening, with the feet on their way up after the hips.
                let middle = Placed {
                    eye: entry.via + (seat.eye - seat.hips),
                    hips: entry.via,
                    feet: entry.stand.lerp(seat.feet, 0.5),
                };
                if going_in < 0.5 { outside.lerp(&middle, ease(going_in * 2.0)) } else { middle.lerp(&inside, ease(going_in * 2.0 - 1.0)) }
            }
        }
    }
}

/// A soldier's place in a carrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeatedAt {
    pub carrier: u32,
    pub seat: u8,
}

/// Who sits in each seat of a carrier: plain data, indexed like its list of seats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Occupants(pub Vec<Option<u32>>);

impl Occupants {
    pub fn empty(seats: usize) -> Self {
        Self(vec![None; seats])
    }

    pub fn is_free(&self, seat: usize) -> bool {
        self.0.get(seat).is_some_and(Option::is_none)
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    pub fn of_role(&self, seats: &[SeatConfig], role: SeatRole) -> Option<u32> {
        self.0.iter().zip(seats).find(|(_, s)| s.role == role).and_then(|(who, _)| *who)
    }

    /// Takes a free seat. Says whether it was free.
    pub fn sit(&mut self, seat: usize, who: u32) -> bool {
        if !self.is_free(seat) {
            return false;
        }
        self.0[seat] = Some(who);
        true
    }

    /// Whoever it is leaves whichever seat they are in.
    pub fn leave(&mut self, who: u32) {
        for slot in &mut self.0 {
            if *slot == Some(who) {
                *slot = None;
            }
        }
    }
}

/// A seat's look as a yaw and pitch from the way the seat faces, given a world look. Yaw comes out in
/// the half turn either side of the seat.
pub fn relative_look(frame: &Frame, seat: &SeatConfig, yaw: f32, pitch: f32) -> (f32, f32) {
    let inside = frame.rot.inverse() * look_dir(yaw, pitch);
    let toward = (-inside.x).atan2(-inside.z) - seat.facing;
    (wrap_angle(toward), inside.y.clamp(-1.0, 1.0).asin())
}

fn wrap_angle(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch) * Vec3::NEG_Z
}

/// A world look turned along with whatever carries it. The yaw stays unwrapped, so it never jumps a whole turn.
pub fn carry_look(yaw: f32, pitch: f32, turn: Quat) -> (f32, f32) {
    let carried = turn * look_dir(yaw, pitch);
    (yaw + wrap_angle((-carried.x).atan2(-carried.z) - yaw), carried.y.clamp(-1.0, 1.0).asin())
}

/// A rotation as a tilt of the world and a turn round the vertical, in that order: `rot = tilt * turn`.
pub fn tilt_and_turn(rot: Quat) -> (Quat, Quat) {
    let flat = Vec2::new(rot.y, rot.w);
    let turn = if flat.length() > 1e-4 { Quat::from_xyzw(0.0, rot.y, 0.0, rot.w).normalize() } else { Quat::IDENTITY };
    (rot * turn.inverse(), turn)
}

/// The world look nearest what was asked for that the seat allows. Yaw stays near the one asked for and
/// never jumps a whole turn, because a look is unwrapped.
pub fn limit_look(frame: &Frame, seat: &SeatConfig, yaw: f32, pitch: f32) -> (f32, f32) {
    let (rel_yaw, rel_pitch) = relative_look(frame, seat, yaw, pitch);
    let (clamped_yaw, clamped_pitch) = seat.look.clamp(rel_yaw, rel_pitch);
    if clamped_yaw == rel_yaw && clamped_pitch == rel_pitch {
        return (yaw, pitch);
    }
    let world = frame.rot * look_dir(clamped_yaw + seat.facing, clamped_pitch);
    (yaw + wrap_angle((-world.x).atan2(-world.z) - yaw), world.y.clamp(-1.0, 1.0).asin())
}

/// Can a gun be brought to bear this way from this seat?
pub fn can_fire(frame: &Frame, seat: &SeatConfig, yaw: f32, pitch: f32) -> bool {
    let (rel_yaw, rel_pitch) = relative_look(frame, seat, yaw, pitch);
    seat.fire.is_some_and(|arc| arc.contains(rel_yaw, rel_pitch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_carried_by_a_turn_and_a_tilt_stays_on_the_same_point_inside() {
        let (yaw, pitch) = (0.7, -0.2);
        let from = Quat::from_rotation_y(0.4);
        let to = Quat::from_rotation_x(0.25) * Quat::from_rotation_z(-0.15) * Quat::from_rotation_y(2.9);
        let (carried_yaw, carried_pitch) = carry_look(yaw, pitch, to * from.inverse());
        let inside = from.inverse() * look_dir(yaw, pitch);
        assert!((to.inverse() * look_dir(carried_yaw, carried_pitch)).distance(inside) < 1e-4);
        let (tilt, turn) = tilt_and_turn(to);
        assert!((tilt * turn).angle_between(to) < 1e-3);
        assert!((Frame { pos: Vec3::ZERO, rot: turn }.yaw() - Frame { pos: Vec3::ZERO, rot: to }.yaw()).abs() < 0.2);
    }

    #[test]
    fn limits_go_with_the_seat_when_it_tilts() {
        let cfg = seat();
        let level = Frame { pos: Vec3::ZERO, rot: Quat::IDENTITY };
        let nose_up = Frame { pos: Vec3::ZERO, rot: Quat::from_rotation_x(0.3) };
        assert_eq!(limit_look(&level, &cfg, 0.0, 0.2), (0.0, 0.2), "in the arc, a look is left exactly alone");
        let (_, pitch) = limit_look(&nose_up, &cfg, 0.0, 1.0);
        assert!((pitch - 1.0).abs() < 1e-3, "0.8 above the seat is 1.1 above the world, so 1.0 is allowed: {pitch}");
        let (_, pitch) = limit_look(&nose_up, &cfg, 0.0, 1.3);
        assert!((pitch - 1.1).abs() < 0.01, "{pitch}");
    }

    fn seat() -> SeatConfig {
        SeatConfig {
            role: SeatRole::Passenger,
            eye: Vec3::new(0.5, 1.0, 0.0),
            facing: 0.0,
            swivel: false,
            hips: Vec3::new(0.5, 0.2, 0.0),
            feet: Vec3::new(0.5, -0.2, -0.4),
            hands: None,
            look: Arc { yaw: Vec2::new(-1.5, 1.5), pitch: Vec2::new(-0.6, 0.8) },
            fire: Some(Arc { yaw: Vec2::new(-1.4, -0.4), pitch: Vec2::new(-0.4, 0.5) }),
        }
    }

    fn door() -> Entry {
        Entry {
            name: "door",
            stand: Vec3::new(1.4, -0.8, 0.0),
            on_ground: true,
            hand: Vec3::new(0.9, 0.3, 0.0),
            via: Vec3::new(0.8, 0.2, 0.0),
            seats: vec![0],
            time: 1.0,
            door: None,
        }
    }

    #[test]
    fn going_through_a_door_goes_from_standing_outside_to_sitting_inside_without_a_jump() {
        let standing = Standing { eye: 1.6, hips: 0.9 };
        let (entries, seat) = (vec![door()], seat());
        let seats = [seat];
        let mut transit = Transit::new(TransitKind::In { entry: 0 });
        let start = transit.placed(&seats, &entries, &seat, standing);
        assert!(start.feet.distance(entries[0].stand) < 1e-5);
        assert!((start.eye.y - (entries[0].stand.y + 1.6)).abs() < 1e-5);
        let mut last = start;
        let mut finished = false;
        for _ in 0..62 {
            finished = transit.advance(1.0, 1.0 / 60.0);
            let now = transit.placed(&seats, &entries, &seat, standing);
            assert!(now.eye.distance(last.eye) < 0.08, "the eye jumped: {:?} to {:?}", last.eye, now.eye);
            last = now;
        }
        assert!(finished);
        assert!(last.eye.distance(seat.eye) < 1e-5);
    }

    #[test]
    fn turning_back_ends_where_it_began() {
        let mut transit = Transit::new(TransitKind::In { entry: 0 });
        for _ in 0..20 {
            assert!(!transit.advance(1.0, 1.0 / 60.0));
        }
        transit.back = true;
        let mut ended = false;
        for _ in 0..21 {
            ended |= transit.advance(1.0, 1.0 / 60.0);
        }
        assert!(ended);
        assert_eq!(transit.t, 0.0);
    }

    #[test]
    fn only_linked_seats_have_a_way_between_them() {
        let links = [SeatLink { a: 0, b: 1, time: 1.5 }];
        assert_eq!(link_between(&links, 1, 0), Some(1.5));
        assert_eq!(link_between(&links, 0, 2), None);
    }

    #[test]
    fn a_frame_takes_points_out_and_back() {
        let f = Frame { pos: Vec3::new(3.0, 1.0, -2.0), rot: Quat::from_rotation_y(0.7) };
        let p = Vec3::new(0.4, 0.1, 0.9);
        assert!(f.to_local(f.point(p)).distance(p) < 1e-5);
        assert!((f.yaw() - 0.7).abs() < 1e-5);
    }

    #[test]
    fn a_look_turns_with_the_carrier_and_is_held_inside_the_arc() {
        let f = Frame { rot: Quat::from_rotation_y(1.0), ..Frame::IDENTITY };
        let (yaw, pitch) = limit_look(&f, &seat(), 1.2, 0.1);
        assert!((yaw - 1.2).abs() < 1e-5 && pitch == 0.1, "inside the arc nothing changes");
        let (yaw, pitch) = limit_look(&f, &seat(), 3.0, 1.4);
        assert!((yaw - 2.5).abs() < 1e-4 && (pitch - 0.8).abs() < 1e-4, "{yaw} {pitch}");
    }

    #[test]
    fn a_look_held_inside_the_arc_stays_near_an_unwrapped_yaw() {
        let f = Frame { rot: Quat::from_rotation_y(0.2), ..Frame::IDENTITY };
        let turns = 40.0 * std::f32::consts::TAU;
        let (yaw, _) = limit_look(&f, &seat(), turns + 0.25, 0.0);
        assert!((yaw - (turns + 0.25)).abs() < 1e-3, "{yaw}");
    }

    #[test]
    fn a_gun_only_bears_inside_its_arc() {
        let f = Frame::IDENTITY;
        assert!(can_fire(&f, &seat(), -1.0, 0.0));
        assert!(!can_fire(&f, &seat(), 0.0, 0.0));
        assert!(!can_fire(&f, &seat(), -1.0, 1.0));
    }

    #[test]
    fn people_take_free_seats() {
        let mut who = Occupants::empty(2);
        assert!(who.sit(1, 7));
        assert!(!who.sit(1, 8));
        who.leave(7);
        assert!(who.is_empty());
    }
}
