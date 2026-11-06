//! Capture points and what climbing can do to them, on the real town. Everything here gets up the roof
//! the way a player has to, one soldier onto the other's back and then up.

use super::*;
use glam::Vec2;
use prs_sim::Level;
use prs_sim::movement::{MoveInput, MoveState, Perch, Stance, plan_climb, yaw_basis};
use prs_sim::protocol::{Buttons, PlayerCmd};

const AT_ZONE: &str = "Market";

struct Spot {
    feet: Vec3,
    yaw: f32,
}

/// Somewhere in this zone where only a back gets you up, going by the same asking a player's press does.
fn spot_in_zone(sim: &Sim, zone: usize) -> Spot {
    let c = &sim.move_cfg;
    let point = &sim.level.capture_points[zone];
    for solid in &sim.level.solids {
        let (x, z) = (solid.rotation * Vec3::X, solid.rotation * Vec3::Z);
        let top = solid.center.y + solid.half.y + 0.5;
        for (normal, half, along, along_half) in [(x, solid.half.x, z, solid.half.z), (-x, solid.half.x, z, solid.half.z), (z, solid.half.z, x, solid.half.x), (-z, solid.half.z, x, solid.half.x)] {
            let flat = Vec3::new(normal.x, 0.0, normal.z).normalize_or_zero();
            if flat == Vec3::ZERO {
                continue;
            }
            let steps = (along_half * 2.0).ceil().max(1.0) as i32;
            for i in 0..=steps {
                let side = solid.center + normal * half + along * (i as f32 / steps as f32 * 2.0 - 1.0) * along_half;
                let at = side + flat * (c.radius + 0.25);
                let high = Vec3::new(at.x, top, at.z);
                let Some(hit) = sim.world.raycast(high, Vec3::NEG_Y, top + 200.0) else { continue };
                let feet = high - Vec3::Y * (hit.distance - 0.02);
                let yaw = flat.x.atan2(flat.z);
                let land = sim.world.land_height(feet.x, feet.z).unwrap_or(feet.y);
                if hit.distance <= 0.0 || feet.y - land > 0.3 || !sim.world.capsule_is_clear(feet, c.capsule(Stance::Stand)) {
                    continue;
                }
                let input = MoveInput { yaw, ..MoveInput::default() };
                let mut state = MoveState::new(feet, c);
                state.grounded = true;
                if plan_climb(&state, c, &input, &sim.world).is_some() {
                    continue;
                }
                // A little lower and further back than the very best case, as a real back settles.
                state.pos += Vec3::Y * (c.stool.back_height - 0.08) + flat * 0.05;
                state.perch = Perch { on: Some(1), spot: state.pos };
                let Some(plan) = plan_climb(&state, c, &input, &sim.world) else { continue };
                let inside = Vec2::new(plan.to.x - point.center.x, plan.to.z - point.center.z).length() < point.radius - 2.0;
                if inside && plan.boosted && plan.to.y > land + 2.4 {
                    return Spot { feet, yaw };
                }
            }
        }
    }
    panic!("nothing to climb in {}", point.name);
}

struct Town {
    sim: Sim,
    game: Match,
    zone: usize,
}

impl Town {
    /// A round on the town, with team `team` attacking the Market.
    fn new(mode: Mode) -> (Self, u8) {
        let mut sim = Sim::new(Level::named("town").unwrap());
        let mut game = Match::new(mode, &mut sim);
        let zone = sim.level.capture_points.iter().position(|p| p.name == AT_ZONE).unwrap();
        // Everything up to it already taken, so that it is the next point along.
        let team = 0;
        while let Some(next) = game.state.attack_target(team).filter(|&next| next != zone) {
            game.state.points[next].owner = Some(team);
            game.state.points[next].claimant = Some(team);
        }
        assert_eq!(game.state.attack_target(team), Some(zone));
        (Self { sim, game, zone }, team)
    }

    fn soldier(&mut self, team: u8) -> SoldierId {
        let seed = self.sim.soldiers.len() as u32 + 1;
        let id = self.sim.spawn_soldier(seed);
        self.game.join(&mut self.sim, id, Some(team), false);
        self.sim.step(&[]);
        self.game.step(&mut self.sim, &[(id, ModeCmd::Deploy)]);
        assert!(self.sim.soldiers[id].present);
        id
    }

    fn tick(&mut self, cmds: &[(SoldierId, PlayerCmd)]) {
        self.sim.step(cmds);
        self.game.step(&mut self.sim, &[]);
    }

    fn run(&mut self, seconds: f32) {
        for _ in 0..(seconds / TICK_DT) as u32 {
            self.tick(&[]);
        }
    }

    fn go(&mut self, id: SoldierId, feet: Vec3, yaw: f32) {
        let body = &mut self.sim.soldiers[id].body;
        body.pos = feet;
        body.vel = Vec3::ZERO;
        self.sim.soldiers[id].yaw = yaw;
        self.sim.soldiers[id].anim.hip_yaw = yaw;
    }

    /// Takes one soldier up a roof in the zone, on a mate's back, and sends the mate home. Returns the roof soldier.
    fn up_on_a_roof(&mut self, team: u8) -> SoldierId {
        let spot = spot_in_zone(&self.sim, self.zone);
        let forward = yaw_basis(spot.yaw).0;
        let (rider, mate) = (self.soldier(team), self.soldier(team));
        let back = self.sim.move_cfg.stool.back_offset;
        self.go(mate, spot.feet + forward * back, spot.yaw);
        let crouch = self.sim.move_cfg.crouch_eye;
        self.sim.soldiers[mate].body.stance = Stance::Crouch;
        self.sim.soldiers[mate].body.eye_height = crouch;
        self.go(rider, spot.feet - forward * 0.7, spot.yaw);
        self.run(0.3);

        self.press_up(rider, spot.yaw);
        assert_eq!(self.sim.soldiers[rider].body.perch.on, Some(mate as u32), "on the back");
        self.run(self.sim.move_cfg.climb.cooldown + 0.2);
        self.press_up(rider, spot.yaw);
        let body = &self.sim.soldiers[rider].body;
        assert!(body.boosted, "should have needed the back, at {}", body.pos);
        let land = self.sim.world.land_height(body.pos.x, body.pos.z).unwrap();
        assert!(body.grounded && body.pos.y > land + 2.4, "up on it: {} over land at {land}", body.pos);
        let home = self.sim.soldiers[mate].home;
        self.go(mate, home, 0.0);
        self.run(0.5);
        assert!(self.sim.soldiers[rider].body.grounded && self.sim.soldiers[rider].body.boosted);
        rider
    }

    fn press_up(&mut self, id: SoldierId, yaw: f32) {
        let mut jump = Buttons::default();
        jump.set(Buttons::JUMP, true);
        let cmd = |tick, buttons| PlayerCmd { tick, wish_move: Vec2::Y, yaw, buttons, ..Default::default() };
        let tick = self.sim.tick + 1;
        self.tick(&[(id, cmd(tick, jump))]);
        assert!(self.sim.soldiers[id].body.climb.active(), "did not start, at {}", self.sim.soldiers[id].body.pos);
        while self.sim.soldiers[id].body.climb.active() {
            let tick = self.sim.tick + 1;
            self.tick(&[(id, cmd(tick, Buttons::default()))]);
        }
        let tick = self.sim.tick + 1;
        let idle = PlayerCmd { tick, yaw, ..Default::default() };
        self.tick(&[(id, idle)]);
    }

    fn point(&self) -> PointState {
        self.game.state.points[self.zone]
    }

    /// A soldier stood in the open in the zone, at its flag.
    fn on_the_ground(&mut self, team: u8) -> SoldierId {
        let id = self.soldier(team);
        let center = self.sim.level.capture_points[self.zone].center;
        let top = center + Vec3::Y * 30.0;
        let hit = self.sim.world.raycast(top, Vec3::NEG_Y, 60.0).unwrap();
        self.go(id, top - Vec3::Y * (hit.distance - 0.05), 0.0);
        id
    }
}

#[test]
fn a_roof_only_a_back_gets_you_up_takes_no_point() {
    let (mut town, team) = Town::new(Mode::advance_and_secure());
    let before = town.point();
    town.up_on_a_roof(team);
    town.run(60.0);
    assert_eq!(town.point().inside[team as usize], 0, "should not be counted");
    assert_eq!(town.point().owner, before.owner, "taken from a roof");
    assert_ne!(town.point().claimant, Some(team), "claimed from a roof");
}

/// The griefing this is all for: one enemy lying up there with nobody able to get at them, stalling
/// everyone on the ground for ever.
#[test]
fn nobody_stalls_a_capture_from_a_roof_nobody_can_walk_up_to() {
    let (mut town, team) = Town::new(Mode::advance_and_secure());
    town.up_on_a_roof(1 - team);
    town.on_the_ground(team);
    town.run(60.0);
    assert_eq!(town.point().owner, Some(team), "held off from a roof: {:?}", town.point());
}

/// Coming down counts again, so the roof is a choice and not a way to lose your place in the fight.
#[test]
fn coming_back_down_to_the_land_counts_again() {
    let (mut town, team) = Town::new(Mode::advance_and_secure());
    let id = town.up_on_a_roof(team);
    let center = town.sim.level.capture_points[town.zone].center;
    let top = center + Vec3::Y * 30.0;
    let hit = town.sim.world.raycast(top, Vec3::NEG_Y, 60.0).unwrap();
    town.sim.soldiers[id].body.pos = top - Vec3::Y * (hit.distance - 1.5);
    town.run(1.0);
    assert!(!town.sim.soldiers[id].body.boosted);
    assert_eq!(town.point().inside[team as usize], 1);
}

/// Someone standing on a back in the open is still at street level and in reach, so they count.
#[test]
fn on_a_back_in_the_street_you_still_count() {
    let (mut town, team) = Town::new(Mode::advance_and_secure());
    let spot = spot_in_zone(&town.sim, town.zone);
    let forward = yaw_basis(spot.yaw).0;
    let (rider, mate) = (town.soldier(team), town.soldier(team));
    town.go(mate, spot.feet + forward * town.sim.move_cfg.stool.back_offset, spot.yaw);
    let crouch = town.sim.move_cfg.crouch_eye;
    town.sim.soldiers[mate].body.stance = Stance::Crouch;
    town.sim.soldiers[mate].body.eye_height = crouch;
    town.go(rider, spot.feet - forward * 0.7, spot.yaw);
    town.run(0.3);
    town.press_up(rider, spot.yaw);
    assert_eq!(town.sim.soldiers[rider].body.perch.on, Some(mate as u32));
    town.run(0.2);
    assert_eq!(town.point().inside[team as usize], 2, "both of them");
}

/// What a mod gets by turning it back on, and the answer as things stood before this rule: from up there
/// one soldier takes a point alone, and one on the other side stalls it for as long as they like.
#[test]
fn the_rule_is_data_and_without_it_a_roof_takes_or_stalls_a_point() {
    let open = || {
        let mut mode = Mode::advance_and_secure();
        mode.objective = Box::new(AdvanceAndSecure(CaptureRules { boosted_soldiers_count: true, ..CaptureRules::default() }));
        mode
    };
    let (mut town, team) = Town::new(open());
    town.up_on_a_roof(team);
    town.run(60.0);
    assert_eq!(town.point().owner, Some(team), "alone from a roof");

    let (mut town, team) = Town::new(open());
    town.up_on_a_roof(1 - team);
    town.on_the_ground(team);
    town.run(60.0);
    assert_ne!(town.point().owner, Some(team), "stalled from a roof");
}
