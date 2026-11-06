//! Runs the real game on a script and writes down what came out, to the bit, so that two builds can be
//! compared. They do not agree: `sin` and `cos` differ in the last bits between native and wasm, so
//! terrain, buildings and the whole simulation drift apart, by about a centimetre in a minute with
//! nothing to correct it. A browser client on a native server gets by on `PredictConfig::epsilon`
//! and occasional reconciles. This is how to see whether that is still so.

use glam::Vec2;
use prs_sim::protocol::{Buttons, PlayerCmd};
use prs_sim::seats::SeatedAt;
use prs_sim::{Level, Sim};

use crate::rules_hash;

/// What a soldier does on `tick`, going through movement, collision, weapons and animation.
fn scripted(tick: u32) -> PlayerCmd {
    let t = tick as f32 / 60.0;
    let phase = tick % 600;
    let mut buttons = Buttons::default();
    buttons.set(Buttons::SPRINT, phase < 120);
    buttons.set(Buttons::JUMP, phase == 130);
    buttons.set(Buttons::CROUCH, phase == 200 || phase == 320);
    buttons.set(Buttons::ADS, (360..540).contains(&phase));
    buttons.set(Buttons::FIRE, (420..450).contains(&phase));
    buttons.set(Buttons::RELOAD, phase == 560);
    PlayerCmd {
        tick,
        wish_move: if (360..540).contains(&phase) { Vec2::ZERO } else { Vec2::new((t * 0.4).sin(), 1.0).normalize() },
        yaw: t * 0.6 + (t * 3.0).sin() * 0.05,
        pitch: (t * 0.3).sin() * 0.15,
        buttons,
        ..Default::default()
    }
}

/// The same again for something driven and something flown, which is where small differences grow:
/// one line per checkpoint with position, rotation and speed. `carrier` is who to sit in, and `flying`
/// says how to work the controls.
pub fn run_ride(map: &str, carrier: u32, flying: bool, ticks: u32, every: u32) -> Option<String> {
    let mut sim = Sim::new(Level::named(map)?);
    let id = sim.spawn_soldier(0x5EED);
    sim.sit_in(id, SeatedAt { carrier, seat: 0 });
    let mut out = String::new();
    for _ in 0..ticks {
        let tick = sim.tick + 1;
        let t = tick as f32 / 60.0;
        let mut cmd = PlayerCmd { tick, ..Default::default() };
        if flying {
            cmd.buttons.set(Buttons::JUMP, tick == 60);
            cmd.wish_move = Vec2::new((t * 0.4).sin() * 0.05, if t < 25.0 { 0.0 } else { 0.8 });
            cmd.stick = [(t.sin() * 15.0) as i8, 8];
        } else {
            cmd.wish_move = Vec2::new((t * 0.5).sin() * 0.05, 1.0);
        }
        sim.step(&[(id, cmd)]);
        if sim.tick % every.max(1) == 0 {
            let frame = sim.carrier_frame(carrier);
            let [x, y, z, w] = frame.rot.to_array();
            let values = [frame.pos.x, frame.pos.y, frame.pos.z, x, y, z, w];
            out.push_str(&format!("{}", sim.tick));
            for v in values {
                out.push_str(&format!(" {:08x}", v.to_bits()));
            }
            out.push('\n');
        }
    }
    Some(out)
}

/// One line for the rules, then one per checkpoint with the raw bits of everything that matters most.
pub fn run(map: &str, ticks: u32, every: u32) -> Option<String> {
    let mut sim = Sim::new(Level::named(map)?);
    let id = sim.spawn_soldier(0x5EED);
    let mut out = format!("{map} rules {:016x}\n", rules_hash(&sim, &prs_mode::Mode::free_practice()));
    let parts = [
        ("move", format!("{:?}", sim.move_cfg)),
        ("weapons", format!("{:?}", sim.weapons)),
        ("health", format!("{:?}", sim.health_cfg)),
        ("suppression", format!("{:?}", sim.suppression_cfg)),
        ("world", format!("{:?}", sim.world_cfg)),
        ("inventory", format!("{:?}", sim.inventory_cfg)),
        ("body", format!("{:?}", sim.body_cfg)),
        ("terrain", format!("{:?}", sim.level.terrain)),
        ("solids", format!("{:?}", sim.level.solids)),
        ("level", format!("{:?}", sim.level)),
    ];
    for (name, text) in parts {
        out.push_str(&format!("part {name} {:016x} {} {}\n", prs_net::hash_text(&text), text.len(), &text[..text.len().min(60)]));
    }
    for _ in 0..ticks {
        let cmd = scripted(sim.tick + 1);
        sim.step(&[(id, cmd)]);
        if sim.tick % every.max(1) == 0 {
            let s = &sim.soldiers[id];
            let values = [
                s.body.pos.x,
                s.body.pos.y,
                s.body.pos.z,
                s.body.vel.x,
                s.body.vel.y,
                s.body.vel.z,
                s.body.stride,
                s.weapon.recoil.x,
                s.weapon.recoil.y,
                s.weapon.wander.x,
                s.weapon.wander.y,
                s.weapon.sway.x,
                s.weapon.sway.y,
                s.gun_retract,
                s.anim.hip_yaw,
            ];
            out.push_str(&format!("{}", sim.tick));
            for v in values {
                out.push_str(&format!(" {:08x}", v.to_bits()));
            }
            out.push('\n');
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_build_gives_the_same_answer_twice() {
        assert_eq!(run("range", 300, 30), run("range", 300, 30));
        assert!(run("nowhere", 1, 1).is_none());
    }
}
