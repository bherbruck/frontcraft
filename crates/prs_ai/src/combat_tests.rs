//! The design goals for soldiers who fight, written as tests. They shoot the real gun, so these are as much
//! about the rules of the game as about the bots: nobody hits like a laser, being shot at keeps your head
//! down, and no one soldier mows down a group.

use super::*;
use crate::fighter::Plan;
use prs_mode::{Match, Mode, ModeEvent};
use prs_sim::health::{Health, HealthEvent};
use prs_sim::protocol::{Buttons, TICK_DT};
use prs_sim::{Level, SimEvent, Solid};
use std::collections::HashMap;

fn wall(center: Vec3, half: Vec3) -> Solid {
    Solid { center, half, rotation: glam::Quat::IDENTITY, paint: prs_sim::Paint::Concrete }
}

struct Field {
    sim: Sim,
    platoon: Platoon,
    areas: Vec<WalkArea>,
}

fn field(solids: Vec<Solid>, cfg: FightConfig) -> Field {
    let mut level = Level::flat(1600.0);
    level.solids = solids;
    let sim = Sim::new(level);
    let platoon = Platoon::new(&sim, cfg, WalkerConfig::default());
    Field { sim, platoon, areas: vec![WalkArea { center: Vec2::ZERO, radius: 30.0, doors: Vec::new() }] }
}

impl Field {
    fn soldier(&mut self, team: u8, at: Vec3, yaw: f32) -> SoldierId {
        let loadout = self.sim.inventory_cfg.loadout;
        let seed = 77 + self.sim.soldiers.len() as u32;
        let id = self.sim.spawn_soldier_with(seed, team, loadout);
        self.sim.set_home(id, at, yaw);
        id
    }

    fn fighter(&mut self, team: u8, at: Vec3, yaw: f32, squad: u32, holds: bool) -> SoldierId {
        let id = self.soldier(team, at, yaw);
        self.platoon.enlist(&self.sim, id, squad, squad as usize, holds, 0);
        id
    }

    fn step(&mut self, extra: &[(SoldierId, PlayerCmd)]) {
        let mut cmds = self.platoon.think(&self.sim, &self.areas, None).to_vec();
        cmds.extend_from_slice(extra);
        self.sim.step(&cmds);
    }

    fn fighter_of(&self, id: SoldierId) -> &Fighter {
        self.platoon.fighters.iter().find(|f| f.soldier == id).unwrap()
    }

    fn sees(&self, from: Vec3, to: Vec3) -> bool {
        let path = to - from;
        self.sim.world.raycast(from, path.normalize(), path.length()).is_none()
    }
}

const YAW_NORTH: f32 = 0.0;

/// How many shots a soldier fires at someone standing still in the open and how many land, in a minute or so.
fn shots_at_range(range: f32, skill: SkillConfig, seconds: f32) -> (u32, u32) {
    let mut f = field(Vec::new(), FightConfig { skill, ..Default::default() });
    let target = f.soldier(1, Vec3::new(0.0, 0.1, -range), 0.0);
    let shooter = f.fighter(0, Vec3::new(0.0, 0.1, 0.0), YAW_NORTH, 0, true);
    let (mut shots, mut hits) = (0, 0);
    for _ in 0..(seconds / TICK_DT) as u32 {
        f.step(&[]);
        f.sim.soldiers[shooter].weapon.magazines = vec![30, 30];
        let health = Health::new(&f.sim.health_cfg);
        f.sim.soldiers[target].health = health;
        for event in &f.sim.events {
            match *event {
                SimEvent::ShotFired { soldier, .. } if soldier == shooter => shots += 1,
                SimEvent::SoldierHit { soldier, .. } if soldier == target => hits += 1,
                _ => {}
            }
        }
    }
    (shots, hits)
}

/// A real gun, a real shooter with a slight tremor and a bullet that drops. Nothing is on rails at 200 m.
#[test]
fn at_200_m_a_bot_is_no_laser_and_still_a_threat() {
    let (shots, hits) = shots_at_range(200.0, SkillConfig::regular(), 240.0);
    let share = hits as f32 / shots.max(1) as f32;
    assert!(shots > 40, "barely fired: {shots} shots");
    assert!(hits > 0, "{hits} hits from {shots} shots: a threat should land some");
    assert!(share < 0.4, "{hits} hits from {shots} shots is a laser");
    // Taking their time about it, too.
    assert!((shots as f32) / 240.0 < 2.0, "{shots} shots in four minutes is not patient shooting");
}

/// Iron sights on someone a finger's width across. Fair at any skill, and not much of a threat at this range.
#[test]
fn at_300_m_iron_sights_miss_nearly_all_the_time() {
    for (name, skill, limit) in [("regular", SkillConfig::regular(), 0.1), ("veteran", SkillConfig::veteran(), 0.15)] {
        let (shots, hits) = shots_at_range(300.0, skill, 240.0);
        let share = hits as f32 / shots.max(1) as f32;
        assert!(shots > 40, "{name}: barely fired, {shots} shots");
        assert!(share < limit, "{name}: {hits} hits from {shots} shots at 300 m");
    }
}

/// Reading the sim's own suppression: a soldier with their head full of it gets down, stays down and knows who did it.
#[test]
fn suppression_keeps_a_bot_down_and_it_knows_who_to_blame() {
    let solids = vec![wall(Vec3::new(0.0, 1.1, -6.0), Vec3::new(5.0, 1.1, 0.3))];
    let mut f = field(solids, FightConfig::default());
    f.areas[0].center = Vec2::new(0.0, 80.0);
    f.areas[0].radius = 5.0;
    let enemy = f.soldier(1, Vec3::new(0.0, 0.1, -90.0), std::f32::consts::PI);
    let bot = f.fighter(0, Vec3::new(2.0, 0.1, 0.0), YAW_NORTH, 0, true);
    let mut fired = 0;
    for tick in 0..2400 {
        f.step(&[]);
        let felt = &mut f.sim.soldiers[bot].suppression;
        felt.level = 0.7;
        felt.hold = 1.0;
        felt.last_shooter = Some(enemy);
        if tick > 600 {
            assert!(matches!(f.fighter_of(bot).plan, Plan::Hide | Plan::Seek { .. } | Plan::Fight), "tick {tick}: {:?}", f.fighter_of(bot).plan);
            assert_ne!(f.fighter_of(bot).plan, Plan::Peek, "looked out while it was that bad");
        }
        fired += f.sim.events.iter().filter(|e| matches!(e, SimEvent::ShotFired { soldier, .. } if *soldier == bot)).count();
    }
    assert_eq!(f.fighter_of(bot).contact.map(|c| c.soldier), Some(enemy));
    assert!(matches!(f.fighter_of(bot).plan, Plan::Hide), "ended up {:?} with {fired} rounds fired", f.fighter_of(bot).plan);
    // It clears, and it comes back up.
    for _ in 0..2400 {
        f.step(&[]);
        f.sim.soldiers[bot].suppression.level = 0.0;
        if !matches!(f.fighter_of(bot).plan, Plan::Hide) {
            return;
        }
    }
    panic!("still down long after it cleared");
}

#[test]
fn closer_is_easier_and_skill_is_what_it_says() {
    let hits = |range: f32, skill: SkillConfig| shots_at_range(range, skill, 180.0).1;
    let near = hits(40.0, SkillConfig::regular());
    let far = hits(200.0, SkillConfig::regular());
    assert!(near > far, "{near} hits at 40 m, {far} at 200 m");
    let green = hits(120.0, SkillConfig::recruit());
    let hard = hits(120.0, SkillConfig::veteran());
    assert!(hard > green, "recruit {green}, veteran {hard}");
}

/// Someone with their back turned, far away, takes a while to be noticed, and only ever with a look.
#[test]
fn nobody_shoots_through_walls_or_out_of_the_back_of_their_head() {
    // Behind a long wall nothing is seen, and nothing is fired.
    let mut f = field(vec![wall(Vec3::new(0.0, 2.0, -20.0), Vec3::new(9.0, 2.0, 0.5))], FightConfig::default());
    f.soldier(1, Vec3::new(0.0, 0.1, -60.0), 0.0);
    let bot = f.fighter(0, Vec3::new(0.0, 0.1, 0.0), YAW_NORTH, 0, true);
    f.platoon.fighters[0].nav.area = 0;
    f.areas[0].center = Vec2::new(0.0, 0.0);
    f.areas[0].radius = 2.0;
    for _ in 0..600 {
        f.step(&[]);
        assert!(!f.sim.events.iter().any(|e| matches!(e, SimEvent::ShotFired { soldier, .. } if *soldier == bot)));
    }

    // In the open, behind them, at range: not noticed.
    let mut f = field(Vec::new(), FightConfig::default());
    f.soldier(1, Vec3::new(0.0, 0.1, 80.0), 0.0);
    let bot = f.fighter(0, Vec3::new(0.0, 0.1, 0.0), YAW_NORTH, 0, true);
    f.areas[0].radius = 2.0;
    f.step(&[]);
    assert!(f.fighter_of(bot).contact.is_none());
}

/// Sends rounds past someone, as a real shooter with a real gun would, from a spot with a line to them.
fn fire_past(f: &mut Field, shooter: SoldierId, at: Vec3, tick: u32) -> (SoldierId, PlayerCmd) {
    let eye = f.sim.soldiers[shooter].eye();
    let to = at - eye;
    let yaw = (-to.x).atan2(-to.z);
    let pitch = (to.y / to.length()).asin();
    let mut buttons = Buttons::default();
    buttons.set(Buttons::ADS, true);
    buttons.set(Buttons::FIRE, tick % 16 == 0);
    f.sim.soldiers[shooter].weapon.ammo = 30;
    (shooter, PlayerCmd { tick, yaw, pitch, buttons, ..Default::default() })
}

#[test]
fn a_pinned_bot_gets_behind_something_and_stays_there_until_the_fire_stops() {
    // A wall across the way, close enough to reach, with the shooter well beyond it.
    let solids = vec![wall(Vec3::new(4.0, 1.1, -5.0), Vec3::new(5.0, 1.1, 0.3))];
    let mut f = field(solids, FightConfig::default());
    f.areas[0].center = Vec2::new(0.0, 60.0);
    f.areas[0].radius = 5.0;
    let shooter = f.soldier(1, Vec3::new(4.0, 0.1, -90.0), std::f32::consts::PI);
    let bot = f.fighter(0, Vec3::new(1.0, 0.1, 0.0), YAW_NORTH, 0, true);

    let target = |f: &Field| f.sim.soldiers[bot].body.pos + Vec3::Y * 1.2;
    for tick in 0..3600u32 {
        let at = target(&f);
        let cmd = fire_past(&mut f, shooter, at, tick);
        f.step(&[cmd]);
        f.sim.soldiers[bot].health = Health::new(&f.sim.health_cfg);
    }
    // It has had a minute under fire by now. For the next one it has to keep its head out of the line.
    let mut shown = 0;
    let mut fired = 0;
    let total = 3600;
    for tick in 0..total as u32 {
        let at = target(&f);
        let cmd = fire_past(&mut f, shooter, at, tick);
        f.step(&[cmd]);
        f.sim.soldiers[bot].health = Health::new(&f.sim.health_cfg);
        let eye = f.sim.soldiers[bot].eye();
        let their_eye = f.sim.soldiers[shooter].eye();
        shown += u32::from(f.sees(their_eye, eye));
        fired += f.sim.events.iter().filter(|e| matches!(e, SimEvent::ShotFired { soldier, .. } if *soldier == bot)).count();
    }
    let share = shown as f32 / total as f32;
    assert!(share < 0.1, "showed itself {:.0}% of the time, plan {:?}, pressure {}", share * 100.0, f.fighter_of(bot).plan, f.fighter_of(bot).pressure);
    assert!(fired < 15, "fired {fired} rounds while pinned");

    // The fire stops, and it looks out again before long.
    let mut looked = false;
    for _ in 0..1800 {
        f.step(&[]);
        f.sim.soldiers[bot].health = Health::new(&f.sim.health_cfg);
        looked |= matches!(f.fighter_of(bot).plan, Plan::Peek | Plan::Fight | Plan::Move);
    }
    assert!(looked, "still hiding from nothing, pressure {}", f.fighter_of(bot).pressure);
}

/// A rifle that runs dry with an enemy in view goes behind something to fill it.
#[test]
fn a_bot_reloads_behind_cover_when_it_can() {
    let solids = vec![wall(Vec3::new(0.0, 1.1, -6.0), Vec3::new(5.0, 1.1, 0.3))];
    // A regular, so that what is tested is finding cover to reload behind and not how fast a bot sees or shoots.
    // Each skill draws its chances in its own order, and the cover found is one of those chances.
    let mut f = field(solids, FightConfig { skill: SkillConfig::regular(), ..Default::default() });
    f.areas[0].center = Vec2::new(0.0, 60.0);
    f.areas[0].radius = 5.0;
    let enemy = f.soldier(1, Vec3::new(12.0, 0.1, -80.0), 0.0);
    let bot = f.fighter(0, Vec3::new(7.0, 0.1, 0.0), YAW_NORTH, 0, true);
    let mut reloaded = None;
    let mut low = false;
    for _ in 0..3600 {
        f.step(&[]);
        // Nearly out, now that there is someone to be out in front of.
        if !low && f.fighter_of(bot).plan == Plan::Fight {
            f.sim.soldiers[bot].weapon.ammo = 2;
            low = true;
        }
        f.sim.soldiers[enemy].health = Health::new(&f.sim.health_cfg);
        if f.sim.events.iter().any(|e| matches!(e, SimEvent::Weapon { soldier, event: prs_sim::weapons::WeaponEvent::ReloadStarted } if *soldier == bot)) {
            let eye = f.sim.soldiers[bot].body.pos + Vec3::Y * f.platoon.cfg.cover_eye;
            reloaded = Some(f.sees(f.sim.soldiers[enemy].eye(), eye));
            break;
        }
    }
    assert_eq!(reloaded, Some(false), "reloaded where the enemy could see it, or never: {:?} at {} pressure {} contact {:?} tries {} no_cover {}", f.fighter_of(bot).plan, f.sim.soldiers[bot].body.pos, f.fighter_of(bot).pressure, f.fighter_of(bot).contact, f.fighter_of(bot).cover_tries, f.fighter_of(bot).no_cover);
}

#[test]
fn a_medic_goes_to_someone_down_and_gets_them_up() {
    let mut f = field(Vec::new(), FightConfig::default());
    f.areas[0].center = Vec2::new(0.0, 0.0);
    let medic = f.fighter(0, Vec3::new(0.0, 0.1, 0.0), YAW_NORTH, 0, true);
    let hurt = f.soldier(0, Vec3::new(14.0, 0.1, 6.0), 0.0);
    f.sim.soldiers[medic].inventory.stash.medkits = 1;
    f.sim.hurt(hurt, prs_sim::health::Zone::Chest, 120.0);
    assert!(!f.sim.soldiers[hurt].health.is_up());
    let mut revived = false;
    for _ in 0..(40.0 / TICK_DT) as u32 {
        f.step(&[]);
        revived |= f.sim.events.iter().any(|e| matches!(e, SimEvent::Health { soldier, event: HealthEvent::Revived } if *soldier == hurt));
    }
    assert!(revived, "plan {:?} from {} to {}", f.fighter_of(medic).plan, f.sim.soldiers[medic].body.pos, f.sim.soldiers[hurt].body.pos);
    assert_eq!(f.fighter_of(medic).plan, Plan::Move, "and gets on with it");
}

/// A squad meeting a machine gun across open ground with a few things to get behind.
#[test]
fn a_squad_goes_round_or_keeps_heads_down_rather_than_walking_into_fire() {
    let solids: Vec<Solid> = (0..14)
        .map(|i| {
            let x = (i as f32 * 2.399).sin() * (15.0 + i as f32 * 5.0);
            let z = -20.0 - i as f32 * 7.0;
            wall(Vec3::new(x, 1.0, z), Vec3::new(2.0, 1.0, 0.4))
        })
        .collect();
    let mut f = field(solids, FightConfig::default());
    // Their orders are on the far side of the gun.
    f.areas[0].center = Vec2::new(0.0, -220.0);
    f.areas[0].radius = 12.0;
    let gun = f.soldier(1, Vec3::new(0.0, 0.1, -130.0), std::f32::consts::PI);
    let squad: Vec<SoldierId> = (0..4).map(|i| f.fighter(0, Vec3::new(i as f32 * 3.0 - 4.5, 0.1, 0.0), YAW_NORTH, 3, i % 2 == 0)).collect();
    // Riflemen, who leave the medkits to a medic.
    for &id in &squad {
        f.sim.soldiers[id].inventory.stash.medkits = 0;
    }

    let bearing = |f: &Field, id: SoldierId| {
        let d = f.sim.soldiers[id].body.pos - f.sim.soldiers[gun].body.pos;
        d.x.atan2(d.z)
    };
    let start: Vec<f32> = squad.iter().map(|&id| bearing(&f, id)).collect();
    let mut furthest_round = 0.0_f32;
    let (mut charging, mut exposed) = (0u32, 0u32);
    let mut fired_at_nothing = 0;
    let mut flanked_plan = false;
    for tick in 0..(150.0 / TICK_DT) as u32 {
        // The gun fires at whoever is nearest with a line to it, in real bursts.
        let eye = f.sim.soldiers[gun].eye();
        let nearest = squad
            .iter()
            .filter(|&&id| f.sim.soldiers[id].health.is_up() && f.sees(eye, f.sim.soldiers[id].eye()))
            .min_by(|&&a, &&b| f.sim.soldiers[a].body.pos.distance(eye).total_cmp(&f.sim.soldiers[b].body.pos.distance(eye)))
            .copied();
        let mut cmds = Vec::new();
        if let Some(target) = nearest {
            let at = f.sim.soldiers[target].body.pos + Vec3::Y * 1.2;
            let mut cmd = fire_past(&mut f, gun, at, tick);
            cmd.1.buttons.set(Buttons::FIRE, tick % 150 < 30);
            cmds.push(cmd);
        }
        f.step(&cmds);
        f.sim.soldiers[gun].health = Health::new(&f.sim.health_cfg);
        for (i, &id) in squad.iter().enumerate() {
            let s = &f.sim.soldiers[id];
            if !s.health.is_up() {
                continue;
            }
            furthest_round = furthest_round.max((bearing(&f, id) - start[i]).abs());
            flanked_plan |= matches!(f.fighter_of(id).plan, Plan::Flank { .. });
            let to_gun = (f.sim.soldiers[gun].body.pos - s.body.pos).normalize();
            if f.sees(s.eye(), f.sim.soldiers[gun].eye()) {
                exposed += 1;
                charging += u32::from(s.body.vel.dot(to_gun) > 1.5);
            }
            if matches!(f.fighter_of(id).plan, Plan::Suppress) {
                fired_at_nothing += f.sim.events.iter().filter(|e| matches!(e, SimEvent::ShotFired { soldier, .. } if *soldier == id)).count();
            }
        }
    }
    let share = charging as f32 / exposed.max(1) as f32;
    println!("squad: round {:.0} deg, charging {:.0}% of {} exposed ticks, {} rounds of suppression", furthest_round.to_degrees(), share * 100.0, exposed, fired_at_nothing);
    assert!(share < 0.15, "ran at the gun for {:.0}% of the time it could see them", share * 100.0);
    assert!(flanked_plan, "nobody tried to go round");
    assert!(furthest_round.to_degrees() > 15.0 || fired_at_nothing > 0, "neither went round nor kept its head down");
}

/// All the luck comes from a seed, so a fight between the same people goes the same way every time.
#[test]
fn the_same_seed_fights_the_same_fight() {
    let run = || {
        let mut f = field(vec![wall(Vec3::new(0.0, 1.1, -30.0), Vec3::new(6.0, 1.1, 0.4))], FightConfig::default());
        for i in 0..6 {
            let team = (i % 2) as u8;
            let z = if team == 0 { 0.0 } else { -70.0 };
            let yaw = if team == 0 { 0.0 } else { std::f32::consts::PI };
            f.fighter(team, Vec3::new(i as f32 * 3.0 - 8.0, 0.1, z), yaw, team as u32, i % 4 == 0);
        }
        for _ in 0..3600 {
            f.step(&[]);
        }
        (f.sim.export_state(), f.platoon)
    };
    let (a, b) = (run(), run());
    assert!(a.0.soldiers.iter().any(|s| s.weapon.ammo < 30), "nobody fired, so this shows nothing");
    assert_eq!(a.0, b.0);
    assert_eq!(a.1, b.1);
}

/// Nobody in a match stands about on the way somewhere: a squad waiting on itself, or someone wedged.
#[test]
fn in_a_match_nobody_stands_about_when_they_should_be_going_somewhere() {
    let mut sim = Sim::new(Level::named("town").unwrap());
    let mut mode = Mode::advance_and_secure();
    mode.cfg.bots_per_team = Some(16);
    let mut game = Match::new(mode, &mut sim);
    game.fill_bots(&mut sim);
    let mut troops = Troops::new(&sim, TroopsConfig::default());
    let mut still: HashMap<SoldierId, (Vec3, f32)> = HashMap::new();
    let mut worst = (0.0_f32, 0, Vec3::ZERO);
    let mut why = String::new();
    for _ in 0..(300.0 / TICK_DT) as u32 {
        let cmds = troops.think(&sim, &game).to_vec();
        sim.step(&cmds);
        game.step(&mut sim, &[]);
        for f in &troops.platoon.fighters {
            let s = &sim.soldiers[f.soldier];
            let area = &troops.areas[f.nav.area];
            let on_the_way = f.plan == Plan::Move && s.health.is_up() && Vec2::new(s.body.pos.x, s.body.pos.z).distance(area.center) > area.radius;
            let entry = still.entry(f.soldier).or_insert((s.body.pos, 0.0));
            if !on_the_way || s.body.pos.distance(entry.0) > 3.0 {
                *entry = (s.body.pos, 0.0);
            } else {
                entry.1 += TICK_DT;
                if entry.1 > worst.0 {
                    worst = (entry.1, f.soldier, s.body.pos);
                    why = format!("area {} of radius {} at {}, hold {} hurry {} follow {:?} mood {:?} stuck {} detour {} goal {} speed {}", f.nav.area, area.radius, area.center, f.nav.hold, f.nav.hurry, f.nav.follow, f.nav.mood, f.nav.stuck_for, f.nav.detour_left, f.nav.goal, s.body.horizontal_speed());
                }
            }
        }
    }
    assert!(worst.0 < 25.0, "soldier {} stood {:.0} s at {} with somewhere to be: {why}", worst.1, worst.0, worst.2);
}

/// Smoke between them and it goes quiet, and it comes back when the smoke has gone. All sight is one question.
#[test]
fn smoke_stops_a_bot_seeing_you() {
    let mut f = field(Vec::new(), FightConfig::default());
    f.areas[0].center = Vec2::new(0.0, 0.0);
    f.areas[0].radius = 3.0;
    let target = f.soldier(1, Vec3::new(0.0, 0.1, -50.0), 0.0);
    let bot = f.fighter(0, Vec3::new(0.0, 0.1, 0.0), YAW_NORTH, 0, true);
    f.sim.soldiers[bot].inventory.stash.frags = 0;
    f.sim.soldiers[bot].inventory.stash.smokes = 0;
    let shots = |f: &Field| f.sim.events.iter().filter(|e| matches!(e, SimEvent::ShotFired { soldier, .. } if *soldier == bot)).count();
    let mut before = 0;
    for _ in 0..1800 {
        f.step(&[]);
        f.sim.soldiers[target].health = Health::new(&f.sim.health_cfg);
        before += shots(&f);
    }
    assert!(before > 3, "should have been shooting: {before} rounds");

    // A cloud full grown between them, for as long as this takes.
    let cfg = f.sim.throw_cfg.smoke.clone();
    let mut cloud = prs_sim::throwables::Cloud::new(9, Vec3::new(0.0, 0.0, -25.0), &cfg);
    cloud.age = cfg.grow_time;
    let (mut during, mut seen_during, mut last_shot) = (0, 0, 0);
    for tick in 0..1800 {
        f.sim.clouds = vec![cloud];
        f.step(&[]);
        f.sim.soldiers[target].health = Health::new(&f.sim.health_cfg);
        // A moment to lose sight of them, and what it remembers it may go on suppressing for a little.
        let round = shots(&f);
        if round > 0 {
            last_shot = tick;
        }
        if tick > 600 {
            during += round;
            seen_during += usize::from(f.fighter_of(bot).contact.is_some_and(|c| c.visible));
        }
    }
    assert_eq!(seen_during, 0, "saw through smoke, and fired {during} rounds, the last at tick {last_shot}");
    assert_eq!(during, 0, "fired through smoke at what it could not see, the last at tick {last_shot}");

    f.sim.clouds.clear();
    let mut after = 0;
    for _ in 0..1800 {
        f.step(&[]);
        f.sim.soldiers[target].health = Health::new(&f.sim.health_cfg);
        after += shots(&f);
    }
    assert!(after > 0, "it should take it up again");
}

/// With nothing to hold there is nowhere to meet but the middle of the map, and they have to find each other there.
#[test]
fn with_no_objective_two_sides_still_find_a_fight() {
    let mut sim = Sim::new(Level::named("town").unwrap());
    let mut game = Match::new(Mode::team_deathmatch(), &mut sim);
    game.fill_bots(&mut sim);
    let mut troops = Troops::new(&sim, TroopsConfig::default());
    let (mut shots, mut first) = (0, None);
    for tick in 0..(300.0 / TICK_DT) as u32 {
        let cmds = troops.think(&sim, &game).to_vec();
        sim.step(&cmds);
        game.step(&mut sim, &[]);
        let fired = sim.events.iter().filter(|e| matches!(e, SimEvent::ShotFired { .. })).count();
        if fired > 0 {
            first.get_or_insert(tick as f32 * TICK_DT);
        }
        shots += fired;
    }
    println!("team deathmatch: first shot at {first:?} s, {shots} shots in 5 minutes");
    assert!(first.is_some_and(|t| t < 180.0), "first shot at {first:?}");
    assert!(shots > 100, "{shots} shots in 5 minutes is not much of a fight");
}

fn bleeding_stops(f: &mut Field, bot: SoldierId, seconds: f32) -> bool {
    f.sim.hurt(bot, prs_sim::health::Zone::Arm, 20.0);
    assert!(f.sim.soldiers[bot].health.bleeding());
    let before = f.sim.soldiers[bot].inventory.stash.bandages;
    for _ in 0..(seconds / TICK_DT) as u32 {
        f.step(&[]);
        if !f.sim.soldiers[bot].health.bleeding() {
            assert_eq!(f.sim.soldiers[bot].inventory.stash.bandages, before - 1);
            return true;
        }
    }
    false
}

#[test]
fn a_bot_bandages_itself_on_foot_and_in_a_seat() {
    let mut f = field(Vec::new(), FightConfig::default());
    f.areas[0].radius = 3.0;
    let bot = f.fighter(0, Vec3::new(0.0, 0.1, 0.0), YAW_NORTH, 0, true);
    assert!(bleeding_stops(&mut f, bot, 20.0), "on foot: {:?}", f.fighter_of(bot).plan);

    let mut level = Level::flat(1000.0);
    level.vehicles.push(prs_sim::VehicleSpawn { kind: prs_sim::vehicles::VehicleTable::UTILITY_TRUCK.0, pos: Vec3::ZERO, yaw: 0.0 });
    let sim = Sim::new(level);
    let platoon = Platoon::new(&sim, FightConfig::default(), WalkerConfig::default());
    let mut f = Field { sim, platoon, areas: vec![WalkArea { center: Vec2::ZERO, radius: 3.0, doors: Vec::new() }] };
    for _ in 0..60 {
        f.step(&[]);
    }
    for seat in [0, 1] {
        let bot = f.fighter(0, Vec3::new(4.0, 0.1, seat as f32), YAW_NORTH, 0, true);
        f.sim.sit_in(bot, prs_sim::seats::SeatedAt { carrier: 0, seat });
        assert!(f.sim.soldiers[bot].seat.is_some());
        assert!(bleeding_stops(&mut f, bot, 20.0), "seat {seat}: {:?}", f.fighter_of(bot).plan);
    }
}

/// Sparingly, with the right things, at the right moments.
#[test]
fn bots_throw_things_in_a_match_but_not_many() {
    let (_, _, tally, _) = play_town(600.0, 16);
    println!("{tally:?}");
    assert!(tally.frags + tally.smokes > 0, "nobody threw anything: {tally:?}");
    // A soldier has a couple of each and a good few lives in this time. Well short of what they could.
    assert!(tally.frags + tally.smokes < 150, "that is throwing everything they have: {tally:?}");
}

/// Everything that happened in a bots vs bots match.
#[derive(Debug, Default)]
struct Tally {
    shots: u32,
    hits: u32,
    downs: u32,
    deaths: u32,
    revives: u32,
    captures: u32,
    /// The most anyone put down inside the window, on anyone.
    worst_streak: u32,
    team_hits: u32,
    frags: u32,
    smokes: u32,
    treatments: u32,
    /// Ticks spent on each plan, all soldiers together.
    plans: HashMap<&'static str, u32>,
    /// Ticks anyone was down with a medic within reach.
    down_near_medic: u32,
    down_ticks: u32,
}

fn play_town(seconds: f32, per_team: usize) -> (Sim, Match, Tally, f64) {
    const STREAK_WINDOW: f32 = 15.0;
    let mut sim = Sim::new(Level::named("town").unwrap());
    let mut mode = Mode::advance_and_secure();
    mode.cfg.bots_per_team = Some(per_team);
    let mut game = Match::new(mode, &mut sim);
    game.fill_bots(&mut sim);
    let mut troops = Troops::new(&sim, TroopsConfig::default());
    let mut tally = Tally::default();
    let mut kills: HashMap<SoldierId, Vec<u32>> = HashMap::new();
    let mut cmds = Vec::new();
    let mut thinking = std::time::Duration::ZERO;
    let ticks = (seconds / TICK_DT) as u32;
    for tick in 0..ticks {
        cmds.clear();
        let t = std::time::Instant::now();
        cmds.extend_from_slice(troops.think(&sim, &game));
        thinking += t.elapsed();
        sim.step(&cmds);
        game.step(&mut sim, &[]);
        for event in &sim.events {
            match *event {
                SimEvent::ShotFired { .. } => tally.shots += 1,
                SimEvent::ThrownReleased { kind: prs_sim::throwables::ThrownKind::Frag, .. } => tally.frags += 1,
                SimEvent::ThrownReleased { .. } => tally.smokes += 1,
                SimEvent::SoldierHit { soldier, shooter, .. } => {
                    tally.hits += 1;
                    tally.team_hits += u32::from(sim.soldiers[soldier].team == sim.soldiers[shooter].team);
                }
                SimEvent::Health { soldier, event: HealthEvent::Downed } => {
                    tally.downs += 1;
                    if let Some(by) = sim.soldiers[soldier].attacker {
                        let list = kills.entry(by).or_default();
                        list.push(tick);
                        let window = (STREAK_WINDOW / TICK_DT) as u32;
                        tally.worst_streak = tally.worst_streak.max(list.iter().filter(|t| tick - **t < window).count() as u32);
                    }
                }
                SimEvent::Health { event: HealthEvent::Died, .. } => tally.deaths += 1,
                SimEvent::Health { event: HealthEvent::Revived, .. } => tally.revives += 1,
                _ => {}
            }
        }
        for f in &troops.platoon.fighters {
            let s = &sim.soldiers[f.soldier];
            if !s.present {
                continue;
            }
            let name = match f.plan {
                Plan::Move => "move",
                Plan::Fight => "fight",
                Plan::Suppress => "suppress",
                Plan::Seek { .. } => "seek",
                Plan::Hide => "hide",
                Plan::Peek => "peek",
                Plan::Flank { .. } => "flank",
                Plan::Revive { .. } => "revive",
                Plan::Treat => "treat",
                Plan::Throw { .. } => "throw",
            };
            *tally.plans.entry(name).or_default() += 1;
            if s.health.life == prs_sim::health::Life::Down {
                tally.down_ticks += 1;
                let medic = sim.soldiers.iter().any(|m| m.present && m.team == s.team && m.health.is_up() && m.inventory.stash.medkits > 0 && m.body.pos.distance(s.body.pos) < 45.0);
                tally.down_near_medic += u32::from(medic);
            }
        }
        tally.treatments += sim.events.iter().filter(|e| matches!(e, SimEvent::Carry { event: prs_sim::CarryEvent::TreatmentStarted { .. }, .. })).count() as u32;
        tally.captures += game.events.iter().filter(|e| matches!(e, ModeEvent::PointCaptured { .. })).count() as u32;
    }
    (sim, game, tally, thinking.as_secs_f64() * 1000.0 / ticks as f64)
}

/// The player's rule still holds with bots on both sides of it: nobody mows down a group, and the fight goes
/// on being a fight. Set up as real kits on `town`, playing Advance and Secure.
#[test]
fn in_a_match_on_town_nobody_mows_down_a_group() {
    let (_, game, tally, _) = play_town(600.0, 8);
    println!("{tally:?} {:?}", game.state.teams.iter().map(|t| t.tickets).collect::<Vec<_>>());
    assert!(tally.shots > 200, "hardly a fight: {tally:?}");
    assert!(tally.downs + tally.deaths > 5, "nobody got hurt: {tally:?}");
    assert!(tally.captures >= 1, "nothing was taken: {tally:?}");
    assert!(tally.worst_streak <= 4, "one soldier put down {} inside 15 s: {tally:?}", tally.worst_streak);
    let hit_share = tally.hits as f32 / tally.shots as f32;
    assert!(hit_share < 0.35, "a round in {:.1} hits: {tally:?}", 1.0 / hit_share);
    assert!(tally.team_hits * 5 <= tally.hits.max(5), "mostly hitting their own side: {tally:?}");
}

/// `cargo test -p prs_ai match_report -- --ignored --nocapture`. How a whole match goes and what it costs.
#[test]
#[ignore]
fn match_report() {
    let (sim, game, tally, ms) = play_town(900.0, 16);
    println!("{tally:?}");
    println!("think {ms:.3} ms/tick for {} bots on {} soldiers", game.state.players.len(), sim.soldiers.len());
    for team in &game.state.teams {
        println!("tickets {}", team.tickets);
    }
    println!("phase {:?}", game.state.phase);
    let (mut kills, mut deaths, mut revives) = (0, 0, 0);
    for p in &game.state.players {
        kills += p.score.kills;
        deaths += p.score.deaths;
        revives += p.score.revives;
    }
    println!("scoreboard: kills {kills} deaths {deaths} revives {revives}");
}

/// `cargo test -p prs_ai fight_cost -- --ignored --nocapture`. Thirty-two soldiers in sight of each other.
#[test]
#[ignore]
fn fight_cost() {
    let mut f = field(Vec::new(), FightConfig::default());
    f.areas[0].center = Vec2::new(0.0, 0.0);
    f.areas[0].radius = 100.0;
    for i in 0..32 {
        let team = (i % 2) as u8;
        let z = if team == 0 { 80.0 } else { -80.0 };
        let yaw = if team == 0 { 0.0 } else { std::f32::consts::PI };
        f.fighter(team, Vec3::new(i as f32 * 4.0 - 64.0, 0.1, z), yaw, team as u32, i % 4 == 0);
    }
    let ticks = 3600;
    let (mut thinking, mut stepping) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    let mut cmds = Vec::new();
    for _ in 0..ticks {
        let t = std::time::Instant::now();
        cmds.clear();
        cmds.extend_from_slice(f.platoon.think(&f.sim, &f.areas, None));
        thinking += t.elapsed();
        let t = std::time::Instant::now();
        f.sim.step(&cmds);
        stepping += t.elapsed();
    }
    let rays: u32 = f.platoon.fighters.iter().map(|x| x.rays_spent).sum();
    println!("32 fighters: think {:.3} ms/tick, sim step {:.3} ms/tick, {:.1} sight rays/tick", thinking.as_secs_f64() * 1000.0 / ticks as f64, stepping.as_secs_f64() * 1000.0 / ticks as f64, rays as f32 / ticks as f32);
}

/// Most the two sides' walks to their first and second points may differ, as a share of the longer.
const FAIR_WALK: f32 = 0.10;

/// Neither base gets to its first point or on to its second much sooner, measured along the ground, and
/// every point in the chain can be walked to from the one before it.
fn both_sides_have_as_far_to_walk_up_the_chain(map: &str) {
    let mut sim = Sim::new(Level::named(map).unwrap());
    let game = Match::new(Mode::advance_and_secure(), &mut sim);
    let troops = Troops::new(&sim, TroopsConfig::default());
    let (flow, chain) = (&troops.flow, &game.state.chain);
    let flat = |p: glam::Vec3| Vec2::new(p.x, p.z);
    let points = &sim.level.capture_points;
    let walk = |from: Vec2, to: usize| flow.grid.walk_length(&flow.fields[to], from).unwrap_or_else(|| panic!("{map}: no way to {}", points[to].name));
    for pair in chain.windows(2) {
        walk(flat(points[pair[0]].center), pair[1]);
        walk(flat(points[pair[1]].center), pair[0]);
    }
    let walks = |team: usize, first: usize, second: usize| {
        let base = flat(sim.level.teams[team].spawns[0].pos);
        (walk(base, first), walk(flat(points[first].center), second))
    };
    let last = chain.len() - 1;
    let south = walks(0, chain[1], chain[2]);
    let north = walks(1, chain[last - 1], chain[last - 2]);
    println!("{map}: south {south:.0?} north {north:.0?}, {:?}", chain.iter().map(|&i| &points[i].name).collect::<Vec<_>>());
    for (name, a, b) in [("first", south.0, north.0), ("second", south.1, north.1), ("both", south.0 + south.1, north.0 + north.1)] {
        assert!((a - b).abs() <= a.max(b) * FAIR_WALK, "{map}, {name} walk: south {a:.0} m, north {b:.0} m");
    }
}

#[test]
fn both_sides_have_as_far_to_walk_up_the_chain_of_the_town() {
    both_sides_have_as_far_to_walk_up_the_chain("town");
}

#[test]
fn both_sides_have_as_far_to_walk_up_the_chain_of_claudabad() {
    both_sides_have_as_far_to_walk_up_the_chain("claudabad");
}

/// `cargo test -p prs_ai side_report -- --ignored --nocapture`. Who wins on `town` over ten seeds, and what
/// goes differently for each side on the way.
#[test]
#[ignore]
fn side_report() {
    let seconds: f32 = std::env::var("SIDE_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(900.0);
    let per_team: usize = std::env::var("SIDE_BOTS").ok().and_then(|v| v.parse().ok()).unwrap_or(16);
    let first: u32 = std::env::var("SIDE_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let handles: Vec<_> = (first..first + 10).map(|i| std::thread::spawn(move || side_run(i, seconds, per_team))).collect();
    let mut wins = [0, 0];
    let mut revives = 0;
    let mut rounds = [0, 0];
    let mut caps = [0, 0];
    let mut first_caps = ([0.0_f32; 2], [0.0_f32; 2]);
    for h in handles {
        let r = h.join().unwrap();
        println!("{r}");
        if let Some(rest) = r.split("winner ").nth(1) {
            if rest.starts_with('0') { wins[0] += 1 }
            if rest.starts_with('1') { wins[1] += 1 }
        }
        if let Some(rest) = r.split("caps [").nth(1).and_then(|v| v.split(']').next()) {
            let got: Vec<u32> = rest.split(", ").filter_map(|v| v.parse().ok()).collect();
            caps[0] += got.first().copied().unwrap_or(0);
            caps[1] += got.get(1).copied().unwrap_or(0);
        }
        if let Some(rest) = r.split(" cap [").nth(1).and_then(|v| v.split(']').next()) {
            for (team, part) in rest.split(", ").enumerate().take(2) {
                if let Some(t) = part.trim_start_matches("Some(").trim_end_matches(')').parse::<f32>().ok() {
                    first_caps.0[team] += t;
                    first_caps.1[team] += 1.0;
                }
            }
        }
        if let Some(rest) = r.split("rounds won ").nth(1) {
            let won: Vec<u32> = rest.trim_matches(|c| c == '[' || c == ']').split(", ").filter_map(|v| v.parse().ok()).collect();
            rounds[0] += won.first().copied().unwrap_or(0);
            rounds[1] += won.get(1).copied().unwrap_or(0);
        }
        revives += r.split("revives ").nth(1).and_then(|v| v.split(' ').next()).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    }
    let mean = |team: usize| first_caps.0[team] / first_caps.1[team].max(1.0);
    println!("wins team 0 {} team 1 {}; rounds won {rounds:?}; captures {caps:?}; mean first capture {:.0} s, {:.0} s; revives {revives}", wins[0], wins[1], mean(0), mean(1));
}

fn side_run(seed: u32, seconds: f32, per_team: usize) -> String {
    let mut level = Level::named("town").unwrap();
    // `SIDE_SWAP=1` gives each team the other's base, to tell the map from the team. `SIDE_MOVE=name,x,z` tries a point elsewhere.
    if std::env::var("SIDE_SWAP").is_ok() {
        level.teams.swap(0, 1);
    }
    if let Ok(spec) = std::env::var("SIDE_MOVE") {
        for one in spec.split(';') {
            let parts: Vec<&str> = one.split(',').collect();
            let point = level.capture_points.iter_mut().find(|p| p.name == parts[0]).expect("a point by that name");
            point.center.x = parts[1].parse().unwrap();
            point.center.z = parts[2].parse().unwrap();
        }
    }
    let mut sim = Sim::new(level);
    // `SIDE_MODE=tdm` plays another mode, and `SIDE_SUPPRESSION=0` has bots take no notice of suppression.
    let mut mode = std::env::var("SIDE_MODE").ok().and_then(|name| Mode::named(&name)).unwrap_or_else(Mode::advance_and_secure);
    mode.cfg.bots_per_team = Some(per_team);
    let mut game = Match::new(mode, &mut sim);
    game.fill_bots(&mut sim);
    let mut cfg = TroopsConfig::default();
    cfg.walk.seed = cfg.walk.seed.wrapping_add(seed.wrapping_mul(7919));
    if let Some(weight) = std::env::var("SIDE_SUPPRESSION").ok().and_then(|v| v.parse().ok()) {
        cfg.fight.skill.suppression_weight = weight;
    }
    let mut troops = Troops::new(&sim, cfg);
    let mut first_shot = [None; 2];
    let mut first_hit = [None; 2];
    let mut first_capture = [None; 2];
    let (mut shots, mut hits, mut deaths, mut caps) = ([0u32; 2], [0u32; 2], [0u32; 2], [0u32; 2]);
    let mut revives = 0u32;
    let mut rounds = [0u32; 2];
    let mut downs: HashMap<SoldierId, Vec<f32>> = HashMap::new();
    let mut streak = 0;
    let (mut frags, mut smokes) = (0, 0);
    let mut cmds = Vec::new();
    for tick in 0..(seconds / TICK_DT) as u32 {
        cmds.clear();
        cmds.extend_from_slice(troops.think(&sim, &game));
        sim.step(&cmds);
        game.step(&mut sim, &[]);
        let t = tick as f32 * TICK_DT;
        for event in &sim.events {
            match *event {
                SimEvent::ShotFired { soldier, .. } => {
                    let team = sim.soldiers[soldier].team as usize;
                    shots[team] += 1;
                    first_shot[team].get_or_insert(t);
                }
                SimEvent::SoldierHit { shooter, .. } => {
                    let team = sim.soldiers[shooter].team as usize;
                    hits[team] += 1;
                    first_hit[team].get_or_insert(t);
                }
                SimEvent::Health { soldier, event: HealthEvent::Died } => deaths[sim.soldiers[soldier].team as usize] += 1,
                SimEvent::Health { event: HealthEvent::Revived, .. } => revives += 1,
                SimEvent::Health { soldier, event: HealthEvent::Downed } => {
                    if let Some(by) = sim.soldiers[soldier].attacker {
                        let list = downs.entry(by).or_default();
                        list.push(t);
                        streak = streak.max(list.iter().filter(|at| t - **at < 15.0).count());
                    }
                }
                SimEvent::ThrownReleased { kind: prs_sim::throwables::ThrownKind::Frag, .. } => frags += 1,
                SimEvent::ThrownReleased { .. } => smokes += 1,
                _ => {}
            }
        }
        for event in &game.events {
            if let ModeEvent::PointCaptured { team, .. } = *event {
                caps[team as usize] += 1;
                first_capture[team as usize].get_or_insert(t);
            }
            if let ModeEvent::RoundEnded { winner: Some(team) } = *event {
                rounds[team as usize] += 1;
            }
        }
    }
    let tickets = [game.state.teams[0].tickets, game.state.teams[1].tickets];
    // A round already won counts for far more than where the next one has got to.
    let score = |team: usize| rounds[team] as i64 * 1000 + tickets[team] as i64;
    let winner = if score(0) > score(1) { 0 } else if score(1) > score(0) { 1 } else { 2 };
    let kits = |team: u8| game.state.players.iter().filter(|p| p.team == team).fold([0u32; 4], |mut k, p| { k[p.kit.min(3)] += 1; k });
    let squads = |team: u8| game.state.squads.iter().filter(|s| s.team == team).map(|s| s.id).collect::<Vec<_>>();
    format!(
        "seed {seed}: tickets {tickets:?} deaths {deaths:?} shots {shots:?} hits {hits:?} caps {caps:?} first shot {first_shot:.0?} hit {first_hit:.0?} cap {first_capture:.0?} kits {:?} {:?} squads {:?} {:?} frags {frags} smoke {smokes} streak {streak} revives {revives} winner {winner} rounds won {rounds:?}",
        kits(0), kits(1), squads(0), squads(1)
    )
}

/// A town match with bots and nobody else: they take the trucks where they have a long way to go, they get
/// where they are going, and nobody sits in one for good.
/// A truck that goes over with bots in it: out they get, and off they go, rather than sit there.
#[test]
fn bots_in_a_truck_that_goes_over_get_out() {
    let mut sim = Sim::new(Level::named("town").unwrap());
    let mut mode = Mode::advance_and_secure();
    mode.cfg.bots_per_team = Some(16);
    let mut game = Match::new(mode, &mut sim);
    game.fill_bots(&mut sim);
    let mut troops = Troops::new(&sim, TroopsConfig::default());
    let mut over = None;
    for _ in 0..(150.0 / TICK_DT) as u32 {
        let cmds = troops.think(&sim, &game).to_vec();
        sim.step(&cmds);
        game.step(&mut sim, &[]);
        if over.is_none() {
            over = sim.soldiers.iter().find(|soldier| soldier.seat.is_some() && soldier.transit.is_none() && soldier.seat.is_some_and(|at| sim.vehicles.get(at.carrier as usize).is_some_and(|vehicle| vehicle.vel.length() > 3.0))).and_then(|soldier| soldier.seat).map(|at| at.carrier as usize);
            if let Some(v) = over {
                let truck = &mut sim.vehicles[v];
                truck.rot *= glam::Quat::from_rotation_z(std::f32::consts::PI);
                truck.pos.y += 2.0;
                truck.vel = Vec3::ZERO;
                break;
            }
        }
    }
    let v = over.expect("somebody should have got in a truck");
    assert!(sim.soldiers.iter().any(|soldier| soldier.seat.is_some_and(|at| at.carrier as usize == v)));
    for _ in 0..(25.0 / TICK_DT) as u32 {
        let cmds = troops.think(&sim, &game).to_vec();
        sim.step(&cmds);
        game.step(&mut sim, &[]);
    }
    let left: Vec<_> = sim.soldiers.iter().filter(|soldier| soldier.present && soldier.health.is_up() && soldier.seat.is_some_and(|at| at.carrier as usize == v)).map(|soldier| soldier.seat).collect();
    assert!(left.is_empty(), "still in it: {left:?}, up {}", (sim.vehicles[v].rot * Vec3::Y).y);
}

#[test]
fn squads_ride_trucks_to_the_objective_and_get_out_to_fight() {
    let mut sim = Sim::new(Level::named("town").unwrap());
    let mut mode = Mode::advance_and_secure();
    mode.cfg.bots_per_team = Some(16);
    let mut game = Match::new(mode, &mut sim);
    game.fill_bots(&mut sim);
    let mut troops = Troops::new(&sim, TroopsConfig::default());
    let homes: Vec<Vec3> = sim.vehicles.iter().map(|vehicle| vehicle.pos).collect();
    let mut seated_for = vec![0u32; sim.soldiers.len()];
    let mut stuck_longest = 0u32;
    let mut furthest = vec![0.0_f32; sim.vehicles.len()];
    let mut think_ms = 0.0_f64;
    let ticks = (240.0 / TICK_DT) as u32;
    let mut ever_seated = 0;
    for _ in 0..ticks {
        let t = std::time::Instant::now();
        let cmds = troops.think(&sim, &game).to_vec();
        think_ms += t.elapsed().as_secs_f64() * 1000.0;
        sim.step(&cmds);
        game.step(&mut sim, &[]);
        seated_for.resize(sim.soldiers.len(), 0);
        for (id, soldier) in sim.soldiers.iter().enumerate() {
            // In a seat of something going nowhere, that is, as opposed to on the way.
            let parked = soldier.seat.is_some_and(|at| sim.vehicles.get(at.carrier as usize).is_some_and(|vehicle| vehicle.vel.length() < 0.6));
            seated_for[id] = if parked && soldier.present && soldier.health.is_up() { seated_for[id] + 1 } else { 0 };
            stuck_longest = stuck_longest.max(seated_for[id]);
            ever_seated += soldier.seat.is_some() as u32;
        }
        for (v, vehicle) in sim.vehicles.iter().enumerate() {
            furthest[v] = furthest[v].max(vehicle.pos.distance(homes[v]));
        }
    }
    let convoy = &troops.convoy;
    println!(
        "{} trips, {} arrived, {} gave up, longest stuck seated {:.1} s, trucks went {furthest:.0?} m, think {:.3} ms/tick for {} bots, {:.0} soldier-seconds in seats",
        convoy.started,
        convoy.arrived,
        convoy.gave_up,
        stuck_longest as f32 * TICK_DT,
        think_ms / ticks as f64,
        game.state.players.len(),
        ever_seated as f32 * TICK_DT,
    );
    assert!(convoy.started >= 2, "trucks should get used: {convoy:?}");
    assert!(convoy.arrived >= 1, "and get somewhere: {convoy:?}");
    assert!(furthest.iter().filter(|far| **far > 100.0).count() >= 2, "{furthest:?}");
    assert!(think_ms / (ticks as f64) < 1.0, "bots should stay cheap: {} ms/tick", think_ms / ticks as f64);
    assert!((stuck_longest as f32 * TICK_DT) < 40.0, "someone sat parked for {} s", stuck_longest as f32 * TICK_DT);
}


