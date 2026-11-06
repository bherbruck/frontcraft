//! Everything about a match that changes, as plain public data: all a server needs to send and all a
//! client needs to show. Nothing in here changes every tick. A clock is the sim tick it runs out on, and
//! a capture under way is where it was and how fast it is moving, so these only change when something
//! happens and a server only has to send them then. Anyone can work out how things stand on any tick.

use glam::Vec3;
use prs_sim::SoldierId;
use prs_sim::protocol::TICK_DT;

pub const TEAMS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnAt {
    Base,
    Rally,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Score {
    pub kills: u32,
    pub deaths: u32,
    pub revives: u32,
    pub team_kills: u32,
    pub captures: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerState {
    pub soldier: SoldierId,
    pub team: u8,
    pub squad: Option<u32>,
    /// An index into the mode's kits, for the next time they spawn.
    pub kit: usize,
    pub spawn: SpawnAt,
    /// Has said to go as soon as the wait is over.
    pub ready: bool,
    pub bot: bool,
    /// On the map at all. A soldier who has not gone in yet is on the spawn screen.
    pub deployed: bool,
    /// The sim tick they can come back on.
    pub back_at: u32,
    /// The sim tick they went down on, if they are down.
    pub down_since: Option<u32>,
    /// This life has already cost the team a ticket.
    pub charged: bool,
    /// The sim tick they went into the enemy's main base, if they are still there.
    pub trespass_since: Option<u32>,
    pub score: Score,
}

impl PlayerState {
    /// Seconds until they can come back, on this sim tick.
    pub fn wait_seconds(&self, tick: u32) -> f32 {
        self.back_at.saturating_sub(tick) as f32 * TICK_DT
    }
}

/// A squad's forward spawn, lying on the ground where the leader put it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rally {
    pub pos: Vec3,
    pub yaw: f32,
    pub spawns_left: u32,
    /// The sim tick it goes stale on.
    pub expires_at: u32,
    /// The sim tick enemies got the better of it, if they still have.
    pub overrun_since: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Squad {
    pub id: u32,
    pub team: u8,
    pub leader: SoldierId,
    pub members: Vec<SoldierId>,
    pub rally: Option<Rally>,
    /// The sim tick they can place another.
    pub rally_ready_at: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointState {
    pub owner: Option<u8>,
    /// Whose progress `progress` is. Whenever a point has an owner it is the owner.
    pub claimant: Option<u8>,
    /// From 0 to 1, how far `claimant` had it on sim tick `since`.
    pub progress: f32,
    /// How much of the way it is moving per tick, up or down, from then on.
    pub rate: f32,
    pub since: u32,
    /// How many of each team are in the zone and able to fight.
    pub inside: [u32; TEAMS],
}

impl PointState {
    /// How far `claimant` has it on this sim tick.
    pub fn progress_at(&self, tick: u32) -> f32 {
        (self.progress + self.rate * tick.saturating_sub(self.since) as f32).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TeamState {
    pub tickets: i32,
    /// Holding too few points, a ticket goes every this many ticks, the next on `next_bleed`. 0 means not bleeding.
    pub bleed_every: u32,
    pub next_bleed: u32,
    /// Which spawn in the base is next, so that soldiers do not come in on top of each other.
    pub next_spawn: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Playing,
    /// `winner` is `None` on a draw. The next round starts on sim tick `restart_at`.
    Ended { winner: Option<u8>, restart_at: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchState {
    /// Rounds begun so far.
    pub round: u32,
    /// The sim tick this round started on.
    pub started_at: u32,
    pub phase: Phase,
    pub teams: Vec<TeamState>,
    /// In the level's order, which is not the chain's.
    pub points: Vec<PointState>,
    /// The points in order from team 0's end to team 1's.
    pub chain: Vec<usize>,
    pub squads: Vec<Squad>,
    pub players: Vec<PlayerState>,
    pub next_squad: u32,
}

impl MatchState {
    pub fn player(&self, soldier: SoldierId) -> Option<&PlayerState> {
        self.players.iter().find(|p| p.soldier == soldier)
    }

    pub(crate) fn player_mut(&mut self, soldier: SoldierId) -> Option<&mut PlayerState> {
        self.players.iter_mut().find(|p| p.soldier == soldier)
    }

    pub fn squad(&self, id: u32) -> Option<&Squad> {
        self.squads.iter().find(|s| s.id == id)
    }

    pub fn squad_of(&self, soldier: SoldierId) -> Option<&Squad> {
        self.squad(self.player(soldier)?.squad?)
    }

    pub fn team_size(&self, team: u8) -> usize {
        self.players.iter().filter(|p| p.team == team).count()
    }

    pub fn humans(&self, team: u8) -> usize {
        self.players.iter().filter(|p| p.team == team && !p.bot).count()
    }

    /// The one point a team may attack: the first along the chain from its own end that it does not hold.
    pub fn attack_target(&self, team: u8) -> Option<usize> {
        let held = |i: &&usize| self.points[**i].owner != Some(team);
        if team == 0 { self.chain.iter().find(held).copied() } else { self.chain.iter().rev().find(held).copied() }
    }

    /// The point a team holds furthest along the chain, which is the next one the other side can take.
    pub fn front_point(&self, team: u8) -> Option<usize> {
        let held = |i: &&usize| self.points[**i].owner == Some(team);
        if team == 0 { self.chain.iter().rev().find(held).copied() } else { self.chain.iter().find(held).copied() }
    }

    pub fn points_held(&self, team: u8) -> usize {
        self.points.iter().filter(|p| p.owner == Some(team)).count()
    }

    pub fn ended(&self) -> bool {
        matches!(self.phase, Phase::Ended { .. })
    }
}

/// What a player asks of the mode, as opposed to what they ask of their soldier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeCmd {
    SetKit(usize),
    SetSpawn(SpawnAt),
    /// Go as soon as the wait is over.
    Deploy,
    CancelDeploy,
    JoinTeam(u8),
    CreateSquad,
    JoinSquad(u32),
    LeaveSquad,
    PlaceRally,
    /// Stop waiting for a medic.
    GiveUp,
}

/// Why the mode said no.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    RoundOver,
    NoSuchKit,
    KitTaken,
    NoRally,
    NoRallyRules,
    SquadFull,
    NoSuchSquad,
    TooManySquads,
    OtherTeam,
    InTheField,
    TeamsUneven,
    NotLeader,
    NotUp,
    NotOnGround,
    RallyCooling,
    NoSquadmateNear,
    EnemyNear,
    TooCloseToBase,
    InAZone,
    NoTickets,
    NotDownLongEnough,
}

/// How a rally point came to be gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RallyLoss {
    Overrun,
    Expired,
    UsedUp,
    Replaced,
    SquadGone,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModeEvent {
    RoundStarted { round: u32 },
    RoundEnded { winner: Option<u8> },
    PointNeutralised { point: usize, by: u8 },
    PointCaptured { point: usize, team: u8 },
    Deployed { soldier: SoldierId },
    TeamKill { killer: SoldierId, victim: SoldierId },
    RallyPlaced { squad: u32 },
    RallyLost { squad: u32, why: RallyLoss },
    Refused { soldier: SoldierId, why: Refusal },
}
