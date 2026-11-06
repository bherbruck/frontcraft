//! Scripted players, for measuring: the network tests and the `prs_bot` load generator both drive
//! clients with these, so what a test proves is what a real run exercises.

use glam::Vec2;
use prs_sim::protocol::{Buttons, PlayerCmd};

#[derive(Clone, Copy, Debug)]
pub enum BotScript {
    /// Stands still, looking one way.
    Idle,
    /// Walks in a circle, turning steadily, without firing.
    Patrol,
    /// Everything an active player does: run, turn, jump, crouch, fire short bursts, reload.
    Busy,
}

/// The command `who` gives on `tick`. A pure function, so anyone can work out what it should do.
pub fn bot_cmd(script: BotScript, who: u32, tick: u32) -> PlayerCmd {
    let t = (tick + who * 37) as f32 / 60.0;
    let base = PlayerCmd { tick, yaw: who as f32 * 0.7, ..Default::default() };
    match script {
        BotScript::Idle => base,
        BotScript::Patrol => PlayerCmd {
            wish_move: Vec2::new(0.0, 1.0),
            yaw: base.yaw + t * 0.6,
            pitch: (t * 0.3).sin() * 0.1,
            ..base
        },
        BotScript::Busy => {
            let phase = (tick + who * 37) % 600;
            let mut buttons = Buttons::default();
            buttons.set(Buttons::SPRINT, phase < 120);
            buttons.set(Buttons::JUMP, phase == 130);
            buttons.set(Buttons::CROUCH, phase == 200 || phase == 320);
            buttons.set(Buttons::ADS, (360..540).contains(&phase));
            buttons.set(Buttons::FIRE, (420..450).contains(&phase) || (480..490).contains(&phase));
            buttons.set(Buttons::RELOAD, phase == 560);
            buttons.set(Buttons::LEAN_LEFT, phase == 240 || phase == 300);
            buttons.set(Buttons::LEAN_RIGHT, phase == 270);
            PlayerCmd {
                wish_move: if (360..540).contains(&phase) { Vec2::ZERO } else { Vec2::new((t * 0.4).sin(), 1.0).normalize() },
                yaw: base.yaw + t * 0.6 + (t * 3.0).sin() * 0.05,
                pitch: (t * 0.3).sin() * 0.15,
                buttons,
                ..base
            }
        }
    }
}
