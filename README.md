# FrontCraft

A Project Reality style tactical shooter in Rust and Bevy, for desktop and web, played alone or
on a dedicated server (see [Multiplayer](#multiplayer)). You fight over a low-poly town. There is also a training range for getting movement and shooting
right: `PRS_MAP=range cargo run --release`, or add `?map=range` to the address in a browser.

## Run it

```sh
cargo run --release                  # desktop
./scripts/build_web.sh [out dir]     # web, output in bin/web unless told otherwise
python3 -m http.server -d bin/web 8080
```

All build output goes to `bin/`, which is not tracked.

The web build wants `wasm-opt` (binaryen) and `brotli` on the path, and warns if either is missing. It
writes `.br` and `.gz` files beside the wasm and script. A plain `http.server` ignores them and sends
about 20 MB. `scripts/serve_web.py bin/web 8080` sends them, about 5 MB, and so will nginx with
`brotli_static on; gzip_static on;` or Caddy with `file_server { precompressed br gzip }`. Whatever
serves it has to send the wasm as `application/wasm` with `Vary: Accept-Encoding`.

## Controls

| Input | Action |
| --- | --- |
| WASD | Move |
| Shift | Sprint, or hold breath while aiming |
| Space | Jump. With W held, facing something you can get up: vault, mantle or pull up onto it, climb through a window, or climb onto a crouched soldier's back. A faint line marks an edge you can take from where you are |
| X / Z | Crouch / prone |
| Q / E | Lean left / right. Again to come back, the other key to swap. Standing or crouched, not sprinting |
| C | Set the gun down on whatever is under or against the fore-end, a sill, wall top, crate or truck, from any stance. Again to lift it |
| Alt | Hold to look around without moving the gun: the mouse turns your head, up to about 120 degrees either way, and it comes back when you let go |
| 1 2 3 4 in a seat | Move to that seat, if it connects to yours |
| WASD in the driver's seat | Throttle, brake and reverse, steering. S is the brake |
| Space in the driver's seat | Handbrake: locks the back wheels, so with the wheel over the tail comes out. Holds it still when stopped |
| Q / E in the driver's seat | Change down / up a gear. Automatic until you do, then yours until you leave the seat |
| In a helicopter's pilot seat | W / S collective, A / D pedals, mouse cyclic, Space starts or stops the engine, Alt looks around |
| Left mouse | Fire |
| Right mouse | Tap: raise or lower sights. Hold: zoom in for as long as you hold, with or without sights |
| R / V | Reload / fire mode |
| G | Take out a frag, again for smoke. Hold left mouse and let go to throw, with right mouse held for underarm |
| 1 2 3 4 | On foot: primary, sidearm, bandage, medkit. On the spawn screen 1 to 9 choose a kit |
| B | Hold to bandage yourself, on foot or in a seat. You can walk while you do, at the pace of aiming in whatever stance you are in (`bandage_on_the_move`). The bleeding waits from the moment you go for the dressing until it is on or you stop, so nobody goes down halfway through one (`bandage_holds_bleeding`). Going through a door or across to another seat stops it, and so does being hit |
| Left mouse with 3 or 4 selected | Hold to treat an ally in front of you, or yourself if there is none |
| F | Whatever you are at and facing: a kit on the ground, a vehicle's door or hatch. In a seat, get out. Hold it at a vehicle on its side or roof to right it, with others to go faster |
| I | What you carry |
| M | Map: the points, your squad and you, and nothing else |
| Hold Tab | Scoreboard |
| Hold Caps Lock, H or Mouse 4 | Talk out loud, for anyone near. In the browser, with the microphone turned on in the pause menu |
| Hold Y or Mouse 5 | Talk on the squad radio, likewise |
| Enter | Squad and kit for your next spawn. On the spawn screen, deploy (mouse or number keys choose a kit). When down, and after a while, give up waiting for a medic |
| T | Place a rally point, if you lead a squad |
| `[` `]` | Mouse sensitivity |
| Esc | Pause menu: FOV, zoom FOV, sensitivity, volume, effects volume, microphone, voice volume, and muting anyone you can hear. They stay set between games |
| F3 | Tuning overlay, with a dot on the gun's true aim point |
| F4 | Watch your own soldier and gun from outside |

A key only counts when the game has the mouse. In the pause menu nothing reaches your soldier, and a key held through a menu or the spawn screen has to come up before it counts again. On the spawn screen the mouse is a pointer and only Enter, the number keys and Tab do anything. While down you can look around, read the map and the scoreboard, and give up with Enter.

## Crates

Dependencies only point downwards. Nothing below the line knows Bevy exists, so the simulation
runs in plain unit tests and can run unchanged on a dedicated server.

| Crate | Job |
| --- | --- |
| `prs_game` | The app. Wires the plugins together and nothing else. |
| `prs_input` | Devices to player intent. |
| `prs_view` | Camera rig and the rifle in your hands. |
| `prs_scene` | Draws the level, targets, soldiers, tracers and impacts. |
| `prs_atmosphere` | The look: sun, sky, fog, tonemapping, and what gets baked into vertices. One `Atmosphere` resource. |
| `prs_audio` | Procedural sound effects: each shot rendered for its barrel direction, distance, side and any walls in the way. |
| `prs_hud` | Minimal HUD and the tuning overlay. |
| `prs_models` | Low-poly models shared by those views: the rifle and the soldier's segments. |
| `prs_runtime` | Hosts the sim in Bevy, or a connection to a server. The only crate the five above share. |
| --- | --- |
| `prs_mode` | The rules of a match over the sim: sides, squads, spawning, tickets, objectives. |
| `prs_sim` | One tick of the whole game: commands in, state and events out. |
| `prs_movement` | Character movement. |
| `prs_weapons` | The table of guns. Recoil, gun instability, sway, dispersion, fire control, magazines. |
| `prs_inventory` | What a soldier carries, as plain data: guns, magazines, bandages, kits, weight. |
| `prs_health` | Health, bleeding, wounds, going down and dying. Knows nothing about guns or bodies. |
| `prs_ai` | Soldiers walking about on their own. Sends `PlayerCmd`s like anyone, from a seed. Sits on `prs_mode` and `prs_sim`. |
| `prs_body` | A soldier's skeleton: bones and animation worked out from state, hands on the gun by IK. |
| `prs_ballistics` | Bullet flight. |
| `prs_vehicles` | Ground vehicles: a rigid chassis on ray suspension, tyres, engine, panels, damage. Knows no soldiers. |
| `prs_helicopter` | A helicopter's flight model: rotor, engine, tail rotor, skids, what breaks. Knows no soldiers. |
| `prs_seats` | Seats in anything that carries people: where the eye and hips go, what can be seen and shot from there, who sits where. |
| `prs_level` | Map data, shared by collision and rendering. |
| `prs_collision` | Static geometry queries. The only crate that sees parry. |
| `prs_protocol` | `PlayerCmd` and the tick rate: what a client will send a server. |

`prs_movement`, `prs_weapons`, `prs_health` and `prs_body` do not depend on each other. `prs_sim`
connects them. `prs_inventory` sits on `prs_weapons`, because guns are what it carries.

## Light and sky

Late afternoon, set by one `Atmosphere` resource in `crates/prs_atmosphere/src/lib.rs`. A mod that wants
another time of day inserts its own before startup. Nothing in it moves while you play.

- **Light** is a low warm sun that casts the shadows, a blue sky fill and a warm ground bounce that cast
  none, so a shadowed facet still has its own colour. WebGL2 draws one directional light in Bevy, and a
  fill would put out the sun, so there the fills are folded into ambient, with a little of their direction
  baked into vertex colours (`face_tint`). Shaded sides are flatter on the web, and it has one shadow
  cascade reaching 110 m.
- **Sky** is one unlit mesh that follows the camera: a gradient, a sun, hazy far ridges and faceted
  clouds. At the horizon it is exactly the colour of fully fogged land at that bearing, worked out from
  the same sum as the fog shader, so distant hills melt into it.
- **Baked light** goes into vertex colours when the level is built (`crates/prs_scene/src/bake.rs`). A
  room is lit by how much of its doors and windows each point can see and darkens into corners, and walls
  darken towards their feet. Those faces are cut into tiles to hold the gradient.
- **Tonemapping** is `SomewhatBoringDisplayTransform`, which needs no lookup table: the game is built without
  Bevy's `tonemapping_luts`, so TonyMcMapface, AgX and BlenderFilmic would not work. It is not a mod setting,
  so a mod asking for one is told there is no such setting. There is no HDR or bloom.

## Game modes

`PRS_MODE=aas|tdm|practice`, or `?mode=` in a browser. The town defaults to `aas` and the training range to
`practice`. `prs_mode` knows nothing of Bevy, a screen or a network. A host calls `Match::step(&mut sim, &cmds)`
straight after `Sim::step`, so a dedicated server runs it the same way, and `Match::export_state` and
`import_state` go with the sim's.

A mode is a `ModeConfig`, which is plain data (`crates/prs_mode/src/config.rs`), and an `Objective`, a small
trait for whatever makes it more than a fight. To add one, combine what is there or implement `Objective`,
keep anything it has to remember in `MatchState`, and name it in `Mode::named`. No number here is hardcoded.

- **Advance and Secure** (`aas`): the capture points of a map form a chain, ordered by how much nearer each is to one base
  than the other, or as given in `CaptureRules::chain`. A team can only attack the first point along the chain from its
  own end that it does not hold. Whoever has more soldiers on their feet in a zone moves it: a held point goes down to
  nobody first and then up for the attacker, faster with a bigger advantage. Even numbers stall it. The flag on the pole
  goes up and down to match, so you can read a point from a long way off. Each team has tickets. Going down or dying
  costs one, once, and a medic reviving someone hands it back, so a revive is worth more than the health. The side
  holding fewer points bleeds tickets for each point it is behind by. Hitting your own side costs a penalty on top: bullets
  hit everyone. Out of tickets, or behind when the time runs out, is a lost round, and the next one starts clean on the same
  sides and squads.
- **Team Deathmatch** (`tdm`): the same harness and nothing to hold.
- **Free Practice** (`practice`): the sim as it is, with no sides that matter, no ending and no spawn screen.

**Squads** take six, with a leader. A player takes over from a bot, and a squad that loses its leader gets the next
player. **Kits** come from the mode, each a `prs_inventory` loadout with a limit per squad: rifleman, medic, machine gunner,
marksman. Only medics carry medkits, so only they can get someone up, unless someone picks a kit up off a dead medic.

**Over a network** the server decides everything about a match. A client's copy of `MatchState` is for showing, and everything
a player does here is a `ModeCmd` request that the server checks: which kit, which spawn, which squad, placing a rally point.
A request with a kit, squad or spawn that isn't allowed is refused with a `Refusal`, and a rally point goes where the server
says the leader is standing. `MatchState` holds no clocks that tick. Timers are the sim tick they end on, and a capture under way is
`progress`, `rate` and `since`, so a match at rest or mid-capture sends nothing, and clients work out how far it has got
(`PointState::progress_at`). `prs_server --mode aas|tdm|practice` hosts one with no Bevy, bots included.

**Main bases**: going into the other side's main base gets you a warning and a clock, and if you are still there when it runs out
you die, which costs a ticket like any death and is nobody's kill. Nobody in a base is protected from bullets. Only the server decides
this and bots obey it. `BaseRules` has the clock and how high it counts, and each map says how big its bases are (`Team::base_radius`).

**Rally points**: the squad leader puts one down with T as a forward spawn. It costs tickets and has a cooldown, and it
needs a squadmate close by, no enemy near, and to be clear of capture zones and away from your base. It takes a
number of spawns and lasts a few minutes. Spawning on it takes longer than at the base. Enemies outnumbering friends on top
of it for a few seconds destroy it. All of this is `RallyRules`.

**On screen**, in the spirit of Project Reality: no crosshair, no hit markers, no kill feed and no enemy positions, ever. There
is a spawn screen when you are off the field, a map, a scoreboard, and the status of a zone only while you are in it.
Uniforms are by team (`TeamLooks` in `prs_models`). So that you can tell friend from foe, a friend gets a small coloured
mark above them only when you are looking almost straight at them with a clear line (`IdentifyRules`: 4 degrees, 8 for your
squad, out to 300 m). That answers "is that one of ours?" just before you shoot and says nothing about anyone you were
not already looking at. There is no mark on an enemy, so no mark tells you nothing for certain.

**Bots** keep the sides the same size, every tick: each side has `bots_per_team` soldiers, or as many as the side with more
players if that is more, and the side with fewer players gets the bots. A bot makes way for a player who joins, and comes back
when they leave or drop out, mid-round. One off the field or dead goes first; one on its feet only goes when no player is within
`bot_unseen_distance`, so nobody sees a soldier vanish. `bots_most` caps them and `no_bots_from_players` turns them off once enough
people are playing. `PRS_WALKERS=<n>` or `?walkers=<n>` overrides the number. They are `prs_ai` soldiers, and all they hear from the
mode is `Match::order_for`: which point their squad should head for.

The usual bot is `SkillConfig::easy`: slow to see and react, unsteady in its aim and poor at recoil, range and lead, which is about
half as dangerous as a regular at middle range and still dangerous close in. A server can run `PRS_MOD=bots-normal` (the old
regular) or `PRS_MOD=bots-hard` (veteran). Like everyone, a bot only misses because its gun points somewhere else.
`cargo test -p prs_ai skill_report -- --ignored --nocapture` prints hit rates at 25, 75 and 150 m.

What a bot sees is worked out point by point: head, chest, hips, shoulders, knees and the gun, a few rays a look. Walls and
smoke stop a look, and tree canopies let less through the more of them it crosses. Seeing someone takes a while, faster up
close, the more of them shows, standing and moving, and much faster when they fire. A sliver of someone, a soldier in the
shade of a room away from its windows, or someone lying still past `still_hidden_beyond`, is seldom or never picked out. A bot
keeps someone in sight only while enough of them shows, and aims at what shows. All of it is `SightConfig` and `SkillConfig`.
`cargo test -p prs_ai sight_report -- --ignored --nocapture` prints how long each case takes.

**Changing sides** is free while there are few players. From `balance_from_players` (6) the side being joined may only be
`team_balance_margin` ahead in players. Bots never count.

## How shooting works

The gun is a real object in the world. `gun_pose` in `prs_weapons` works out where it is from a
soldier's state, the same way for the shooter, the simulation and anyone watching. Bullets leave
from its muzzle, and its barrel collides with walls: it pulls back, and if that is not enough it
comes down and cannot fire.

A bullet leaves along the gun's axis, give or take the weapon's mechanical dispersion: a small fixed
cone set per weapon (`dispersion_moa`) that nothing the shooter does can widen. Nearly every miss is
the gun visibly pointing somewhere else. Most of that movement is shown on the rifle rather than the
screen, and the front sight marks where the shot will go.

- **Recoil** is a kick on a spring. The random part is larger than the predictable part and grows
  with sustained fire, and shots arrive faster than anyone can react.
- **Instability** makes the gun wander. Firing, moving, turning and changing stance unsettle it, and
  it calms down slowly once you stop.
- **Sway** is breathing. It is reduced by stance and by holding your breath, and overdoing the hold
  makes it worse.

Those goals are tests in `crates/prs_sim/src/tests.rs`, so a tuning change that allows mowing down a
squad fails the build. Tuning values are in `crates/prs_weapons/src/config.rs` and `MoveConfig` in
`crates/prs_movement/src/lib.rs`. To see hit rates by range after a change:

```sh
cargo test -p prs_sim balance_report -- --ignored --nocapture
```

### Leaning

Q and E lean left and right, and it is a toggle: the same key comes back, the other swaps side. You can
lean standing or crouched, and walking, slowly (`LeanConfig::max_speed`). You cannot while prone or
sprinting, sprinting ends a lean, going down ends it, and in the air you come upright and carry on when you
land. Reloading, aiming and treating are all fine.

The lean is real for everyone: the eye swings over about the waist (`Lean` in `prs_movement`), `gun_pose`
carries the gun with it and rolls it about the line through the eye, and the body's hips, spine and head go
over with the hands on the gun by IK. Bullets leave from that muzzle and hit bones where they are, and the
sight's zero follows the roll. A lean goes only as far as fits: a ball is swept out from the upright eye, and
the eye stays out of whatever that finds, with the lean easing short of it. Going over unsettles the gun like
a change of stance (`changing_lean`), and a lean that has arrived costs no steadiness. There is nothing else
to it, so what it buys you is what it looks like it buys: a view round a corner with most of you behind it.

It runs on a spring with a limit on how hard it can pull, so the eye never snaps. The screen rolls only
`ViewSettings::lean_roll`, about 3 degrees, much less than the gun and body, and that can go to zero.

### Tuning sounds

`/tune/` on the web build is a page of sliders over the game's own footstep code (`crates/prs_synth`, by way of `crates/prs_sound_lab`), at the game's volumes, with a rifle to compare against. A second tab does the slides: stance changes, crawling, sights, climbing, the gun along a wall, changing gun, magazines, kit shifting as you walk, and handling. What it gives you is a mod: the `[audio.steps.*]`, `[audio.slides.*]` and `step_volume` settings. If the rifle's sound changes, `cargo test -p prs_audio write_tuner_reference_shot -- --ignored` writes a new `web/tune/rifle.wav`.

### Climbing and standing on each other

Hold W and press Space at something. There is no key of its own, because with nothing to climb it is the jump it always was, and a wall you have your back to is still a wall to jump next to. Up to about the waist
(`vault_height`) is a quick hop with one hand, to about the chin (`mantle_height`) both hands and over, and at the limit of reach (`max_height`, 2.25 m) a pull-up that ends crouched. A thin wall can't be stood on, so it is
something to go over and down the far side, up to chin height. All of it is `ClimbConfig` in `crates/prs_movement/src/climb.rs`.

**Windows and other low gaps.** Where there is no room to go through upright, as there is not between a town window's sill and its lintel, you go through flat: the body goes down on the way up to the sill, through the gap, and gets up again on the far side (`squeeze`, `squeeze_time`, `squeeze_stand`). Any gap about 0.75 m high and 0.65 m wide will do, and a smaller one is not tried. Every ground floor window in town is tested both ways, standing and crouched, in `crates/prs_level/src/climb_tests.rs`. Out of an upstairs window is a drop, and is refused.

**What you can climb.** A faint thin line lies along an edge you could climb from where you stand, so that a jump is not spent on one you cannot. It comes from `Sim::climb_on_offer`, which asks the same planner a press of jump does with everything the press checks, so a lit edge is one jump with W held will climb, and nothing else is lit. It is asked a few times a second and fades in and out. `view.climb_hint` in a mod turns it off or changes its range, strength, colour and rate.

A climb is worked out once as a path and then followed, with the height and the eye on quintic curves, so there is no jolt in it. You are committed and exposed for as long as it takes: the gun comes down through `ready`, won't
fire, won't aim and won't reload. It is looked for with rays and tried out with the body's own capsule, all along the path, so you can't climb through a wall or a roof, or into anywhere that doesn't fit you. Only an abrupt rise
counts as a ledge, so no slope is ever climbed, which is what keeps the rim of the map shut. Each tick looks again, and if something has got in the way you let go and fall.

Press a little too far from it and you jump, and with W still held you catch the ledge when you get there. A jump never adds reach: in the air the limits are measured from the ground below you.

It costs stamina, and with too little it still happens, leaves you with none and takes up to twice as long (`winded_time`). You come out of it with no speed at all, and a wait of `cooldown` before the next, so a staircase of ledges is well behind the stairs. Not while prone, down, in a seat,
reloading or treating. Wounds and weight take off the height and add to the time (`wound_reach`, `burden_reach`), so a machine gunner can't do the highest pull-up and a badly wounded leg can't climb at all.

**Standing on someone.** A crouched soldier's back holds one person, friend or enemy, who climbs on the same way. From up there you reach further by the height of the back and a shove (`perched_boost`), so two can get up
what one can't: tin roofs, stall awnings, the tops of walls, a few tiled roofs, and over a 2.3 m wall. Someone on a back doesn't move with it. Standing up, moving past `slack`, going prone or going down drops the rider, with no more than a small nudge. They can't
walk at any speed with you on, and no one can be stood on who is standing, lying down, down, on someone else or already has someone on them, so there are no towers and no jumping on heads. All that is `StoolConfig`.

**Roofs and capture points.** Somewhere that took a back to reach doesn't hold ground: no one up there counts towards taking a point or stalling one, until they are back down on the land (`MoveState::boosted`,
`CaptureRules::boosted_soldiers_count`). Otherwise one soldier could take a point alone from a roof, or stall it for ever from where nobody on foot can get at them. It is still a fine place to shoot from. Someone on a
back in the street still counts, and so does anywhere one soldier can get to alone, as the other side can get there too.

**Bumping into each other** (`crates/prs_movement/src/crowd.rs`). Soldiers don't pass through each other but they can't block each other either. Overlapping someone limits your speed in every direction and moves you both
apart a little, with no speed in it. Squeezing past is always faster than the push, so nobody can hold a door shut, however hard they lean, and nobody gets launched. Someone lying down, down or seated can be stepped over or past. `CrowdConfig`
has it, and at 64 soldiers it costs about a hundredth of a millisecond a tick, because they are taken down once a tick in a list sorted by one axis.

**Over a network.** A climb on a wall is your own and is predicted exactly, with no reconciles. Everything to do with another soldier goes by what the wire can agree on: where they are is rounded to the wire's own grid on both
sides, so a client looking at someone keeping still has the same numbers as the server, and climbing on a still back doesn't disagree either. Someone walking is looked for where they will have got to by now (`CrowdConfig::look_ahead`).
A rider's place depends on nothing but themselves until they are let go. So a back standing up costs one correction no bigger than the fall, faded out, and walking into people costs corrections of a few centimetres, up to a quarter of a metre
sprinting past someone, and half a metre in the moment two people running head on pass each other, which you see as about half a step's worth more in a frame. It is all measured at 100 ms each way in `crates/prs_net_sim/src/climb_tests.rs`. To see what climbing does to the town:

```sh
cargo test -p prs_level climb_report -- --ignored --nocapture
cargo test -p prs_sim crowd_cost_report -- --ignored --nocapture
```

Anything over 2.4 m in the town takes two people, and no two-storey roof or flat roof behind a parapet can be got to from the ground. Bots go by `Sim::climb_ahead` to ask what a press would give them.

### Resting the gun

C sets the gun down, from standing, crouched or lying down: rays from where the gun can really get to look for something level under the fore-end, anywhere from as low as the body goes to as high as it goes, so from a low sill to about chin height (`RestConfig` in `crates/prs_weapons/src/config.rs`, per gun, so `weapons."*".rest.low_eye` in a mod). If there is nothing, nothing happens and it says so. From then on that point stays where it is and the mouse swings the barrel about it, with the gun's underside `clearance` above the contact.

`gun_pose` is exactly what it always was, out from the eye to the sights, so the sights stay on the eye and bullets leave the same way. What changes is where the eye is: it goes wherever puts the fore-end over the contact (`GunRest` in `crates/prs_sim/src/rest.rs`). So aiming up over the rest takes the body down and aiming down brings it up, and while rested a soldier's height is continuous and not a stance. It is a real height: `held_capsule` for walls and ceilings, hips and knees and hit zones from the eye as ever, the same for everybody watching. It gets there at a pace a head moves at (`ease_time`, `ease_per_metre`, and never faster than `eye_speed`). Coming off goes to whichever stance is nearest. A stance key while rested takes you off and does what it says.

The swing stops for what really stops it, softly and with nothing to unwind: the arc (`arc_yaw`, `arc_up`, `arc_down`), the body (`low_eye` kneeling, standing tall on the ground beneath you, a ceiling, something under a knee, and `prone_high_eye` for someone lying down), and the barrel having to point clear of what it is resting on, the whole way to where that ends (`clear_reach`, `clear_margin`, `clear_wander`). That last one is because a bore is below its sights, so a line of sight just clearing a sill or a truck's bonnet was a round into it. It means a low target close in over a long bonnet can be out of reach, as it would be for a real gun lying there. `Sim::rest_arc` is one sum for all of it, read by the client's mouse and by the sim. Stopping there is yours and not a rule: it is `Rest stops at arc` in the Esc menu, saved with the other settings, with its default in `view.rest_stops_at_arc` for a mod. The sim's own rule is that past those limits it comes off, which is what turning it off gets you, and all a modified client could ever get. Alt still looks round with the head alone.

Windows are the point of it. Crouched up against a window in town a free gun has its barrel in the sill and won't fire, and a gun on the sill has its muzzle out through the opening. That is measured on all of the town's windows in `crates/prs_sim/src/rest_tests.rs`. It doesn't let you shoot through anything: a barrel swung into a frame stops firing as it does anywhere.

It gives you less sway, less wander and less kick, through those same systems, with `dispersion_moa` untouched, but only once it has settled (`settle_time`), and each shot knocks that back (`shot_unsettles`). So it is for picking someone off, and holding the trigger down from it does not take out a group. Moving away, sprinting, a stance key or leaning, changing guns, going down or what was under it going away all lift it, smoothly. A rest doesn't go along with what it is on, so a truck driving off lifts it, whether it was beside you or you are riding it.

To see what it does to shots at a rest: `cargo test -p prs_sim rest_report -- --ignored --nocapture`.

## Weapons, kit and magazines

Guns are rows in a `WeaponTable`, and a soldier refers to one by a plain `WeaponId`. There is an assault
rifle, a machine gun, a marksman rifle and a pistol, all with the same handling model: each has its own
recoil, instability, sway, `dispersion_moa`, hold, grips, ballistics, damage, weight and report. Add a row
and pick a `GunShape` to add a gun.

A soldier carries a primary, a sidearm, bandages and a medkit. Changing guns brings the gun all the way
down, swaps it there, and brings the other one up, all through `ready` in `gun_pose`. Each spare magazine
is tracked with what it holds. Reloading takes the fullest one and keeps the old one with its rounds, so
reloading early costs you full magazines later. The HUD gives only a feel for the magazine in the gun.
Weight past what a standard rifleman carries slows you a little and drains sprinting stamina.

Dead soldiers drop everything in a kit. F swaps your primary for the kit's, or takes its ammunition if it is
the same gun, and fills your pouches from it. Your old gun stays in the kit for someone else.

## Damage, wounds and healing

A soldier is hit on the bones of the pose they are in, as one capsule per bone, tested against each tick's
stretch of bullet flight. So someone behind a wall or lying down is hit exactly where they look. Damage is
the gun's, scaled by the bullet's remaining speed and then by the body part (`HealthConfig` in
`crates/prs_health/src/lib.rs`). Bones are only worked out for soldiers a bullet actually comes near.

- **Bleeding** starts with a hit, at a rate by body part, and drains health until someone stops it.
- **Wounds** take effect through the same inputs as everything else. A leg wound slows you and takes your
  sprint. A wounded arm and low blood feed instability and sway. The gun visibly shakes, and the weapon's
  fixed cone stays the same. There is no state-based deviation, and no camera shake or screen flash for wounds.
- **Down** at no health: on the ground, unable to act, bleeding out on a clock, able to be revived. Head
  shots and hits far past zero kill outright. A bandage slows the clock, and being shot again shortens it or
  ends it.
- **Treatment** means selecting a bandage or medkit and holding fire. The gun goes onto its sling, and you see
  your hands take a roll of gauze to the wound and work it on for exactly as long as the treatment takes, or to
  your forearm held up in view if the wound is somewhere you can't see. Everyone else sees the same thing, a
  dressing stays on an arm or leg until you respawn, and treating an ally has your hands reach out to them.
  A bandage on yourself carries on as you walk, at the pace of aiming, with your hands working it while your
  legs keep going. Anything else takes seconds of standing still. Moving, letting go or being hit starts it over.
  A bandage stops bleeding, and a medkit
  restores health and takes away most of a wound, or revives someone who is down.
- **Respawn** comes when the game mode says, with whatever kit it says (see [Game modes](#game-modes)). A sim
  with no mode brings a soldier back at their home spot after `respawn_time`.

All of this is tests in `crates/prs_sim/src/tests.rs`: one magazine of spray does not take out a squad at
range, head shots matter, and a bandage only saves you in cover.

## Being shot at

A bullet going past close by leaves a soldier suppressed, by how near it came, the size of the round and
whether it cracked. A wall between you takes most of it. Friendly fire counts and your own does not, and it
wears off slowly (`SuppressionConfig` in `crates/prs_suppression/src/lib.rs`, with `enabled` for mods). It shakes
the real gun through the same instability and sway inputs as a wound, and the weapon's cone stays the same. The
view closes in with a dark ring and loses some colour, with no camera movement (`SuppressionLook` in `prs_view`),
and heavy suppression dulls your hearing. Losing blood looks nothing like it: a squared-off red frame that hugs the edges of the screen and beats in time with your heart, leaving the colour of the world alone. It has three plain levels, by how long you have left: bleeding, bleeding heavily and bleeding out, each stronger, further in and faster than the last, with a bar of your blood and `down in N s` by the stamina bar. A bandage going on eases it, and when it is on the red fades and the readout says so (`BleedLook` in `prs_health`, modded as `view.bleed`). You hear the crack or whizz from the point the bullet actually passed.
The server decides it, and a client only predicts it wearing off.

## How bodies work

A body is derived and never decides anything. `solve` in `prs_body` takes a soldier's state and gives
nineteen world-space bones, the same way for you and for anyone watching, so a server could compute
it too. The eye and the gun are inputs: the sim says where they are and the skeleton fits itself
between them and the ground. That is why animation can never move your camera or your aim. Between
ticks the body remembers only a little (`BodyAnim`: where the hips face, the landing dip), and the
sim steps that from its own state.

Hands reach the gun by two-bone IK, feet are placed by distance walked so that they do not skate, and
the torso twists between the look and the hips. In first person your soldier is the same skeleton
with the head moved to a layer the camera does not see, so your shadow keeps it, and it fits the rifle
the view actually drew, so hands stay on it through the view's cosmetic springs.

A soldier who goes down falls into a lying pose by blending every bone towards it (`lying` in `solve`).
That pose listens to nothing but the feet and the way they fell, so the look can go anywhere without the
body following, and bullets hit it where it lies.

Bones are rigid low-poly segments rather than a skinned mesh. Facets are the look anyway, and it costs
a transform per bone and nothing WebGL2 might lack. Proportions and all animation tuning are in
`crates/prs_body/src/config.rs`.

## Vehicles

A vehicle is one rigid body held up by a ray per wheel, stepped at 120 Hz with no idea what a soldier or a
renderer is. Tyres are velocity limits applied where the rubber touches rather than springs, so a parked
vehicle comes exactly to rest and then goes to sleep. Weight transfer, body roll and going over on a steep
slope are written nowhere: they follow from forces acting below the centre of mass. All of it is in
`VehicleConfig` in `crates/prs_vehicles/src/config.rs`, and the utility truck's feel is held to numbers in
`crates/prs_vehicles/src/tests.rs`: top speed, stopping distance by surface, what it climbs and what it does
not, no rolling on the flat, no sliding when parked on a slope, and a real ditch being the end of the trip.
For the numbers:

```sh
cargo test -p prs_vehicles handling_report -- --ignored --nocapture
cargo test -p prs_sim drive_cost_report -- --ignored --nocapture
```

You get in by going to it. Each seat is reached through a door, hatch or similar (`Entry` in
`crates/prs_seats/src/lib.rs`): stand there, face it, and **F** takes you through it in a few seconds, in the
open with the gun down and the door swinging out of your way, and anyone can shoot you on the way. Being hit turns you back. A door against a wall does
not open, and a door whose seat is taken does not open. **1 to 4** in a seat move you to another, taking a moment and
only where the seats connect (`SeatLink`): across the truck's cab or across its back, but not from one to the
other, and round the inside of the armoured car. Nobody changes kit in a seat, which is why the number keys can be
the seats there. The armoured car's gunner gets in at the hatch on the roof, which means climbing up from the
step at the back and walking along the deck. Hulls are solid to walk into and stand on. **F** is one key for what you
are at and facing, so a door and a kit side by side go by which you face, and the prompt says which. Q and E
lean, and in a seat do not. The driver has **W A S D**. W and S stop it first and then go the other
way, Space is the handbrake whatever else is pressed (`Brakes::handbrake_rear`: steer and hold it and the back lets go, too long and it spins, and let go in time it can be caught), and with no input and under walking pace it holds itself still
on its brakes. A look in a seat goes with the seat: its turning exactly and its tilt by way of `RideConfig::tilt_rate`,
so a slope arrives and a bump does not, and what the seat allows is measured from the seat. Full lock is there at any speed and nothing steps in to save you: the tyres say how hard it really turns,
and past that it slides, ploughs or goes over. On its side or roof its engine stops and the crew climb out, and anyone
on foot can right it (`VehicleConfig::lift`) by holding F with their hands on the hull: it really rolls, against the ground
and whatever is in the way, around its lower edge. A roof-down one comes onto its side and then its wheels. Let go, walk
off, be hit or run out of stamina and it falls back, past its balance it goes on over by itself, and if it stops dead
something is in the way and the prompt says so. A truck takes one and an armoured car two, more go faster, and friend or
enemy will do. Left alone like that for `respawn_after` it goes the way of a wreck. The next round does not put them back.
Q and E change down and up, one gear a press. It is automatic until the first change that goes in, and
then yours until you leave the seat (`Engine::manual_until_stop` makes it until you stop instead): nothing changes up for
you at the redline, and a change down that would take the engine past it does not go in. Reverse and first are still
found by the throttle, stopped. F in a seat goes out of its door, or whichever of its doors you face, if nothing is in the way: there is no
looking for somewhere else to put you, so with that door against a wall you have to move seats first. Nobody gets in
faster than `RideConfig::board_speed`, and getting out faster than `exit_speed` is a jump, and hurts.

A seated soldier is the same soldier in the same world: the seat says where the eye is, and `gun_pose` goes on
from that eye as it does on foot, so a shot leaves the real muzzle. A look is a world look, which the vehicle
carries round with it. A driver has their hands on the wheel and no gun drawn. A passenger's gun bears only
in the seat's `fire` arc and comes down outside it, and the speed and spin of the vehicle unsettle the gun through
the same instability running does. The bullet still leaves along the barrel. In first person the eye's height
is eased a little and only a fifth of the vehicle's roll reaches the view, so nothing shakes the screen
(`ViewSettings::seat_settle_rate`, `seat_roll_share`).

Panels are boxes in the chassis's frame, drawn as they are, with nothing where there is no panel. A truck's
door takes a rifle round and lets most of it through slower, so it is concealment. The sides above it are open, and
whoever sits there can be shot. Tyres and the engine are parts of their own. Stages come in order: a tyre
goes flat and pulls, the engine loses power and smokes and stops, and more hits start a fire, which hurts anyone
who stays and in the end blows up, hurting whoever is near. A new one comes at its spawn point
(`Level::vehicles`, on maps in `MapSpec::vehicles`). The armoured car is the same machinery with plates that stop
everything: those inside see out of a slit and have no way to shoot, and the gunner stands in the roof hatch with
all round fire and the upper body in the open.

An engine is two looped copies of one rendered sound, one bright and one dull, played faster with revs and faded
by load. Through a wall you hear the dull one, quietly (`EngineSound` in `crates/prs_audio/src/hearing.rs`).
Someone stood on a hull is kept as where on it they are (`Soldier::deck`), so a client predicts them on the hull it draws. Bot squads with a long way to go walk to a free vehicle's doors, drive along a way across the map worked out for what fits a truck, get out near the objective and carry on (`ConvoyConfig` in `crates/prs_ai/src/convoy.rs`). A truck that will not go is abandoned after a few tries. All of a vehicle is in `Sim::export_state`, plain data, so it goes through the same restore tests as the rest.

## Helicopters

There is a Huey on each team's pad on the town and one on the range. `prs_helicopter` flies it, with no idea
what a soldier or a renderer is, and every number is in `HeliConfig` (`crates/prs_helicopter/src/config.rs`),
starting from the mass, power and rotor of a loaded UH-1H.

The rotor is blade elements on momentum theory rather than a lift figure, so thrust comes from collective,
rotor speed and how the air is going through the disc. That one set of sums gives translational lift, climb
and descent, droop when you ask for more than the engine has, a rotor that keeps itself turning in an
autorotation, and no lift at all from a slow rotor however far the collective is up. What momentum theory cannot
do goes on top: ground effect, looked for under the disc so a roof counts, and vortex ring state when you sink too
fast with too little speed across the ground. Pulling power in it does nothing and flying out of it does.

Like the real one the rotor teeters and has hardly any hub moment. The machine turns only by leaning its thrust line
away from the centre of mass, so it is sluggish, and sloppier still when the thrust is low. A centred stick holds
whatever attitude it has, it does not level it. The tail rotor takes most of the torque by a linkage from the collective,
and what is left yaws it whenever the power changes. Keys are all or nothing, so a pedal key held gives a kick and walks
the trim, and tapping is how you settle it. The tail thrust pushes the machine sideways as it does in life, so a hover
hangs a few degrees left. The virtual stick lives on the player's side and only its position goes in a command, as
`PlayerCmd::stick`. W and S are the same `wish_move` that walks.

Skids are stiff springs with a look at the ground once a tick, and standing still on them it stops simulating, so it cannot
jitter. Coming down faster than `landing_safe` costs structure and past `landing_fatal` it is written off. The airframe
is lines from its centre, so nothing is missed at any speed. Rays in the plane of the disc turn with the rotor, and they
find walls and, through `Level::canopies`, the crowns of trees. Bullets find the engine (less power, then an
autorotation), the tail rotor (it spins), the blades (only where a blade is right now: less thrust, then none) and the
cockpit (slack cyclic, dead gauges), and they go on through the skin to whoever is sitting behind it. A wind at the edge of
the map turns you back, and air gets thin above `ceiling`.

Two people up front and two in the doors, on the same seats as anything else that carries people. The door seats sit
in the opening and fire out of it with the gun in real space, moved around by the machine and never made to miss in any
other way. Sound is three loops panned to where it was when the sound left it, with doppler: close up the slap of the
blades and the turbine, and from a long way off a dull beat that arrives well before it comes over a ridge.

On a network the pilot predicts the whole of `HeliState` with the same step and is told it to the bit, with
`NetTolerance` saying how far apart two copies may be. Everyone else draws a compact pose about 100 ms in the past, and a
shot at it is judged against that past pose. A correction moves the whole airframe with the eye in it and fades, so the
camera never jumps. `crates/prs_server/tests/heli_net.rs` flies a real sortie through the simulated network. At 100 ms each
way, with or without 15 ms of jitter and 2% loss, hover and 40 m/s circles cost no reconciles at all, and someone watching
draws it within about 1.6 m of where it was on that tick, because it goes in every snapshot at any distance. On TCP with
0.5% loss a stall in the link costs some tens of reconciles a minute and up to 7 m of correction at 50 m/s. That fades no
faster than `NetView::fade_speed` and `fade_turn`, so from the cockpit it is a slide rather than a lurch, and it takes over
half a second to clear. A door gunner's shot starts where they saw their gun, and at that range the miss is the gun moving.
Replaying 20 ticks takes about 0.25 ms in the dev profile.

`crates/prs_helicopter/src/tests.rs` holds a pilot with no eyes (`autopilot.rs`) flying the same controls, with keys that
are either down or up and looked at ten times a second. The whole sortie is a test: start, hover in a box for 30 seconds,
40 m/s over the ridge and round a turn, flare and put it down on the market square. So is a pilot who looks only four times
a second and sees everything a quarter of a second late. To see how it flies:

```sh
cargo test -p prs_helicopter -- --nocapture sortie
PRS_MAP=range PRS_SHOT=out.png PRS_SCENE=heli:cruise cargo run
```

## Maps

A map is data. `MapSpec` in `crates/prs_level/src/spec.rs` describes the land, what stands on it and
where the teams fight, and `MapSpec::build` turns that into the `Level` that collision and rendering
both read, so what you see is what stops you. To add one, fill in a `MapSpec` (`town.rs` is the
example, with kinds of building in `archetypes.rs`) and name it in `Level::named`. `PRS_MAP` chooses
the map, and `DEFAULT_MAP` in `crates/prs_runtime/src/lib.rs` says which one you get without it.

The town is a kilometre square with a ridge across each approach, so neither base sees the other or
the village. Between them are walled compounds, streets of houses and shops, enterable floors and
roofs, ditches and hedged fields to the west, a wash and an orchard to the east, and six capture points.
Stairs, doors, walls, windows and cover heights are held to `MoveConfig` by tests
that walk a soldier through them, in `crates/prs_level/src/town_tests.rs`.

`prs_scene` merges static geometry into 128 m chunks with colour in the vertices, one or two draws per
chunk, and nothing static runs after startup. `prs_collision` buckets colliders on a grid, so queries
do not grow with the map. For the numbers:

```sh
cargo test -p prs_level query_report -- --ignored --nocapture
cargo test -p prs_scene draw_report -- --ignored --nocapture
```

## People in the world

Playing alone there are soldiers walking about, twelve to begin with, half on each team and a quarter of them
around where you start. `PRS_WALKERS=30` or `?walkers=30` asks for another number. They are ordinary soldiers
with an ordinary kit: they go down, bleed, drop their kit and come back at their own spot, and there is nothing
stopping you shooting them. They walk, jog, stop, crouch, lean, look about and go in and out of doors, and they
don't fire yet. `WalkerConfig` in `crates/prs_ai/src/lib.rs` has all of it. In a match on a server the
bots are `Troops`, run by `SimServer` and sent to clients like anyone else (see [Soldiers who fight](#soldiers-who-fight)).

Each one looks where it is going every fifth of a second with two rays, and spends more only when blocked. That
comes to about 2 rays a tick and a few microseconds for twelve. The soldiers themselves are the real cost:

```sh
cargo test -p prs_ai crowd_report -- --ignored --nocapture
```

## Grenades and smoke

G takes out a frag, and again for smoke. Hold left mouse to wind up and let go to throw, with right mouse held for a short underarm
one. There is no trajectory line. Riflemen carry two of each, and other kits fewer (`standard_kits`). `ThrowConfig` in
`crates/prs_throwables/src/lib.rs` has all the numbers.

A frag's fuse runs from the pin coming out, so holding on to it shortens what is left, and past the end it goes off in your hand.
When it goes off every fragment is a real ray, so walls stop them and bones catch them, and the blast needs a clear line. One does not
wipe out a squad that is spread out, and it rattles anyone in sight of it through the same suppression as bullets. Smoke is a growing
cloud that stops anyone seeing along a line through enough of it, bots included, because they all ask `Sim::smoke_blocks`. You see and hear where it comes from: the can pours a stream of puffs that feeds the cloud, leaving a trail behind it as it falls, then lies on the ground until the smoke runs out, fizzing at the start and then hissing, for all of that, with a rough sound made new as it plays and carrying only a short way (`SmokeJet` in `prs_scene` as `view.smoke_jet`, `SmokeSound` in `prs_synth` as `audio.smoke`, in the tuner at `/tune/`).

Over a network it works like bullets: the thrower sees it go at once, everyone else flies their own copy of the arc from where it
started, and only the server says when it goes off and what it did. Fragments do not hit vehicles yet, and anyone in a seat is safe.

## Soldiers who fight

A bot's only output is a `PlayerCmd`, so it shoots the real gun with the same recoil, sway and wander as anyone. It aims by
turning the view, sees its own front sight a moment late and corrects for part of what it sees, and can't do better than that. It
knows about you from a ray from its eye to you, in the direction it is facing and with a chance that falls off with range, or from
hearing your shot, roughly placed. It never reads anyone's aim or anything else it couldn't see. Everything it can do:

- Stops to shoot, in bursts by range, with a wait for the gun to settle and the sights to come back.
- Breaks for cover when rounds come close or land, feeling around with rays for a wall whose near side is out of sight of the
  shooter, and stays down until the fire stops. It reloads there, bandages there, then leans or stands to look out.
- Tells its squad what it saw. Those marked to hold keep firing where the enemy was while the rest go round the side.
- Holds fire with a friend in the line, and medics run to anyone down when it is quiet enough.
- Reads the sim's own `suppression`: above about a half, a soldier gets down, stays down until it clears, and knows who to blame.
- Throws smoke to cover crossing open ground or getting to someone down, and a frag at someone just behind cover, rarely and never perfectly.
- Keeps with its squad on the way somewhere, within limits, with medics closest.

`SkillConfig` holds how good they are (`recruit`, `regular`, `veteran`: reaction, aim speed, tracking noise, patience, nerve), and
`FightConfig` holds the rest. It all runs on whoever is the authority and no client ever runs it: a client just sees soldiers. A
`Holdover` table for elevation and a `FlowMap` for finding gates across the map are each worked out once, from level data, and only
read after that. To see how a match goes and what it costs:

```sh
cargo test -p prs_ai match_report -- --ignored --nocapture
```

## Checking without playing

`PRS_SOAK=<simulated seconds>[:seed]` runs the real game with no window or graphics card and a seeded monkey at the keyboard and mouse, as fast as the machine can go. It is for finding panics: use it with `PRS_MAP`, `PRS_MODE`, `PRS_MOD` and `PRS_SERVER`. `crates/prs_game/tests/keys.rs` drives every binding the same way, with `cargo test -p prs_game --test keys -- --test-threads=1`.

`PRS_SHOT=out.png PRS_SCENE=ads cargo run` plays a short script, saves a screenshot and exits. The
scenes are `hip`, `ads`, `zoom`, `freelook`, `recoil`, `run`, `prone`, `reload`, `range`, `third`, `buddy`, `walkers`,
`climb:<vault|mantle|pullup|stool|mount|roof>:<first|down|third|side|front>`, `rest:<first|third|side|front|outside|nowhere>[:<yaw>[:<pitch>[:hood]]]` and `lean:<left|right>:<hip|ads>:<first|third|front|back>:<open|corner|wall>[:crouch]`.

For bodies there is `PRS_SCENE=body:<stance>:<action>:<camera>`, such as `body:prone:idle:down` or
`body:stand:sprint:side`. `crates/prs_game/src/dev.rs` lists the choices, along with scenes for each gun (`gun:lmg:ads`), changing
guns, bandaging, the carry panel, wounds, and soldiers down or dead. `PRS_SCENE=mode:<what>` looks at what a mode shows (`crates/prs_game/src/dev_mode.rs`). `PRS_END=<seconds>` moves the
moment of the shot, to catch another point of a walk cycle, and `PRS_AT=x,y,z[,yaw]` starts somewhere else.

## Mods

`PRS_MOD=arcade cargo run`, or `?mod=arcade` on the web, lays `mods/arcade.toml` over the game's tuning. A mod says
only what it changes, so `weapons.m4.rounds_per_minute = 900` alone is a whole mod. `"*"` means every entry beside
it, as in `[weapons."*".recoil]`, and lists are replaced whole. A setting that does not exist is refused with its
path and what you probably meant. The sections are `weapons` (`m4`, `m249`, `m14`, `m9`), `movement`, `health`,
`suppression`, `inventory`, `body`, `vehicles` (`truck`, `armoured_car`), `helicopter`, `vehicle_ride`, `world`,
`mode` (`aas`, `tdm`, `practice`), `ai` (`troops`, `walkers`), `atmosphere`, `audio`, `view` (`camera`,
`suppression`) and `team_looks` (by side, `0` and `1`: uniforms, armbands and banners, as in
`team_looks.1.armband = [1.0, 0.5, 0.0]`). The names of the settings are the fields of those types, which are all in the crates' config files.

The server's mod is part of the rules. A client running anything else is refused and told which mod the server
runs. `atmosphere`, `audio`, `view` and `team_looks` are the client's own and do not count. There are three examples in `mods/`:
`arcade` (instant aim and next to no recoil), `hardcore` and `heavy-recoil`. Adding a new gun or vehicle is not
possible yet, as a new entry needs a model and a sound.

## Planned

- Suppression: incoming fire narrows and disturbs your view and unsettles the gun.
- Multiplayer: a server browser and accounts.

## Multiplayer

```sh
cargo run --release -p prs_server --bin prs_server -- --bind 0.0.0.0:7777 --map town --mode aas
PRS_SERVER=ws://host:7777 cargo run --release     # desktop
http://your-page/?server=ws://host:7777           # web
```

With no server named the game is single player, as ever. The client is told the map and mode by the server. A public server on an `https` page needs `wss://`, so put a proxy that terminates TLS in front of it.
There are no accounts yet, so anyone who can reach the port can play.

The server runs the same `prs_sim`, with no Bevy, and is the only copy that counts. Clients send
`PlayerCmd`s and nothing else, so position, ammunition and every hit are decided there.

- **Your own soldier** runs ahead of the server on the same sim step, so moving, your shot, its flash and
  its sound are immediate. Each tick's result is kept, and when the server's word on that tick arrives
  it is compared. If they agree nothing happens. If not, the soldier goes back to that tick, takes the
  server's values and replays the commands since. What that moved goes into an offset that fades, so
  you see no pop.
- **Everyone else** is drawn about 100 ms in the past, between snapshots that have already arrived, and
  what they do reaches you at the moment it comes round on screen. A snapshot carries every input to
  `gun_pose`, so a gun is where it really is.
- **Hits** are decided on the server, against where the target was when the shooter saw them
  (`Sim::set_view_lag`, `FlyingBullet::rewind`), up to `ServerConfig::max_rewind_ticks`. Your client draws
  bullets but they hit nothing there, not even someone standing right in front of it.
- **The clock** needs no synchronising as such. The server reports how early commands are arriving, and
  the client speeds up or slows down slightly to keep them just in time.

| Crate | Job |
| --- | --- |
| `prs_net` | The netcode with no game in it: wire format, clock, prediction, interpolation, server session, command checks, WebSocket and WebTransport clients and host, simulated network. Tested with a toy. |
| `prs_net_sim` | Plugs the real sim into that. The only crate that knows both a soldier and the wire, so when a soldier grows it is the only one that changes. |
| `prs_server` | The server binary, plus `prs_bot` (scripted players over real sockets) and `prs_fingerprint`. |

A client and a server must have the same rules and map, and one that differs is refused (`rules_hash`).
Limits on what a client may ask are in `CmdLimits`, and tuning is in `ServerConfig`, `SimServerConfig`
and `ClientConfig`.

### Voice

Hold Caps Lock, H or Mouse 4 to talk out loud, and Y or Mouse 5 for the squad radio. They are all in one place, `TALK_KEYS` and `TALK_BUTTONS` in `crates/prs_voice/src/lib.rs`. Caps Lock counts for 15 seconds at a time and then has to be pressed again, in case its release never comes, and on Apple systems, where it goes down at one press and up at the next, it does not talk at all. The microphone is off until a player turns it on in the pause menu, and the browser only asks for it on the first press after that. Out loud goes to anyone within 80 metres on either team, from where you are standing, through the same distance, walls and muffling as every other sound. The radio goes to your squad wherever they are, cut down to a radio's band. Who is talking shows on the left for a moment.

It goes over the game's own WebSocket as frames of its own, with no peer connections, so it needs no ports and the server decides everything. A client says only which of the two it is talking on, and nothing it sends can add a listener. The server sends a voice to nobody out of range, and to nobody who has muted that player, from the moment it hears of the mute, including what was already waiting. It keeps nothing and logs nothing, and throws away whatever is too big or over a talker's rate. Each listener has a small queue that goes after the game's frames and is emptied the moment their socket backs up, so that voice is lost rather than anything waiting for it. None of it goes near the sim, prediction or `rules_hash`.

`--no-voice` or `PRS_VOICE=off` turns it off on a server. `server_voice` in a mod changes the limits and the range, and `voice` the bitrate, frame length, fading and radio band.

It needs WebCodecs Opus and, for the microphone, a page on https or localhost. MDN has that in Chrome and Edge 94 (Opus options 110), Firefox 130 on the desktop and Safari 26, but not Firefox on Android, and a browser need not support Opus, so the game asks when it starts. Without it a player hears a message and plays on without voice, and the server sends them none. The desktop build has no voice yet: it would need its own Opus and microphone. Mutes last as long as the connection, as a player number means nothing after that.

### Names

Players name themselves in the box over Play online, which the browser remembers, or with `PRS_NAME` (or `--name`) on the desktop. The name goes in the request to join, and the server has the last word: it keeps letters, digits, single spaces and `-_.'`, cuts it to 16 characters, will not let it start with a word such as Bot, Server, Admin, Player or You, gives anyone left with nothing "Player" and their number, and numbers anyone whose name is taken. Only a to z by default, as other alphabets have letters that pass for Latin ones and the game's font has only Latin. All of that is `NameRules` in `ServerConfig`. Names go to everyone with the rest of the match, so whoever joins late has them too, and they are no part of the rules. They show on the scoreboard, the squad buttons, the list of who is talking and the rows for muting, and over a teammate's head when you look straight at them close by (`name_tags` in a mod, on by default within 25 metres). Never over an enemy's. Everywhere, including the page, a name is only ever text.

### WebTransport

The game prefers WebTransport (QUIC over UDP) to the WebSocket. Over TCP one lost packet holds up everything behind it until it is resent, so the client's clock has to lead by the length of those stalls and other soldiers are drawn on guesswork while one lasts. Over WebTransport, snapshots, commands and voice go as datagrams, which can be lost but never wait for each other. Anything that has to arrive goes on one reliable stream beside them: hello, join, info, welcome, refusal and voice settings (`wire::must_arrive`). A snapshot for a WebTransport client is kept under `ServerConfig::datagram_bytes`, so that it fits in one datagram. One that can't fit goes on the stream. The session acks baselines and sends each command four times (`ClientConfig::cmd_redundancy`), the same as over any link that can lose things.

The server listens on UDP port 4433 as well as its TCP port. It says where at `GET /webtransport` on the TCP port, so the page never hard-codes a port. A client tries WebTransport first. If that hasn't opened within 5 seconds (a blocked UDP port shows as nothing happening), or fails sooner, it uses the WebSocket as before. Refusals are the same on both. `?transport=ws` in the browser or `PRS_TRANSPORT=ws` on the desktop skips WebTransport. The debug overlay (and a `?netbot` title) says which one is in use.

| Flag | Environment | Default |
| --- | --- | --- |
| `--wt-port PORT` | `PRS_WT_PORT` | `4433`. `off` or `--no-webtransport` for none. |
| `--wt-cert PEM --wt-key PEM` | `PRS_WT_CERT`, `PRS_WT_KEY` | none: the server makes its own certificate |

A browser only connects to a certificate it trusts. A public server needs a real one for its name. A web server on the same box already has one, Caddy's for example under `/var/lib/caddy/.local/share/caddy/certificates/acme-v02.api.letsencrypt.org-directory/<name>/`. The server reads it again every 12 hours, so renewals are picked up. With no certificate given, the server makes its own, which lasts 13 days and is renewed while it runs. A browser accepts that certificate only because `/webtransport` gives its hash and the page names it (`serverCertificateHashes`, allowed for certificates that last under two weeks). That works on localhost and a LAN:

```sh
cargo run --release -p prs_server --bin prs_server -- --map town --web bin/web   # ws on 7777, webtransport on udp 4433
http://127.0.0.1:7777/                                                           # Play online
```

Railway has no public UDP, so a server there is reached by WebSocket only. `tests/webtransport.rs` in `prs_net` plays over real WebTransport, and `report_websocket_against_webtransport` in `prs_server/tests/netcode.rs` compares the two on a lossy link.

### Putting it online

One service on Railway serves the page and the game on one port, at `frontcraft.online`, with the DNS staying on
Cloudflare. The page and the server go up in one image, so a deploy can't leave them on different versions. Railway ends
TLS and passes the server plain WebSockets, which it
[leaves open indefinitely](https://docs.railway.com/networking/public-networking/specs-and-limits). Cloudflare's proxy
[carries WebSockets on every plan](https://developers.cloudflare.com/network/websockets/), and
[Railway expects it on](https://docs.railway.com/networking/domains/working-with-domains), with SSL mode Full.

1. `FRONTCRAFT_ONLINE=1 scripts/build_web.sh dist/web`. This takes about a quarter of an hour.
2. `npm i -g @railway/cli`, then `railway login`.
3. In this folder, `railway init` and name the project `frontcraft`.
4. `railway up`. It sends the folder, with no git needed, and builds the `Dockerfile`.
5. In the Railway dashboard, open the service, then Variables, and add: `PRS_MAP=town`, `PRS_MODE=aas`, `PRS_BOTS=16`,
   `PRS_MAX_PLAYERS=32`, `PRS_ALLOWED_ORIGINS=https://frontcraft.online`. `PRS_MOD` picks a mod.
6. In Settings, set Healthcheck Path to `/health`.
7. In Settings, Public Networking, choose + Custom Domain and enter `frontcraft.online`. Note the CNAME and TXT
   records it shows.
8. In the Cloudflare dashboard, open `frontcraft.online`, then DNS. Add that `CNAME` with the name `@`, proxied (orange
   cloud). Cloudflare flattens it, which [Railway supports](https://docs.railway.com/networking/domains/working-with-domains).
9. Add the `TXT` record too. Without it Railway answers 404.
10. In SSL/TLS, set the mode to Full. Railway says Full (Strict) will not work, and that with the proxy off
    you get `ERR_TOO_MANY_REDIRECTS`.
11. Check: `curl https://frontcraft.online/health` says `ok`.
12. Open `https://frontcraft.online` and choose Play online.

To update, do steps 1 and 4 again.

The page offers Play online or Play alone when it was built with `FRONTCRAFT_ONLINE=1`. `?server=wss://...` goes straight to
a server and `?solo=1` straight to playing alone. If a server can't be reached or refuses, the page says why and offers
playing alone. A browser may keep the wasm for good because its address has the build in it, and is told to check
everything else every time.

Each status line from the server has its tick times, and the last line has them for the whole run: a tick has 16.7 ms.

### Replication: the rules for anything that moves

The server is the only authority, for everything, always. Ownership only says who predicts a thing, and it
follows from who is at the controls.

**Bullets.** Nobody replicates a bullet per tick. A shot is one `ShotFired` event, and each side flies its
own copy from that.

- Your own shot is predicted: flash, sound and tracer are immediate.
- Only the server decides what a bullet hits. A client's bullet is a tracer (`Sim::fly_tracers`). It stops
  at walls and hits nothing, not even someone it visibly passes through.
- Another player's shot reaches you when the tick it was fired on comes round on screen, so it starts at
  the muzzle of the soldier you are drawing at that moment, to within a few centimetres. Its path is
  already the true path, so there is nothing to catch up.
- `Impact`, `SoldierHit` and the like come from the server, and you see or hear them when they do. If the
  server disagrees with what you saw, your tracer has gone where it went and nothing else happens. There are
  no hit markers, so there is nothing false to take back. A shot the server refused (you were already dead,
  say) corrects your ammunition through reconciling.
- A bullet flies in the world as its shooter saw it. `FlyingBullet::rewind` is how many ticks back, up to
  `ServerConfig::max_rewind_ticks`, and it holds for the whole flight. It covers everything a bullet can hit:
  soldiers' poses, and vehicle and helicopter transforms, through the same history (`prs_sim/src/past.rs`).

**Vehicles and helicopters.**

- The driver or pilot predicts: `Sim::step_predicted` steps their vehicle too, and they receive its full
  state, to the bit, to reconcile. Everyone else interpolates it about 100 ms in the past, like a soldier.
- Seated soldiers follow their vehicle: its transform plus the seat. Their position is not sent, only the
  seat, so soldier and vehicle cannot jitter apart. Look and gun-hold are still sent, and `gun_pose` works
  from the seat, so a gun is where it really is.
- A passenger is predicted for their look and gun but rides the vehicle as drawn, in the past. So their
  shot is judged wholly in the past too: on the server the shooter's own seat and muzzle are rewound with
  the vehicle by `rewind`, and the bullet starts from there. Otherwise a shot from a truck at 20 m/s with
  200 ms of lag would start 4 m from where the shooter saw it. A driver is in the present, so only their
  targets are rewound. Reconciling a passenger compares look and gun, never world position.
- Getting in and out goes through the sim like any other input, so it is predicted, and the server's seat
  list corrects it if someone else got there first.
- When ownership changes, the snapshot says so. The new driver takes the server's state for that tick and
  replays their own commands since, and the jump from where it was drawn to where it is predicted goes into a
  fading visual offset. A driver changing mid-drive is those steps twice, with the server coasting the
  vehicle on idle input in between. Losing the driver (dead, disconnected) means nobody predicts it.
- Collisions are the server's. In prediction, anything you do not own is a read-only obstacle at its drawn
  pose, and it never gets pushed or hurt. A predicted vehicle hitting an interpolated soldier therefore does
  nothing on the driver's client until the server's word arrives, and then reconciling blends it in. Damage
  always comes from the server.
- Client-owned vehicles, as Arma does it, are out, because of cheating and hit disagreement. The choice is
  per entity type, so it can be revisited if a vehicle feels bad at high ping.

**Interest and budget.** Something that has not changed is not sent at all, so a parked truck costs nothing.
Out to `InterestConfig::full_rate_within` everything goes in every snapshot. Further off it goes less often in
proportion to distance, down to one snapshot in `slowest_every`, and is drawn that much further in the past.
Anything in the cone you are aiming along goes at full rate however far away, so a far target on your sights is
as fresh as a near one. After that each snapshot has a byte budget, spent on whatever has been waiting longest,
except that anything within `always_within` always goes. Beyond its `max_range` a client forgets a thing exists.
Shots and impacts have a reach of their own in `ReachConfig`, far longer for gunfire than for seeing it land.

**Determinism.** Native and wasm floats do not agree, so nothing may rely on them agreeing. A state within
tolerance is left alone, and one outside it is reconciled and blended through the visual offset. The tolerance is
`PredictConfig::epsilon`, 0.001 on every float a soldier has. Something a driver predicts gets the same, as a share
of the size of the number, since engine speed and tyre load run into the thousands. A passenger's gun gets 200
times that, because their ride is a little behind the server's. On one build everything agrees to the bit, including driving 344 m and flying to 150 m with no reconciles at all.
Left to themselves for a minute with nothing correcting them, wasm and native drift apart by 4 mm driving and 2 mm
flying, and 1.2 cm on foot. Since a correction starts each from the same place again, that is well inside what
anyone could see, though a real browser driving against a server has not been measured.

**Adding a networked thing.** It all goes in `prs_net_sim`. Give it a `KIND_` and fields in `rides.rs` for how others
draw it (`capture_*`, a pose with `from_entity`, `lerp` and a `show_*`), list it in `AnyPose`, `SimSchema::groups` and
`SimServer::capture_world`, and if someone predicts it, add it to `Carried` with every field in a visitor. Those
list every field with no `..`, so a new field on anything sent stops the crate compiling until it has been dealt with.

**Testing on a bad network.** Real server sim, real client sims, virtual clock:

```rust
use prs_net_sim::testkit::*;

let mut world = world_on("range");
let net = Net::datagram(50.0, 10.0, 0.02); // ms one way, ms jitter, loss. Or Net::tcp(..).
let me = world.join(client_on("range"), script(|_who, tick| walk_forward(tick)), &net);
world.run(10.0);
assert_eq!(world.players[me].reconciles(), 0);
```

### What it has been measured to do

`cargo test -p prs_server --test netcode` runs a real server and real clients on a simulated network with
latency, jitter and loss. The reports print numbers, and `prs_bot` does the same over real sockets:

```sh
cargo test -p prs_server --test netcode report -- --ignored --nocapture --test-threads=1   # bandwidth, TCP, interest
cargo run -p prs_server --bin prs_bot -- --url ws://127.0.0.1:7777 --clients 32 --script busy
```

At 30 snapshots a second, with everyone in one place, a client costs about 11 KB/s down at 10 players and 22 KB/s
at 32, and 5 KB/s up. Spread over a kilometre square that comes to 10 KB/s for 32 players and 17 KB/s for 64.

WebSocket runs over TCP, where one lost packet holds up everything behind it until it is resent. That is
fine up to about 1% loss. At 5% the command lead grows to hundreds of milliseconds and other soldiers
spend 15% of frames on extrapolation, sometimes by a metre. WebTransport datagrams would fix that by
implementing `prs_net::transport::Transport`, and the session already acks baselines and sends commands
redundantly when told its transport can lose things.

Native and wasm builds do not agree on floating point. `sin` and `cos` differ in the last bits, so terrain
and the simulation drift by about a centimetre a minute left to themselves (`prs_fingerprint`). In a real
browser against a native server that came to one reconcile in 25 seconds and a correction of under a
micrometre, walking and firing, because `PredictConfig::epsilon` lets that much through. It is why the rules hash takes a map
by its shape and not its numbers.

`?netbot=1` on the web or `PRS_NETBOT=1` on a desktop plays by itself, for checking without a mouse.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
