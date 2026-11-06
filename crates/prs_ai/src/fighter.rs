//! One soldier in a fight. What it knows comes from looking, with a ray, and from hearing the shots
//! everyone else hears. What it does goes out as a [`PlayerCmd`], so it aims the real gun by turning the
//! view: recoil, sway and wander all act on that afterwards, and all it can do about them is what a
//! person does, which is notice a moment late and correct for some of it.

use glam::{Vec2, Vec3};
use prs_sim::health::Life;
use prs_sim::inventory::Slot;
use prs_sim::movement::{Stance, yaw_basis};
use prs_sim::protocol::{Buttons, PlayerCmd, TICK_DT};
use prs_sim::weapons::FireMode;
use prs_sim::throwables::ThrownKind;
use prs_sim::{Sim, SimEvent, Soldier, SoldierId, view_dir};
use std::f32::consts::TAU;

use crate::flow::{FlowField, FlowGrid};
use crate::sight::{BODY_POINTS, Foliage, GUN_POINT, Watch, body_points};
use crate::skill::FightConfig;
use crate::walker::{Dice, Seen, Surroundings, WalkArea, Walker, wrap, yaw_of};
use crate::{Holdover, WalkerConfig};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Plan {
    /// Getting on with the objective, at nobody in particular.
    Move,
    /// Someone in sight: stop and shoot.
    Fight,
    /// Firing at where someone was, to keep their head down.
    Suppress,
    /// Getting to a spot that hides them from whoever is shooting.
    Seek { spot: Vec2 },
    /// Behind something, with their head down.
    Hide,
    /// Looking out from behind it, or looking towards a noise.
    Peek,
    /// Going round the side of someone the squad has seen.
    Flank { spot: Vec2 },
    /// On the way to someone down, or getting them up.
    Revive { patient: SoldierId },
    /// Bandaging themselves.
    Treat,
    /// Getting something thrown: where to point, and how many they had thrown when they started.
    Throw { kind: ThrownKind, aim: Vec2, thrown_before: u32 },
}

/// Someone they know about, and what they know, which is only ever as good as the last look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub soldier: SoldierId,
    /// The bit of them that was seen, or where the noise seemed to come from.
    pub at: Vec3,
    pub velocity: Vec3,
    pub seen_tick: u32,
    /// In sight as of the last look.
    pub visible: bool,
    /// When they will have taken it in and can act.
    pub ready_tick: u32,
}

/// What a squad tells each other.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SquadBoard {
    pub squad: u32,
    pub contact: Option<Contact>,
    /// Which way round this squad goes, 1 or -1.
    pub flank_side: f32,
    /// The middle of whoever is keeping together, leaving out anyone on their way from a spawn.
    pub center: Option<Vec2>,
    /// The tick anyone in the squad last started throwing something, so that they do not all do it at once.
    pub last_throw: u32,
}

/// What a fighter works with on one tick.
pub struct Battle<'a> {
    pub sim: &'a Sim,
    pub cfg: &'a FightConfig,
    pub walk: &'a WalkerConfig,
    pub holdover: &'a Holdover,
    pub foliage: &'a Foliage,
    /// The tick each soldier last fired on.
    pub fired: &'a [u32],
    /// Where they have been told to be.
    pub area: &'a WalkArea,
    pub feet: &'a [Vec2],
    pub way: Option<(&'a FlowGrid, &'a FlowField)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fighter {
    pub soldier: SoldierId,
    pub squad: u32,
    /// Holds the ground while others go round.
    pub holds: bool,
    pub nav: Walker,
    pub dice: Dice,
    pub plan: Plan,
    /// Seconds left of the plan, for those that have a limit.
    pub plan_left: f32,
    pub contact: Option<Contact>,
    /// How shot at they feel: near misses and hits add to it, and it drains away.
    pub pressure: f32,
    /// Where the view is pointing, as yaw and pitch.
    pub aim: Vec2,
    /// What is off about their idea of where to aim, how far it is heading, and until when.
    pub noise: Vec2,
    pub noise_goal: Vec2,
    pub noise_left: f32,
    /// Their front sight off the view as they have noticed it so far.
    pub seen_gun: Vec2,
    /// The range they have settled on for elevation, as they judged it.
    pub judged_range: f32,
    /// Rounds still to fire of this burst, what the magazine held last tick and when the next may start.
    pub burst_left: u32,
    pub last_ammo: u32,
    pub pause_left: f32,
    /// How long they have been waiting on the sights.
    pub waited: f32,
    pub pulled: bool,
    pub friend_in_line: bool,
    pub crouch: bool,
    pub lean: i8,
    pub cover_tries: u32,
    /// Looked for cover and there is none, so a magazine gets changed where they stand.
    pub no_cover: bool,
    /// Seconds until a noise can interrupt them again.
    pub alert_rest: f32,
    /// Who to try next in looking around.
    pub sweep: usize,
    /// How aware they are of whoever they have had a look at lately.
    pub watches: Vec<Watch>,
    pub lives: u32,
    pub rays_spent: u32,
    /// The squad's middle as of the last look.
    pub squad_center: Option<Vec2>,
    /// Seconds until they will think of throwing anything again.
    pub throw_rest: f32,
    /// Seconds spent waiting for them. Past the limit it goes on regardless, and starts over once well clear.
    pub waited_for_squad: f32,
}

impl Fighter {
    pub(crate) fn new(soldier: SoldierId, squad: u32, holds: bool, nav: Walker, dice: Dice, s: &Soldier) -> Self {
        Self {
            soldier,
            squad,
            holds,
            nav,
            dice,
            plan: Plan::Move,
            plan_left: 0.0,
            contact: None,
            pressure: 0.0,
            aim: Vec2::new(s.yaw, s.pitch),
            noise: Vec2::ZERO,
            noise_goal: Vec2::ZERO,
            noise_left: 0.0,
            seen_gun: Vec2::ZERO,
            judged_range: 100.0,
            burst_left: 0,
            last_ammo: s.weapon.ammo,
            pause_left: 0.0,
            waited: 0.0,
            pulled: false,
            friend_in_line: false,
            crouch: false,
            lean: 0,
            cover_tries: 0,
            no_cover: false,
            alert_rest: 0.0,
            sweep: soldier,
            watches: Vec::new(),
            lives: s.lives,
            rays_spent: 0,
            squad_center: None,
            waited_for_squad: 0.0,
            throw_rest: 0.0,
        }
    }

    /// Back from the dead with nothing remembered.
    fn start_over(&mut self, s: &Soldier) {
        self.plan = Plan::Move;
        self.contact = None;
        self.watches.clear();
        self.pressure = 0.0;
        self.aim = Vec2::new(s.yaw, s.pitch);
        self.seen_gun = Vec2::ZERO;
        self.burst_left = 0;
        self.last_ammo = s.weapon.ammo;
        self.crouch = false;
        self.lean = 0;
        self.lives = s.lives;
        self.nav.goal_left = 0.0;
        self.nav.route.clear();
    }

    /// What they make of a tick's gunfire. Everyone hears a shot, and knows when it came close.
    pub(crate) fn listen(&mut self, x: &Battle) {
        let sim = x.sim;
        let me = &sim.soldiers[self.soldier];
        let skill = &x.cfg.skill;
        let ear = me.eye();
        for event in &sim.events {
            match *event {
                SimEvent::ShotFired { soldier, origin, dir, .. } if soldier != self.soldier && sim.soldiers[soldier].team != me.team => {
                    let along = (ear - origin).dot(dir);
                    let past = (ear - origin - dir * along).length();
                    if along > 0.0 && past < skill.crack_radius {
                        self.pressure += skill.crack_pressure * (1.0 - past / skill.crack_radius);
                    }
                    let away = ear.distance(origin);
                    if away < skill.hear_range && !self.contact.is_some_and(|c| c.visible) {
                        let off = self.dice.in_circle() * away * skill.hear_error;
                        let at = origin + Vec3::new(off.x, 0.0, off.y);
                        let ready_tick = sim.tick;
                        self.contact = Some(Contact { soldier, at, velocity: Vec3::ZERO, seen_tick: sim.tick, visible: false, ready_tick });
                    }
                }
                SimEvent::SoldierHit { soldier, shooter, .. } if soldier == self.soldier => {
                    self.pressure += skill.hit_pressure;
                    if sim.soldiers[shooter].team != me.team && !self.contact.is_some_and(|c| c.visible) {
                        let at = sim.soldiers[shooter].eye();
                        let ready_tick = sim.tick;
                        self.contact = Some(Contact { soldier: shooter, at, velocity: Vec3::ZERO, seen_tick: sim.tick, visible: false, ready_tick });
                    }
                }
                _ => {}
            }
        }
    }

    /// The command for the coming tick.
    pub(crate) fn step(&mut self, x: &Battle, board: &mut SquadBoard) -> PlayerCmd {
        let sim = x.sim;
        let me = &sim.soldiers[self.soldier];
        let tick = sim.tick;
        if me.lives != self.lives {
            self.start_over(me);
        }
        if !me.health.is_up() {
            self.nav.lost = true;
            self.burst_left = 0;
            return PlayerCmd { tick, yaw: self.aim.x, pitch: self.aim.y, ..Default::default() };
        }
        let every = x.cfg.think_ticks.max(1);
        if tick % every == self.nav.beat % every {
            self.think(x, board, every as f32 * TICK_DT);
        }
        self.command(x)
    }

    // ---- What they know.

    fn think(&mut self, x: &Battle, board: &mut SquadBoard, dt: f32) {
        let skill = &x.cfg.skill;
        self.pressure *= (-dt / skill.calm_time.max(1e-3)).exp();
        self.feel_suppression(x);
        self.squad_center = board.center;
        self.plan_left -= dt;
        self.throw_rest -= dt;
        self.alert_rest -= dt;
        self.look(x, board);
        self.choose_plan(x, board);
        self.get_on_with_it(x, board, dt);
    }

    /// Bullets going by have left their mark on the soldier, however this bot came to miss them, and the
    /// sim says whose they were. Nobody needs a good look at someone to know to shoot back that way.
    fn feel_suppression(&mut self, x: &Battle) {
        let skill = &x.cfg.skill;
        let me = &x.sim.soldiers[self.soldier];
        let felt = &me.suppression;
        self.pressure = self.pressure.max(felt.level * skill.suppression_weight);
        if let Some(shooter) = felt.last_shooter.filter(|_| felt.level > 0.0 && self.contact.is_none())
            && let Some(them) = x.sim.soldiers.get(shooter).filter(|s| s.team != me.team)
        {
            let off = self.dice.in_circle() * them.eye().distance(me.eye()) * skill.hear_error;
            let at = them.eye() + Vec3::new(off.x, 0.0, off.y);
            self.contact = Some(Contact { soldier: shooter, at, velocity: Vec3::ZERO, seen_tick: x.sim.tick, visible: false, ready_tick: x.sim.tick });
        }
    }

    /// How much of a look from here gets to a point: nothing through a wall or smoke, and less through leaves.
    /// All the sight in the game is this.
    fn sees(&mut self, x: &Battle, from: Vec3, to: Vec3) -> f32 {
        self.rays_spent += 1;
        let path = to - from;
        let length = path.length();
        if length < 0.01 {
            return 1.0;
        }
        if x.sim.smoke_blocks(from, to) || x.sim.world.raycast(from, path / length, length).is_some() {
            return 0.0;
        }
        (-x.cfg.sight.leaf_density * x.foliage.leaves_between(from, to)).exp()
    }

    /// Whether enough of a look gets through to see by.
    fn clear(&mut self, x: &Battle, from: Vec3, to: Vec3) -> bool {
        self.sees(x, from, to) >= x.cfg.sight.see_through
    }

    /// Looks again at a few of someone's points, by turns, and says how much of them shows and the best
    /// showing point to aim at, if any. Builds or fades how aware of them this soldier is, by how much shows,
    /// how far, how they stand and move, whether they are firing and whether they are in the shade indoors.
    fn glimpse(&mut self, x: &Battle, eye: Vec3, soldier: SoldierId) -> (f32, Option<Vec3>) {
        let sim = x.sim;
        let (sight, skill) = (&x.cfg.sight, &x.cfg.skill);
        let them = &sim.soldiers[soldier];
        let points = body_points(sim, sight, them);
        let tick = sim.tick;
        let index = match self.watches.iter().position(|w| w.soldier == soldier) {
            Some(index) => index,
            None => {
                if self.watches.len() >= sight.most_watched.max(1) {
                    // Whoever they are least aware of makes room.
                    let least = (0..self.watches.len()).min_by(|&a, &b| self.watches[a].awareness.total_cmp(&self.watches[b].awareness)).expect("not empty");
                    self.watches.swap_remove(least);
                }
                self.watches.push(Watch::new(soldier, tick));
                self.watches.len() - 1
            }
        };
        let mut watch = self.watches[index];
        for _ in 0..sight.rays_per_look.max(1) {
            let point = watch.next as usize % BODY_POINTS;
            watch.next = ((point + 1) % BODY_POINTS) as u8;
            let bit = 1u8 << point;
            watch.looked |= bit;
            if self.sees(x, eye, points[point]) >= sight.see_through { watch.shown |= bit } else { watch.shown &= !bit }
        }
        let distance = eye.distance(them.eye());
        let fired = x.fired.get(soldier).is_some_and(|&at| at > 0 && tick.saturating_sub(at) as f32 * TICK_DT <= sight.fired_memory);
        let moving = them.body.horizontal_speed() > 0.5;
        let down = them.body.stance != Stance::Stand;
        let mut share = watch.body_share();
        if fired && watch.any_shown() {
            share = share.max(skill.firing_share);
        }
        let hidden = (distance > skill.quiet_range && !fired) || (distance > skill.still_hidden_beyond && down && !moving && !fired);
        let indoors = |at: Vec3| sim.level.rooms.iter().position(|room| room.contains(at));
        let in_the_light = sim.level.windows.iter().any(|w| w.at.distance(them.eye()) < sight.window_light);
        let shaded = !fired && !in_the_light && indoors(them.eye()).is_some_and(|room| indoors(eye) != Some(room));
        let shape = match them.body.stance {
            Stance::Stand => 1.0,
            Stance::Crouch => skill.spot_crouched,
            Stance::Prone => skill.spot_prone,
        } * if moving { 1.0 } else { skill.spot_still };
        let rate = skill.notice_rate
            * share.powf(skill.visible_power)
            * (skill.spot_range / distance.max(1.0)).powi(2).min(1.0)
            * shape
            * if fired { skill.firing_boost } else { 1.0 }
            * if shaded { sight.room_shade } else { 1.0 };
        let gap = (tick.saturating_sub(watch.last_tick) as f32 * TICK_DT).min(sight.longest_gap);
        watch.awareness = if share > 0.0 && !hidden {
            (watch.awareness + rate * gap).min(1.0)
        } else {
            (watch.awareness - skill.forget_rate * gap).max(0.0)
        };
        watch.last_tick = tick;
        self.watches[index] = watch;
        // Somewhere on what shows of them, not the gun, and a different one look by look, as a person's eye
        // goes from one part of someone to another.
        let shown: Vec<usize> = (0..GUN_POINT).filter(|&p| watch.shown & (1 << p) != 0).collect();
        let look = (tick / x.cfg.think_ticks.max(1)) as usize;
        let best = (!shown.is_empty()).then(|| points[shown[look % shown.len()]]);
        (share, best)
    }

    fn awareness_of(&self, soldier: SoldierId) -> f32 {
        self.watches.iter().find(|w| w.soldier == soldier).map_or(0.0, |w| w.awareness)
    }

    /// Looks again at whoever it is watching and at one more in turn. A few rays each.
    fn look(&mut self, x: &Battle, board: &mut SquadBoard) {
        let sim = x.sim;
        let skill = &x.cfg.skill;
        let me = &sim.soldiers[self.soldier];
        let eye = me.eye();
        let forward = flat_forward(self.aim.x);
        let tick = sim.tick;

        if let Some(contact) = self.contact.filter(|c| c.soldier < sim.soldiers.len()) {
            let them = &sim.soldiers[contact.soldier];
            if them.present && them.health.is_up() && them.eye().distance(eye) < skill.view_range && contact.visible {
                let (share, best) = self.glimpse(x, eye, contact.soldier);
                // Kept in sight only while enough of them shows. Otherwise they are where they were last seen.
                match best.filter(|_| share >= skill.track_share) {
                    Some(at) => self.contact = Some(Contact { at, velocity: them.body.vel, seen_tick: tick, ..contact }),
                    None => self.contact = Some(Contact { visible: false, ..contact }),
                }
            } else if !them.health.is_up() {
                self.contact = None;
            }
        }

        // Someone else, in turn, if they are looking that way.
        let count = sim.soldiers.len();
        for offset in 0..count {
            let i = (self.sweep + offset) % count;
            let them = &sim.soldiers[i];
            let tracked = self.contact.is_some_and(|c| c.soldier == i && c.visible);
            if i == self.soldier || tracked || !them.present || them.team == me.team || !them.health.is_up() {
                continue;
            }
            let to = them.eye() - eye;
            let distance = to.length();
            if distance > skill.view_range {
                continue;
            }
            let flat = Vec2::new(to.x, to.z).normalize_or_zero();
            if distance > skill.notice_close && flat.dot(forward) < skill.view_cos {
                continue;
            }
            self.sweep = i + 1;
            let (share, best) = self.glimpse(x, eye, i);
            if self.awareness_of(i) >= 1.0
                && share >= skill.track_share
                && let Some(at) = best
            {
                let nearer = self.contact.is_none_or(|c| !c.visible || c.at.distance(eye) > distance);
                if nearer {
                    let ready_tick = tick + self.reaction(x);
                    self.contact = Some(Contact { soldier: i, at, velocity: them.body.vel, seen_tick: tick, visible: true, ready_tick });
                }
            }
            break;
        }
        // Whoever has not been looked at for a while is forgotten.
        let forget = (x.cfg.forget_after / TICK_DT) as u32;
        self.watches.retain(|w| tick.saturating_sub(w.last_tick) <= forget);

        if self.contact.is_some_and(|c| !c.visible && tick.saturating_sub(c.seen_tick) > forget) {
            self.contact = None;
        }
        // What one of a squad has seen they all hear about, a look later.
        if self.contact.is_some_and(|c| c.visible) {
            board.contact = self.contact;
        } else if self.contact.is_none()
            && let Some(told) = board.contact.filter(|c| tick.saturating_sub(c.seen_tick) <= forget)
        {
            self.contact = Some(Contact { visible: false, ..told });
        }
    }

    fn reaction(&mut self, x: &Battle) -> u32 {
        (self.dice.between(x.cfg.skill.reaction) / TICK_DT) as u32
    }

    // ---- What they mean to do about it.

    fn choose_plan(&mut self, x: &Battle, board: &mut SquadBoard) {
        let cfg = x.cfg;
        let skill = &cfg.skill;
        let sim = x.sim;
        let me = &sim.soldiers[self.soldier];
        let eye = me.eye();
        let visible = self.contact.filter(|c| c.visible);
        let weapon = &me.weapon;
        let held = me.held_config(&sim.weapons);
        let out_of_rounds = weapon.ammo == 0;

        // Being shot at comes before anything else.
        let shaken = self.pressure >= skill.cover_at;
        if let Plan::Throw { thrown_before, .. } = self.plan {
            // Either it has gone, or it has taken too long, or there is something more pressing.
            let carries_on = self.pressure < skill.cover_at * cfg.flank_nerve;
            if me.throw.thrown == thrown_before && self.plan_left > 0.0 && carries_on {
                return;
            }
            self.plan = Plan::Move;
        }
        // Down behind something, with someone just the other side of it.
        if self.plan == Plan::Hide && self.pressure < skill.pinned_at && self.try_frag(x, board) {
            return;
        }
        // A medic behind cover comes out for someone down, once the shooting has eased.
        if self.plan == Plan::Hide && self.pressure < cfg.calm_enough {
            self.go_to_someone_down(x);
        }
        let hiding = matches!(self.plan, Plan::Seek { .. } | Plan::Hide);
        if hiding {
            if matches!(self.plan, Plan::Hide) && self.pressure < skill.pinned_at && self.plan_left <= 0.0 {
                if self.contact.is_some() {
                    self.start_peek(x);
                } else {
                    self.plan = Plan::Move;
                }
            }
            return;
        }
        let far_enough = visible.is_none_or(|c| c.at.distance(eye) > skill.hip_range * 2.0);
        if !out_of_rounds {
            self.no_cover = false;
        }
        // Someone going round keeps going through more than would stop them otherwise: that is the point of it.
        let pressing_on = matches!(self.plan, Plan::Flank { .. }) && self.plan_left > 0.0 && self.pressure < skill.cover_at * cfg.flank_nerve;
        if ((shaken && !pressing_on) || (out_of_rounds && far_enough && !self.no_cover)) && self.contact.is_some() {
            self.cover_tries = 0;
            self.plan = Plan::Seek { spot: Vec2::new(me.body.pos.x, me.body.pos.z) };
            self.burst_left = 0;
            self.lean = 0;
            return;
        }

        // Tending to someone is only for when it has gone quiet enough.
        let calm = self.pressure < cfg.calm_enough && visible.is_none();
        let medic_calm = self.pressure < cfg.calm_enough && visible.is_none_or(|c| c.at.distance(eye) > cfg.medic_ignores_beyond);
        if let Plan::Revive { patient } = self.plan {
            let them = &sim.soldiers[patient];
            let committed = them.body.pos.distance(me.body.pos) < cfg.medic_committed && self.pressure < cfg.calm_enough;
            let done = them.health.life != Life::Down || !(medic_calm || committed);
            if !done {
                return;
            }
            self.plan = Plan::Move;
        }
        if let Plan::Treat = self.plan {
            if me.health.wants_bandage() && calm && me.inventory.stash.bandages > 0 {
                return;
            }
            self.plan = Plan::Move;
        }
        if medic_calm && self.smoke_for_someone_down(x, board) {
            return;
        }
        if medic_calm && self.go_to_someone_down(x) {
            return;
        }
        if calm && me.health.bleeding() && me.inventory.stash.bandages > 0 {
            self.plan = Plan::Treat;
            return;
        }

        if matches!(self.plan, Plan::Fight | Plan::Suppress | Plan::Peek | Plan::Flank { .. }) && self.try_frag(x, board) {
            return;
        }
        if let Some(seen) = visible {
            // A flank well under way does not stop for someone far off.
            let range = seen.at.distance(eye);
            if matches!(self.plan, Plan::Flank { .. }) && range > cfg.flank_commit && self.plan_left > 0.0 {
                return;
            }
            if self.plan != Plan::Fight {
                self.plan = Plan::Fight;
                self.settle_to_shoot(x, seen);
            }
            return;
        }

        let Some(known) = self.contact else {
            if !matches!(self.plan, Plan::Flank { .. }) || self.plan_left <= 0.0 {
                self.plan = Plan::Move;
            }
            return;
        };
        let age = sim.tick.saturating_sub(known.seen_tick) as f32 * TICK_DT;
        // Lost sight of them. Those holding keep firing at where they were, and the rest go round.
        if matches!(self.plan, Plan::Fight | Plan::Suppress) && age < cfg.suppress_memory && self.holds {
            let share = weapon.ammo as f32 / held.magazine.max(1) as f32;
            if share > 1.0 - cfg.suppress_share {
                self.plan = Plan::Suppress;
                return;
            }
        }
        if matches!(self.plan, Plan::Flank { .. }) && self.plan_left > 0.0 {
            return;
        }
        if matches!(self.plan, Plan::Peek) && self.plan_left > 0.0 {
            return;
        }
        // A medic is of more use where the rest of them are.
        let goes_round = !self.holds && me.inventory.stash.medkits == 0;
        if goes_round && known.at.distance(eye) > cfg.flank_reach.x && self.alert_rest <= 0.0 {
            // Crossing open ground towards them is what smoke is for.
            if self.smoke_ahead_of_me(x, board, known) {
                return;
            }
            self.alert_rest = cfg.flank_time;
            self.plan = Plan::Flank { spot: self.flank_spot(x, board, known) };
            self.plan_left = cfg.flank_time;
            self.crouch = false;
            self.lean = 0;
            return;
        }
        if self.alert_rest <= 0.0 {
            self.alert_rest = cfg.alert_rest;
            self.plan = Plan::Peek;
            self.plan_left = self.dice.between(cfg.alert_time);
            return;
        }
        self.plan = Plan::Move;
        self.lean = 0;
    }

    /// Whether they can throw anything at all just now, and whether it comes up on the dice.
    fn may_throw(&mut self, x: &Battle, board: &SquadBoard, chance: f32) -> bool {
        let rest = (x.cfg.squad_throw_rest / TICK_DT) as u32;
        let squad_ready = board.last_throw == 0 || x.sim.tick.saturating_sub(board.last_throw) >= rest;
        self.throw_rest <= 0.0 && squad_ready && self.dice.between(Vec2::new(0.0, 1.0)) < chance
    }

    /// Starts throwing at a spot on the ground, if the way there is open. How hard and how high it goes is
    /// worked out from the arc, then spoilt a little, because nobody's throw is perfect.
    fn start_throw(&mut self, x: &Battle, board: &mut SquadBoard, kind: ThrownKind, at: Vec3) -> bool {
        let cfg = x.cfg;
        let throw = &x.sim.throw_cfg;
        let me = &x.sim.soldiers[self.soldier];
        let eye = me.eye();
        let wrong = self.dice.in_circle() * eye.distance(at) * cfg.throw_error;
        let at = at + Vec3::new(wrong.x, 0.0, wrong.y);
        let flat = Vec2::new(at.x - eye.x, at.z - eye.z);
        let range = flat.length().max(1.0);
        // Half the usual rule for the angle of a throw, taking the low arc, with what the hand adds off it.
        let speed = throw.overarm_speed;
        let arc = 0.5 * (throw.gravity * range / (speed * speed)).min(1.0).asin();
        let climb = ((at.y - eye.y) / range).atan();
        let pitch = (arc + climb - throw.overarm_lift + self.dice.between(Vec2::new(-1.0, 1.0)) * cfg.throw_angle_error).clamp(-1.0, 1.2);
        let yaw = self.aim.x + wrap(yaw_of(flat) - self.aim.x);
        // Into the wall in front of them is no throw at all.
        let dir = view_dir(yaw, pitch + throw.overarm_lift);
        self.rays_spent += 1;
        if x.sim.world.raycast(eye, dir, cfg.throw_room).is_some() {
            return false;
        }
        self.plan = Plan::Throw { kind, aim: Vec2::new(yaw, pitch), thrown_before: me.throw.thrown };
        self.plan_left = cfg.throw_time;
        self.throw_rest = cfg.throw_rest;
        board.last_throw = x.sim.tick;
        self.burst_left = 0;
        self.lean = 0;
        true
    }

    /// A frag for someone just behind cover: seen a moment ago, close enough to reach, with no friends near them.
    fn try_frag(&mut self, x: &Battle, board: &mut SquadBoard) -> bool {
        let cfg = x.cfg;
        let me = &x.sim.soldiers[self.soldier];
        // Someone hard to hit is worth one too: in sight but not moving, so down behind something or in a window.
        let settled = |c: &Contact| !c.visible || c.velocity.length() < cfg.frag_still;
        let Some(known) = self.contact.filter(settled) else { return false };
        let age = x.sim.tick.saturating_sub(known.seen_tick) as f32 * TICK_DT;
        let range = known.at.distance(me.eye());
        if me.inventory.stash.frags == 0 || age > cfg.frag_memory || range < cfg.frag_range.x || range > cfg.frag_range.y {
            return false;
        }
        let friend_near = x.sim.soldiers.iter().any(|s| s.present && s.team == me.team && s.body.pos.distance(known.at) < cfg.frag_friends);
        if friend_near || !self.may_throw(x, board, cfg.frag_chance) {
            return false;
        }
        self.start_throw(x, board, ThrownKind::Frag, known.at - Vec3::Y)
    }

    /// Smoke between someone down and whoever put them there, before going out to them.
    fn smoke_for_someone_down(&mut self, x: &Battle, board: &mut SquadBoard) -> bool {
        let cfg = x.cfg;
        let me = &x.sim.soldiers[self.soldier];
        let Some(known) = self.contact else { return false };
        if me.inventory.stash.smokes == 0 || me.inventory.stash.medkits == 0 {
            return false;
        }
        let near = x.sim.soldiers.iter().filter(|s| s.present && s.team == me.team && s.health.life == Life::Down).map(|s| s.body.pos).min_by(|a, b| a.distance(me.body.pos).total_cmp(&b.distance(me.body.pos)));
        let Some(patient) = near else { return false };
        let away = patient.distance(me.body.pos);
        if away < cfg.medic_close * 3.0 || away > cfg.smoke_reach || !self.may_throw(x, board, cfg.smoke_chance) {
            return false;
        }
        let toward = (known.at - patient).normalize_or_zero();
        self.start_throw(x, board, ThrownKind::Smoke, patient + toward * cfg.smoke_ahead * 0.5)
    }

    /// Smoke part of the way to them, to go round behind.
    fn smoke_ahead_of_me(&mut self, x: &Battle, board: &mut SquadBoard, known: Contact) -> bool {
        let cfg = x.cfg;
        let me = &x.sim.soldiers[self.soldier];
        if me.inventory.stash.smokes == 0 || !self.may_throw(x, board, cfg.smoke_chance) {
            return false;
        }
        let toward = (known.at - me.body.pos).normalize_or_zero();
        self.start_throw(x, board, ThrownKind::Smoke, me.body.pos + toward * cfg.smoke_ahead)
    }

    /// A medic picks the nearest friend who is down, if there is one worth the run.
    fn go_to_someone_down(&mut self, x: &Battle) -> bool {
        let me = &x.sim.soldiers[self.soldier];
        if me.inventory.stash.medkits == 0 {
            return false;
        }
        let near = x
            .sim
            .soldiers
            .iter()
            .enumerate()
            .filter(|(_, s)| s.present && s.team == me.team && s.health.life == Life::Down)
            .map(|(i, s)| (i, s.body.pos.distance(me.body.pos)))
            .filter(|(_, d)| *d < x.cfg.medic_reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((patient, _)) = near {
            self.plan = Plan::Revive { patient };
            self.crouch = false;
            self.lean = 0;
        }
        near.is_some()
    }

    /// Off to one side of the enemy and part of the way in, far enough round to see them from a new angle.
    fn flank_spot(&self, x: &Battle, board: &SquadBoard, known: Contact) -> Vec2 {
        let me = x.sim.soldiers[self.soldier].body.pos;
        let (at, them) = (Vec2::new(me.x, me.z), Vec2::new(known.at.x, known.at.z));
        let to_me = (at - them).normalize_or(Vec2::X);
        let range = at.distance(them);
        let out = (range * x.cfg.flank_share).clamp(x.cfg.flank_reach.x, x.cfg.flank_reach.y);
        them + to_me.perp() * board.flank_side * out + to_me * out * 0.4
    }

    /// Picks a stance for shooting from: low if there is a line from down there, and it is far enough.
    fn settle_to_shoot(&mut self, x: &Battle, seen: Contact) {
        let me = &x.sim.soldiers[self.soldier];
        self.burst_left = 0;
        self.waited = 0.0;
        let range = seen.at.distance(me.eye());
        self.judged_range = range * (1.0 + self.dice.between(Vec2::new(-1.0, 1.0)) * x.cfg.skill.range_error);
        if self.lean == 0 && range > x.cfg.skill.crouch_beyond {
            let low = me.body.pos + Vec3::Y * x.cfg.cover_eye;
            self.crouch = self.clear(x, low, seen.at);
        }
    }

    /// Looks for the way out from behind whatever this is: to the side, or up over it.
    fn start_peek(&mut self, x: &Battle) {
        let Some(known) = self.contact else { return };
        let me = &x.sim.soldiers[self.soldier];
        let cfg = &x.sim.move_cfg;
        self.plan = Plan::Peek;
        self.plan_left = self.dice.between(x.cfg.peek_time);
        let from = me.body.pos + Vec3::Y * me.body.eye_height;
        let right = yaw_basis(self.aim.x).1;
        let order = if self.dice.between(Vec2::new(0.0, 1.0)) < 0.5 { [1i8, -1] } else { [-1i8, 1] };
        for side in order {
            let eye = from + right * f32::from(side) * cfg.lean.eye_side_crouch;
            if self.clear(x, eye, known.at) {
                self.lean = side;
                return;
            }
        }
        // No way round, so over the top if there is a line from standing.
        self.lean = 0;
        self.crouch = !self.clear(x, me.body.pos + Vec3::Y * cfg.stand_eye, known.at);
    }

    // ---- Getting there.

    fn surroundings<'a>(x: &Battle<'a>, area: &'a WalkArea) -> Surroundings<'a> {
        Surroundings { world: &x.sim.world, walkable_cos: x.sim.move_cfg.max_slope_cos, area, others: x.feet, way: x.way }
    }

    fn seen(me: &Soldier) -> Seen {
        Seen { feet: me.body.pos, up: me.health.is_up(), crouched: me.body.stance == Stance::Crouch, lean_side: me.body.lean.side }
    }

    fn get_on_with_it(&mut self, x: &Battle, board: &mut SquadBoard, dt: f32) {
        let me = &x.sim.soldiers[self.soldier];
        let at = Vec2::new(me.body.pos.x, me.body.pos.z);
        let around = Self::surroundings(x, x.area);
        let seen = Self::seen(me);
        match self.plan {
            Plan::Seek { spot } => {
                if at.distance(spot) < 1.0 {
                    self.find_cover(x);
                }
                if let Plan::Seek { spot } = self.plan {
                    self.nav.steer_to(x.walk, &seen, &around, at, spot, dt);
                }
            }
            Plan::Flank { spot } => {
                if at.distance(spot) < x.walk.goal_reach * 2.0 {
                    self.plan_left = 0.0;
                    // Round there and nothing seen: next time go the other way.
                    board.flank_side = -board.flank_side;
                } else {
                    self.nav.steer_to(x.walk, &seen, &around, at, spot, dt);
                }
            }
            Plan::Revive { patient } => {
                let to = x.sim.soldiers[patient].body.pos;
                let there = Vec2::new(to.x, to.z);
                if at.distance(there) > x.cfg.medic_close {
                    self.nav.steer_to(x.walk, &seen, &around, at, there, dt);
                }
            }
            _ => {}
        }
        // Who is in the line of fire, as near as it is worth working out.
        self.friend_in_line = false;
        if let Some(contact) = self.contact {
            let from = me.eye();
            let to = contact.at - from;
            let length = to.length().max(0.01);
            let dir = to / length;
            self.friend_in_line = x.sim.soldiers.iter().enumerate().any(|(i, s)| {
                if i == self.soldier || !s.present || s.team != me.team || s.health.is_dead() {
                    return false;
                }
                let middle = s.body.pos + Vec3::Y;
                let along = (middle - from).dot(dir);
                along > 0.0 && along < length && (middle - from - dir * along).length() < x.cfg.friend_clearance
            });
        }
    }

    /// Tries a few spots around them for one the enemy cannot see. Failing that they get low where they are.
    fn find_cover(&mut self, x: &Battle) {
        let cfg = x.cfg;
        let me = &x.sim.soldiers[self.soldier];
        let Some(known) = self.contact else {
            self.plan = Plan::Move;
            return;
        };
        let low = Vec3::Y * cfg.cover_eye;
        // Already out of their sight is as good as anywhere.
        if !self.clear(x, known.at, me.body.pos + low) {
            self.hide(x);
            return;
        }
        if self.cover_tries >= cfg.cover_tries {
            self.crouch = true;
            self.plan = Plan::Fight;
            self.no_cover = true;
            self.pressure = self.pressure.min(cfg.skill.cover_at * 0.9);
            return;
        }
        self.cover_tries += 1;
        // Feeling about for something solid, and then for whether this side of it is out of their sight.
        let knee = me.body.pos + Vec3::Y * x.walk.probe_height;
        for _ in 0..cfg.cover_spots {
            let forward = yaw_basis(self.dice.between(Vec2::new(0.0, TAU))).0;
            self.rays_spent += 1;
            let world = &x.sim.world;
            let Some(wall) = world.raycast(knee, forward, cfg.cover_reach).filter(|hit| hit.normal.y < x.sim.move_cfg.max_slope_cos) else {
                continue;
            };
            let away = wall.normal * cfg.cover_stand_off;
            let feet = me.body.pos + forward * wall.distance + Vec3::new(away.x, 0.0, away.z);
            let eye = feet + low;
            let path = eye - known.at;
            let length = path.length();
            self.rays_spent += 1;
            let hides = world.raycast(known.at, path / length, length).is_some_and(|hit| length - hit.distance < cfg.cover_hug);
            if hides {
                self.plan = Plan::Seek { spot: Vec2::new(feet.x, feet.z) };
                return;
            }
        }
    }

    fn hide(&mut self, x: &Battle) {
        self.plan = Plan::Hide;
        self.plan_left = self.dice.between(x.cfg.hide_time);
        self.crouch = true;
        self.lean = 0;
        self.burst_left = 0;
    }

    // ---- What goes out on the wire.

    fn command(&mut self, x: &Battle) -> PlayerCmd {
        let sim = x.sim;
        let tick = sim.tick;
        let me = &sim.soldiers[self.soldier];
        if self.plan == Plan::Move {
            return self.walk_on(x);
        }
        let skill = &x.cfg.skill;
        let dt = TICK_DT;
        self.pause_left -= dt;

        // What they can see of their own front sight, a moment late.
        let gun = me.weapon.aim_offset();
        let blend = 1.0 - (-dt / skill.sight_lag.max(1e-3)).exp();
        self.seen_gun += (gun - self.seen_gun) * blend;
        // A wandering idea of where exactly to aim.
        self.noise_left -= dt;
        if self.noise_left <= 0.0 {
            self.noise_left = self.dice.between(skill.tracking_time);
            self.noise_goal = self.dice.in_circle() * skill.tracking_noise;
        }
        self.noise += (self.noise_goal - self.noise) * (dt * 4.0).min(1.0);

        let aiming = matches!(self.plan, Plan::Fight | Plan::Suppress | Plan::Peek);
        let mut buttons = Buttons::default();
        let mut wanted = Vec2::new(self.nav.heading, 0.0);
        let mut firing_at = None;
        if aiming && let Some(contact) = self.contact {
            let goal = self.aim_point(x, &contact);
            wanted = goal - self.seen_gun * skill.sight_correct + self.noise;
            firing_at = Some((contact, goal + self.noise));
        }
        if let Plan::Revive { patient } = self.plan {
            let to = sim.soldiers[patient].body.pos - me.body.pos;
            let flat = Vec2::new(to.x, to.z);
            if flat.length() > 0.1 {
                wanted = Vec2::new(yaw_of(flat), -0.5);
            }
        }
        if let Plan::Throw { aim, .. } = self.plan {
            wanted = aim;
        }
        let turn = skill.aim_speed * dt;
        let close = (skill.aim_rate * dt).min(1.0);
        let off = Vec2::new(wrap(wanted.x - self.aim.x), wanted.y - self.aim.y);
        self.aim += (off * close).clamp(Vec2::splat(-turn), Vec2::splat(turn));
        self.aim.y = self.aim.y.clamp(-1.4, 1.4);

        let mut wish = Vec2::ZERO;
        let run = match self.plan {
            Plan::Seek { .. } => Some((1.0, true)),
            Plan::Flank { .. } => Some((1.0, false)),
            Plan::Revive { patient } => {
                let away = sim.soldiers[patient].body.pos.distance(me.body.pos);
                (away > x.cfg.medic_close + 0.2).then_some((1.0, away > x.cfg.medic_sprint))
            }
            _ => None,
        };
        if let Some((pace, sprint)) = run {
            let away = wrap(self.nav.heading - self.aim.x);
            wish = Vec2::new(-away.sin(), away.cos()) * pace;
            // Sprinting only goes forward, so face the way there first.
            buttons.set(Buttons::SPRINT, sprint && away.abs() < 0.5);
            self.crouch = false;
        }

        let press = tick % 2 == 0;
        buttons.set(Buttons::CROUCH, press && (me.body.stance == Stance::Crouch) != self.crouch);
        let side = me.body.lean.side;
        if side != self.lean && press {
            let button = if self.lean < 0 || (self.lean == 0 && side < 0) { Buttons::LEAN_LEFT } else { Buttons::LEAN_RIGHT };
            buttons.set(button, true);
        }
        buttons.set(Buttons::JUMP, std::mem::take(&mut self.nav.jump));

        self.look_after_the_gun(x, &mut buttons, press);

        // Aiming and shooting.
        let treating = matches!(self.plan, Plan::Revive { .. } | Plan::Treat | Plan::Throw { .. });
        let hip = self.contact.is_some_and(|c| c.at.distance(me.eye()) < skill.hip_range);
        buttons.set(Buttons::ADS, aiming && !hip && me.weapon.ready > 0.5);
        if !treating
            && let Some((contact, goal)) = firing_at
        {
            let may_fire = match self.plan {
                Plan::Fight => contact.visible && tick >= contact.ready_tick,
                Plan::Suppress => true,
                _ => false,
            };
            let fire = may_fire && self.trigger(x, &contact, goal, hip);
            buttons.set(Buttons::FIRE, fire);
        } else if !treating {
            self.burst_left = 0;
            self.pulled = false;
        }
        self.last_ammo = me.weapon.ammo;
        let mut cmd = PlayerCmd { tick, wish_move: wish, yaw: self.aim.x, pitch: self.aim.y, buttons, ..Default::default() };
        crate::no_window_on_a_jump(sim, self.soldier, &mut cmd);
        cmd
    }

    /// Off to the objective, with a walker's manners.
    fn walk_on(&mut self, x: &Battle) -> PlayerCmd {
        let me = &x.sim.soldiers[self.soldier];
        let around = Self::surroundings(x, x.area);
        let mut seen = Self::seen(me);
        seen.up = true;
        let cfg = x.walk;
        self.crouch = false;
        self.lean = 0;
        // Takes over from wherever the view has got to, with no jump.
        self.nav.yaw = self.aim.x;
        self.nav.pitch = self.aim.y;
        self.keep_together(x, me);
        let mut cmd = self.nav.step(cfg, &seen, &around, x.sim.tick);
        crate::no_window_on_a_jump(x.sim, self.soldier, &mut cmd);
        self.aim = Vec2::new(self.nav.yaw, self.nav.pitch);
        self.seen_gun = Vec2::ZERO;
        self.burst_left = 0;
        self.look_after_the_gun(x, &mut cmd.buttons, x.sim.tick % 2 == 0);
        self.last_ammo = me.weapon.ammo;
        cmd
    }

    /// Catches up with the squad, or waits for them on the way somewhere. A medic keeps closer than most.
    fn keep_together(&mut self, x: &Battle, me: &Soldier) {
        let cfg = x.cfg;
        let at = Vec2::new(me.body.pos.x, me.body.pos.z);
        self.nav.follow = None;
        self.nav.hold = false;
        self.nav.hurry = false;
        let Some(center) = self.squad_center else { return };
        let away = at.distance(center);
        let keep = if me.inventory.stash.medkits > 0 { cfg.medic_keep } else { cfg.squad_keep };
        if away > cfg.squad_group {
            return;
        }
        let goal = x.area.center;
        if center.distance(goal) > x.area.radius {
            // On the way there all of them follow the same way, so it is only a matter of pace.
            let along = |p: Vec2| x.way.and_then(|(grid, field)| grid.metres_to(field, p)).unwrap_or(p.distance(goal));
            let ahead = along(center) - along(at);
            let wait = ahead > cfg.squad_ahead && self.waited_for_squad < cfg.squad_wait;
            // Everyone waiting on each other would never get anywhere, so a wait runs out, and recovers slowly.
            let recover = cfg.squad_wait / cfg.squad_wait_recover.max(1e-3);
            self.waited_for_squad = if wait { self.waited_for_squad + TICK_DT } else { (self.waited_for_squad - TICK_DT * recover).max(0.0) };
            self.nav.hold = wait;
            self.nav.hurry = ahead < 0.0;
        } else if away > keep {
            self.nav.follow = Some(center);
        }
    }

    /// Reloading, and what to have in the hands.
    fn look_after_the_gun(&mut self, x: &Battle, buttons: &mut Buttons, press: bool) {
        let me = &x.sim.soldiers[self.soldier];
        let held = me.held_config(&x.sim.weapons);
        let weapon = &me.weapon;
        let share = weapon.ammo as f32 / held.magazine.max(1) as f32;
        let in_sight = self.contact.is_some_and(|c| c.visible);
        let safe = matches!(self.plan, Plan::Hide | Plan::Move | Plan::Treat);
        let close = self.contact.is_some_and(|c| c.at.distance(me.eye()) <= x.cfg.skill.hip_range * 2.0);
        let forced = weapon.ammo == 0 && matches!(self.plan, Plan::Fight) && (self.no_cover || close);
        let reload = forced || (safe && (weapon.ammo == 0 || (!in_sight && share < x.cfg.reload_below)));
        buttons.set(Buttons::RELOAD, press && reload && !weapon.reloading());

        let (slot, button) = match self.plan {
            Plan::Revive { .. } => (Slot::Medkit, Buttons::SLOT_4),
            Plan::Throw { kind: ThrownKind::Frag, .. } => (Slot::Frag, Buttons::GRENADE),
            Plan::Throw { kind: ThrownKind::Smoke, .. } => (Slot::Smoke, Buttons::GRENADE),
            _ => (Slot::Primary, Buttons::SLOT_1),
        };
        // Out of the primary and all its magazines, the sidearm is what there is.
        let (slot, button) = if slot == Slot::Primary && weapon.ammo == 0 && weapon.magazines.is_empty() && me.inventory.held == Slot::Primary {
            (Slot::Sidearm, Buttons::SLOT_2)
        } else if slot == Slot::Primary && me.inventory.held == Slot::Sidearm {
            (Slot::Sidearm, Buttons::SLOT_2)
        } else {
            (slot, button)
        };
        buttons.set(button, press && me.inventory.selected != slot);
        if let Plan::Throw { aim, .. } = self.plan {
            // On target and hands free, wind all the way up and let go.
            let on = Vec2::new(wrap(self.aim.x - aim.x), self.aim.y - aim.y).length() < x.sim.throw_cfg.overarm_lift * 0.5;
            let ready = me.inventory.selected == slot && me.weapon.ready <= 0.0;
            let wound_up = me.throw.held >= x.sim.throw_cfg.windup;
            buttons.set(Buttons::FIRE, ready && !wound_up && (on || me.throw.held > 0.0));
        }
        // Nothing to take out, so it works from a seat as well as on foot.
        buttons.set(Buttons::BANDAGE, self.plan == Plan::Treat);
        if matches!(self.plan, Plan::Revive { .. }) {
            let at_hand = me.inventory.selected == slot && me.weapon.ready <= 0.0;
            let near = match self.plan {
                Plan::Revive { patient } => x.sim.soldiers[patient].body.pos.distance(me.body.pos) <= x.sim.health_cfg.care_reach * 0.9,
                _ => true,
            };
            // Held down until it is done, once the hands are free and everything has stopped.
            buttons.set(Buttons::FIRE, at_hand && near);
        }
    }

    /// Where to point the view for this contact: at them, ahead of them and up for the drop, as judged.
    fn aim_point(&self, x: &Battle, contact: &Contact) -> Vec2 {
        let me = &x.sim.soldiers[self.soldier];
        let eye = me.eye();
        let cfg = me.held_config(&x.sim.weapons);
        let range = contact.at.distance(eye).max(1.0);
        let flight = range / cfg.ballistics.muzzle_velocity.max(1.0);
        let age = x.sim.tick.saturating_sub(contact.seen_tick) as f32 * TICK_DT;
        let moved = if contact.visible { contact.velocity * (age + flight * x.cfg.skill.lead) } else { Vec3::ZERO };
        let to = contact.at + moved - eye;
        let flat = Vec2::new(to.x, to.z);
        let drop = x.holdover.above_zero(me.weapon.weapon, self.judged_range.max(range * 0.5));
        let yaw = self.aim.x + wrap(yaw_of(flat) - self.aim.x);
        Vec2::new(yaw, (to.y / range).asin() + drop)
    }

    /// Whether to have the trigger back this tick: a few rounds, a wait, and the sights on again first.
    fn trigger(&mut self, x: &Battle, contact: &Contact, goal: Vec2, hip: bool) -> bool {
        let me = &x.sim.soldiers[self.soldier];
        let skill = &x.cfg.skill;
        let weapon = &me.weapon;
        let range = contact.at.distance(me.eye());
        if weapon.ammo < self.last_ammo && self.burst_left > 0 {
            self.burst_left -= 1;
            if self.burst_left == 0 {
                let t = ((range - skill.close_range) / (skill.far_range - skill.close_range).max(1.0)).clamp(0.0, 1.0);
                let pause = self.dice.between(skill.pause_close.lerp(skill.pause_far, t));
                self.pause_left = pause;
                self.waited = 0.0;
                self.judged_range = range * (1.0 + self.dice.between(Vec2::new(-1.0, 1.0)) * skill.range_error);
            }
        }
        let ready = weapon.ammo > 0 && weapon.ready >= 1.0 && !weapon.reloading() && !self.friend_in_line;
        if !ready {
            self.burst_left = 0;
            self.pulled = false;
            return false;
        }
        if self.burst_left == 0 {
            if self.pause_left > 0.0 {
                self.pulled = false;
                return false;
            }
            if !hip && weapon.ads < 0.95 {
                return false;
            }
            self.waited += TICK_DT;
            let sights = self.aim + self.seen_gun;
            let off = Vec2::new(wrap(sights.x - goal.x), sights.y - goal.y).length();
            let width = 0.45 / range.max(1.0);
            let on = off < width * skill.fire_tolerance || self.plan == Plan::Suppress;
            if !on && self.waited < skill.patience {
                self.pulled = false;
                return false;
            }
            let (least, most) = if range < skill.close_range {
                skill.burst_close
            } else if range < skill.far_range {
                skill.burst_mid
            } else {
                skill.burst_far
            };
            let extra = if self.plan == Plan::Suppress { 2 } else { 0 };
            self.burst_left = self.dice.between(Vec2::new(least as f32, most as f32 + 0.99)) as u32 + extra;
        }
        // On one shot at a time the trigger has to come up between rounds.
        let semi = weapon.mode == FireMode::Semi;
        let pull = !semi || !self.pulled;
        self.pulled = pull;
        pull
    }
}

pub(crate) fn flat_forward(yaw: f32) -> Vec2 {
    let forward = view_dir(yaw, 0.0);
    Vec2::new(forward.x, forward.z)
}
