//! A match on the wire. The server decides everything about a match and a client only ever hears the
//! result, so a client's copy is for showing, and what a client sends is only ever a request.
//!
//! The state goes out with the rest of the shared world, whole, when it has changed. It holds no clocks
//! that tick, so that is not often, and only what a client can use is in it.

use prs_mode::{MatchState, ModeCmd, ModeEvent, Phase, PlayerState, PointState, Rally, RallyLoss, Refusal, Score, SpawnAt, Squad, TEAMS, TeamState};
use prs_net::wire::{Reader, WireError, Writer};
use prs_sim::SimEvent;

use crate::events;

const MAX_ITEMS: usize = 4096;
const NO_ID: u64 = 0;

fn write_option(w: &mut Writer, value: Option<u64>) {
    w.varu(value.map_or(NO_ID, |v| v + 1));
}

fn read_option(r: &mut Reader) -> Result<Option<u64>, WireError> {
    Ok(r.varu()?.checked_sub(1))
}

fn spawn_at_to_u8(at: SpawnAt) -> u8 {
    match at {
        SpawnAt::Base => 0,
        SpawnAt::Rally => 1,
    }
}

fn spawn_at_from_u8(v: u8) -> SpawnAt {
    if v == 1 { SpawnAt::Rally } else { SpawnAt::Base }
}

const REFUSALS: [Refusal; 21] = [
    Refusal::RoundOver,
    Refusal::NoSuchKit,
    Refusal::KitTaken,
    Refusal::NoRally,
    Refusal::NoRallyRules,
    Refusal::SquadFull,
    Refusal::NoSuchSquad,
    Refusal::TooManySquads,
    Refusal::OtherTeam,
    Refusal::InTheField,
    Refusal::TeamsUneven,
    Refusal::NotLeader,
    Refusal::NotUp,
    Refusal::NotOnGround,
    Refusal::RallyCooling,
    Refusal::NoSquadmateNear,
    Refusal::EnemyNear,
    Refusal::TooCloseToBase,
    Refusal::InAZone,
    Refusal::NoTickets,
    Refusal::NotDownLongEnough,
];

const RALLY_LOSSES: [RallyLoss; 5] = [RallyLoss::Overrun, RallyLoss::Expired, RallyLoss::UsedUp, RallyLoss::Replaced, RallyLoss::SquadGone];

fn write_player(p: &PlayerState, w: &mut Writer) {
    w.varu(p.soldier as u64);
    w.u8(p.team);
    write_option(w, p.squad.map(u64::from));
    w.varu(p.kit as u64);
    let flags = spawn_at_to_u8(p.spawn) | (p.ready as u8) << 1 | (p.bot as u8) << 2 | (p.deployed as u8) << 3 | (p.charged as u8) << 4;
    w.u8(flags);
    w.varu(p.back_at as u64);
    write_option(w, p.down_since.map(u64::from));
    write_option(w, p.trespass_since.map(u64::from));
    let s = &p.score;
    for v in [s.kills, s.deaths, s.revives, s.team_kills, s.captures] {
        w.varu(v as u64);
    }
}

fn read_player(r: &mut Reader) -> Result<PlayerState, WireError> {
    let soldier = crate::read_soldier(r)?;
    let team = r.u8()?;
    let squad = read_option(r)?.map(|v| v as u32);
    let kit = r.varu()? as usize;
    let flags = r.u8()?;
    let back_at = r.varu32()?;
    let down_since = read_option(r)?.map(|v| v as u32);
    let trespass_since = read_option(r)?.map(|v| v as u32);
    let mut score = [0u32; 5];
    for v in &mut score {
        *v = r.varu32()?;
    }
    let [kills, deaths, revives, team_kills, captures] = score;
    Ok(PlayerState {
        soldier,
        team,
        squad,
        kit,
        spawn: spawn_at_from_u8(flags & 1),
        ready: flags & 2 != 0,
        bot: flags & 4 != 0,
        deployed: flags & 8 != 0,
        back_at,
        down_since,
        trespass_since,
        charged: flags & 16 != 0,
        score: Score { kills, deaths, revives, team_kills, captures },
    })
}

fn write_squad(s: &Squad, w: &mut Writer) {
    w.varu(s.id as u64);
    w.u8(s.team);
    w.varu(s.leader as u64);
    w.varu(s.members.len() as u64);
    for m in &s.members {
        w.varu(*m as u64);
    }
    w.u8(s.rally.is_some() as u8);
    if let Some(rally) = &s.rally {
        w.vec3(rally.pos);
        w.f32(rally.yaw);
        w.varu(rally.spawns_left as u64);
        w.varu(rally.expires_at as u64);
        write_option(w, rally.overrun_since.map(u64::from));
    }
    w.varu(s.rally_ready_at as u64);
}

fn read_squad(r: &mut Reader) -> Result<Squad, WireError> {
    let (id, team, leader) = (r.varu32()?, r.u8()?, crate::read_soldier(r)?);
    let mut members = Vec::new();
    for _ in 0..r.count(MAX_ITEMS)? {
        members.push(crate::read_soldier(r)?);
    }
    let rally = if r.u8()? != 0 {
        Some(Rally {
            pos: r.vec3()?,
            yaw: r.f32()?,
            spawns_left: r.varu32()?,
            expires_at: r.varu32()?,
            overrun_since: read_option(r)?.map(|v| v as u32),
        })
    } else {
        None
    };
    let rally_ready_at = r.varu32()?;
    Ok(Squad { id, team, leader, members, rally, rally_ready_at })
}

fn write_point(p: &PointState, w: &mut Writer) {
    write_option(w, p.owner.map(u64::from));
    write_option(w, p.claimant.map(u64::from));
    w.f32(p.progress);
    w.f32(p.rate);
    w.varu(p.since as u64);
    for count in p.inside {
        w.varu(count as u64);
    }
}

fn read_point(r: &mut Reader) -> Result<PointState, WireError> {
    let owner = read_option(r)?.map(|v| v as u8);
    let claimant = read_option(r)?.map(|v| v as u8);
    let (progress, rate, since) = (r.f32()?, r.f32()?, r.varu32()?);
    let mut inside = [0; TEAMS];
    for count in &mut inside {
        *count = r.varu32()?;
    }
    Ok(PointState { owner, claimant, progress, rate, since, inside })
}

/// Which spawn in a base is next is left out, because only a server spawns anyone.
pub fn write_state(state: &MatchState, w: &mut Writer) {
    w.varu(state.round as u64);
    w.varu(state.started_at as u64);
    match state.phase {
        Phase::Playing => w.u8(0),
        Phase::Ended { winner, restart_at } => {
            w.u8(1);
            write_option(w, winner.map(u64::from));
            w.varu(restart_at as u64);
        }
    }
    w.varu(state.teams.len() as u64);
    for team in &state.teams {
        w.vari(team.tickets as i64);
        w.varu(team.bleed_every as u64);
        w.varu(team.next_bleed as u64);
    }
    w.varu(state.points.len() as u64);
    for point in &state.points {
        write_point(point, w);
    }
    w.varu(state.chain.len() as u64);
    for &index in &state.chain {
        w.varu(index as u64);
    }
    w.varu(state.squads.len() as u64);
    for squad in &state.squads {
        write_squad(squad, w);
    }
    w.varu(state.players.len() as u64);
    for player in &state.players {
        write_player(player, w);
    }
}

pub fn read_state(r: &mut Reader) -> Result<MatchState, WireError> {
    let round = r.varu32()?;
    let started_at = r.varu32()?;
    let phase = if r.u8()? == 0 {
        Phase::Playing
    } else {
        Phase::Ended { winner: read_option(r)?.map(|v| v as u8), restart_at: r.varu32()? }
    };
    let mut teams = Vec::new();
    for _ in 0..r.count(TEAMS)? {
        let tickets = r.vari()? as i32;
        teams.push(TeamState { tickets, bleed_every: r.varu32()?, next_bleed: r.varu32()?, next_spawn: 0 });
    }
    let mut points = Vec::new();
    for _ in 0..r.count(MAX_ITEMS)? {
        points.push(read_point(r)?);
    }
    let mut chain = Vec::new();
    for _ in 0..r.count(MAX_ITEMS)? {
        chain.push(r.varu()? as usize);
    }
    let mut squads = Vec::new();
    for _ in 0..r.count(MAX_ITEMS)? {
        squads.push(read_squad(r)?);
    }
    let mut players = Vec::new();
    for _ in 0..r.count(MAX_ITEMS)? {
        players.push(read_player(r)?);
    }
    Ok(MatchState { round, started_at, phase, teams, points, chain, squads, players, next_squad: 0 })
}

/// A request from a player. Numbers in it are whatever the player likes: the match checks them.
pub fn encode_cmd(cmd: &ModeCmd) -> Vec<u8> {
    let mut w = Writer::default();
    match *cmd {
        ModeCmd::SetKit(kit) => {
            w.u8(0);
            w.varu(kit as u64);
        }
        ModeCmd::SetSpawn(at) => {
            w.u8(1);
            w.u8(spawn_at_to_u8(at));
        }
        ModeCmd::Deploy => w.u8(2),
        ModeCmd::CancelDeploy => w.u8(3),
        ModeCmd::JoinTeam(team) => {
            w.u8(4);
            w.u8(team);
        }
        ModeCmd::CreateSquad => w.u8(5),
        ModeCmd::JoinSquad(id) => {
            w.u8(6);
            w.varu(id as u64);
        }
        ModeCmd::LeaveSquad => w.u8(7),
        ModeCmd::PlaceRally => w.u8(8),
        ModeCmd::GiveUp => w.u8(9),
    }
    w.buf
}

pub fn decode_cmd(bytes: &[u8]) -> Result<ModeCmd, WireError> {
    let mut r = Reader::new(bytes);
    Ok(match r.u8()? {
        0 => ModeCmd::SetKit(r.varu()? as usize),
        1 => ModeCmd::SetSpawn(spawn_at_from_u8(r.u8()?)),
        2 => ModeCmd::Deploy,
        3 => ModeCmd::CancelDeploy,
        4 => ModeCmd::JoinTeam(r.u8()?),
        5 => ModeCmd::CreateSquad,
        6 => ModeCmd::JoinSquad(r.varu32()?),
        7 => ModeCmd::LeaveSquad,
        8 => ModeCmd::PlaceRally,
        9 => ModeCmd::GiveUp,
        _ => return Err(WireError::BadTag),
    })
}

/// Anything the server has to say that happened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WireEvent {
    Sim(SimEvent),
    Mode(ModeEvent),
}

const SIM_EVENT: u8 = 0;
const MODE_EVENT: u8 = 1;

pub(crate) fn write_event(event: &WireEvent, w: &mut Writer) {
    match event {
        WireEvent::Sim(event) => {
            w.u8(SIM_EVENT);
            events::write(event, w);
        }
        WireEvent::Mode(event) => {
            w.u8(MODE_EVENT);
            write_mode_event(event, w);
        }
    }
}

pub(crate) fn read_event(r: &mut Reader) -> Result<WireEvent, WireError> {
    match r.u8()? {
        SIM_EVENT => Ok(WireEvent::Sim(events::read(r)?)),
        MODE_EVENT => Ok(WireEvent::Mode(read_mode_event(r)?)),
        _ => Err(WireError::BadTag),
    }
}

fn position_of<T: PartialEq>(list: &[T], value: T) -> u8 {
    list.iter().position(|item| *item == value).expect("every value is listed") as u8
}

fn write_mode_event(event: &ModeEvent, w: &mut Writer) {
    match *event {
        ModeEvent::RoundStarted { round } => {
            w.u8(0);
            w.varu(round as u64);
        }
        ModeEvent::RoundEnded { winner } => {
            w.u8(1);
            w.varu(winner.map_or(NO_ID, |team| team as u64 + 1));
        }
        ModeEvent::PointNeutralised { point, by } => {
            w.u8(2);
            w.varu(point as u64);
            w.u8(by);
        }
        ModeEvent::PointCaptured { point, team } => {
            w.u8(3);
            w.varu(point as u64);
            w.u8(team);
        }
        ModeEvent::Deployed { soldier } => {
            w.u8(4);
            w.varu(soldier as u64);
        }
        ModeEvent::TeamKill { killer, victim } => {
            w.u8(5);
            w.varu(killer as u64);
            w.varu(victim as u64);
        }
        ModeEvent::RallyPlaced { squad } => {
            w.u8(6);
            w.varu(squad as u64);
        }
        ModeEvent::RallyLost { squad, why } => {
            w.u8(7);
            w.varu(squad as u64);
            w.u8(position_of(&RALLY_LOSSES, why));
        }
        ModeEvent::Refused { soldier, why } => {
            w.u8(8);
            w.varu(soldier as u64);
            w.u8(position_of(&REFUSALS, why));
        }
    }
}

fn read_mode_event(r: &mut Reader) -> Result<ModeEvent, WireError> {
    Ok(match r.u8()? {
        0 => ModeEvent::RoundStarted { round: r.varu32()? },
        1 => ModeEvent::RoundEnded { winner: r.varu32()?.checked_sub(1).map(|team| team as u8) },
        2 => ModeEvent::PointNeutralised { point: r.varu32()? as usize, by: r.u8()? },
        3 => ModeEvent::PointCaptured { point: r.varu32()? as usize, team: r.u8()? },
        4 => ModeEvent::Deployed { soldier: crate::read_soldier(r)? },
        5 => ModeEvent::TeamKill { killer: crate::read_soldier(r)?, victim: crate::read_soldier(r)? },
        6 => ModeEvent::RallyPlaced { squad: r.varu32()? },
        7 => ModeEvent::RallyLost { squad: r.varu32()?, why: *RALLY_LOSSES.get(r.u8()? as usize).ok_or(WireError::BadTag)? },
        8 => ModeEvent::Refused { soldier: crate::read_soldier(r)?, why: *REFUSALS.get(r.u8()? as usize).ok_or(WireError::BadTag)? },
        _ => return Err(WireError::BadTag),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn state() -> MatchState {
        let player = |soldier, team, squad| PlayerState {
            soldier,
            team,
            squad,
            kit: 2,
            spawn: SpawnAt::Rally,
            ready: true,
            bot: soldier == 1,
            deployed: true,
            back_at: 12345,
            down_since: Some(99),
            trespass_since: Some(50),
            charged: true,
            score: Score { kills: 3, deaths: 1, revives: 2, team_kills: 1, captures: 4 },
        };
        MatchState {
            round: 3,
            started_at: 100,
            phase: Phase::Ended { winner: Some(1), restart_at: 5000 },
            teams: vec![TeamState { tickets: -2, bleed_every: 720, next_bleed: 900, next_spawn: 0 }; TEAMS],
            points: vec![PointState { owner: Some(1), claimant: Some(1), progress: 0.25, rate: -0.001, since: 77, inside: [2, 1] }, PointState::default()],
            chain: vec![1, 0],
            squads: vec![Squad {
                id: 4,
                team: 1,
                leader: 2,
                members: vec![2, 1],
                rally: Some(Rally { pos: Vec3::new(1.0, 2.0, 3.0), yaw: 0.5, spawns_left: 3, expires_at: 800, overrun_since: Some(70) }),
                rally_ready_at: 600,
            }],
            players: vec![player(2, 1, Some(4)), player(1, 0, None)],
            next_squad: 0,
        }
    }

    #[test]
    fn a_state_comes_out_of_the_wire_as_it_went_in() {
        let state = state();
        let mut w = Writer::default();
        write_state(&state, &mut w);
        assert_eq!(read_state(&mut Reader::new(&w.buf)), Ok(state));
    }

    /// A server could name any soldier at all, and a client would make room for every one up to it.
    #[test]
    fn a_soldier_past_the_limit_is_refused_wherever_it_is_named() {
        let past = crate::limits().max_soldiers;
        let refused = |state: MatchState| {
            let mut w = Writer::default();
            write_state(&state, &mut w);
            read_state(&mut Reader::new(&w.buf))
        };
        let mut player = state();
        player.players[0].soldier = past;
        assert_eq!(refused(player), Err(WireError::TooMany));
        let mut member = state();
        member.squads[0].members.push(usize::MAX);
        assert_eq!(refused(member), Err(WireError::TooMany));
        let mut leader = state();
        leader.squads[0].leader = past;
        assert_eq!(refused(leader), Err(WireError::TooMany));
        let mut fine = state();
        fine.players[0].soldier = past - 1;
        assert!(refused(fine).is_ok());

        for event in [
            WireEvent::Mode(ModeEvent::Deployed { soldier: past }),
            WireEvent::Sim(prs_sim::SimEvent::Respawned { soldier: past }),
        ] {
            let mut w = Writer::default();
            write_event(&event, &mut w);
            assert_eq!(read_event(&mut Reader::new(&w.buf)), Err(WireError::TooMany), "{event:?}");
        }
    }

    #[test]
    fn a_full_match_is_small_enough_to_send_whenever_it_changes() {
        let mut state = state();
        let template = state.players[0].clone();
        state.players = (0..32).map(|soldier| PlayerState { soldier, ..template.clone() }).collect();
        let mut w = Writer::default();
        write_state(&state, &mut w);
        assert!(w.buf.len() < 1024, "{} bytes", w.buf.len());
    }

    #[test]
    fn every_refusal_and_every_request_survives_the_trip() {
        for refusal in REFUSALS {
            let event = WireEvent::Mode(ModeEvent::Refused { soldier: 3, why: refusal });
            let mut w = Writer::default();
            write_event(&event, &mut w);
            assert_eq!(read_event(&mut Reader::new(&w.buf)), Ok(event));
        }
        for cmd in [
            ModeCmd::SetKit(3),
            ModeCmd::SetSpawn(SpawnAt::Rally),
            ModeCmd::Deploy,
            ModeCmd::CancelDeploy,
            ModeCmd::JoinTeam(1),
            ModeCmd::CreateSquad,
            ModeCmd::JoinSquad(9),
            ModeCmd::LeaveSquad,
            ModeCmd::PlaceRally,
            ModeCmd::GiveUp,
        ] {
            assert_eq!(decode_cmd(&encode_cmd(&cmd)), Ok(cmd));
        }
    }

    #[test]
    fn nonsense_from_a_client_is_refused_and_does_not_panic() {
        for bytes in [&[][..], &[200], &[0], &[6, 0xFF], &[1]] {
            assert!(decode_cmd(bytes).is_err(), "{bytes:?}");
        }
        let mut next = 0x1234_5678u32;
        for _ in 0..2000 {
            next = next.wrapping_mul(0x9E37_79B1).wrapping_add(1);
            let bytes = next.to_le_bytes();
            let _ = decode_cmd(&bytes[..(next % 5) as usize]);
            let _ = read_state(&mut Reader::new(&bytes[..(next % 5) as usize]));
        }
    }
}
