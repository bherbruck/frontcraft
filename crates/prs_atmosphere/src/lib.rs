//! The look of the world: where the sun is, what the sky does, how the air and the camera treat colour.
//!
//! All of it is one [`Atmosphere`] resource read at startup, so a mod that wants another time of day
//! inserts its own before the game starts. Nothing here moves or changes while you play.
//!
//! Light is flat-shading friendly: one warm shadow-casting sun, then a blue sky fill and a warm ground
//! bounce that cast nothing, so every facet of a shadowed wall still reads as its own colour. What
//! those fills cannot know about, such as being indoors, is baked into vertices (see [`Bake`]).

mod sky;

use bevy::camera::Exposure;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
use prs_models::SHADOW_ONLY_LAYER;

pub use sky::{ICOSAHEDRON_CORNERS, ICOSAHEDRON_FACES};

const ONE_DIRECTIONAL_LIGHT: bool = cfg!(target_arch = "wasm32");
/// A fill's illuminance as ambient brightness: 1/pi for the surface's response, and half of that again
/// because surfaces face it only partly on average.
const FILL_AS_AMBIENT: f32 = 0.5 / std::f32::consts::PI;

/// A light with no shadow, standing in for light arriving from a whole part of the sky or the ground.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Fill {
    /// From a surface towards this light. Up the world and away from the sun, for instance.
    pub towards: Vec3,
    #[serde(with = "srgb")]
    pub color: Color,
    pub illuminance: f32,
}

/// What the shadow map covers. WebGL2 draws only the first cascade, so there that one does it all.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Shadows {
    pub cascades: usize,
    pub first_bound: f32,
    pub distance: f32,
    pub map_size: usize,
    pub depth_bias: f32,
    pub normal_bias: f32,
}

impl Default for Shadows {
    fn default() -> Self {
        let web = cfg!(target_arch = "wasm32");
        Self {
            cascades: if web { 1 } else { 4 },
            first_bound: if web { 70.0 } else { 11.0 },
            distance: if web { 70.0 } else { 340.0 },
            map_size: 2048,
            depth_bias: 0.06,
            normal_bias: 2.2,
        }
    }
}

/// The dome over everything. Colours are scene light, the same values lit surfaces reach before the
/// camera's tonemapping, so sky and fog are comparable numbers.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Sky {
    #[serde(with = "srgb")]
    pub zenith: Color,
    /// Colour a good way up, where the blue is richest.
    #[serde(with = "srgb")]
    pub high: Color,
    /// Light added to the air right around the sun, and across the whole of its side of the sky.
    #[serde(with = "srgb")]
    pub halo: Color,
    #[serde(with = "srgb")]
    pub glow: Color,
    #[serde(with = "srgb")]
    pub sun_disc: Color,
    /// Angular radius of the sun, in degrees. Larger than life, as drawn suns are.
    pub sun_radius: f32,
    pub clouds: u32,
    #[serde(with = "srgb")]
    pub cloud_lit: Color,
    #[serde(with = "srgb")]
    pub cloud_shade: Color,
    /// Far ridges behind the map, as (height in degrees, how far into the haze from 0 to 1).
    pub ridges: [(f32, f32); 3],
    #[serde(with = "srgb")]
    pub ridge_tint: Color,
}

/// What gets multiplied into static geometry's vertex colours when the level is built.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Bake {
    /// Where there are no fill lights to tell facets apart in shade, how much of what they would have
    /// done goes into vertex colours by which way a face points, from 0 to 1. It tints sunlit faces too.
    pub face_tint: f32,
    /// How far apart, in linear light, a tile's two diagonals may disagree before it gets a middle
    /// vertex. Two triangles alone draw a saw-tooth of 45 degree edges where the light bends.
    pub tile_twist: f32,
    /// Colour of light that has reached deep into a room, and right by its openings.
    pub room_deep: Vec3,
    pub room_near: Vec3,
    /// How fast a room brightens with how much of an opening a surface can see.
    pub room_gain: f32,
    /// Where two room surfaces meet: how dark, and how far out from the corner that reaches, in metres.
    pub corner_dark: f32,
    pub corner_reach: f32,
    /// Largest tile a room's surface is cut into, in metres.
    pub room_cell: f32,
    /// The foot of an outside wall: how dark, and how far up in metres.
    pub foot_dark: Vec3,
    pub foot_height: f32,
}

impl Default for Bake {
    fn default() -> Self {
        Self {
            face_tint: if ONE_DIRECTIONAL_LIGHT { 0.55 } else { 0.0 },
            tile_twist: 0.01,
            room_deep: Vec3::new(0.33, 0.34, 0.42),
            room_near: Vec3::new(0.86, 0.8, 0.72),
            room_gain: 34.0,
            corner_dark: 0.78,
            corner_reach: 0.8,
            room_cell: 2.6,
            foot_dark: Vec3::new(0.6, 0.58, 0.62),
            foot_height: 1.5,
        }
    }
}

/// Colours in a mod are three numbers, red, green and blue, as you would give them to `Color::srgb`.
mod srgb {
    use bevy::prelude::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(color: &Color, s: S) -> Result<S::Ok, S::Error> {
        let c = color.to_srgba();
        [c.red, c.green, c.blue].serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Color, D::Error> {
        let [r, g, b] = <[f32; 3]>::deserialize(d)?;
        Ok(Color::srgb(r, g, b))
    }
}

fn default_tonemapping() -> Tonemapping {
    Tonemapping::SomewhatBoringDisplayTransform
}

#[derive(Resource, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Atmosphere {
    /// Compass bearing of the sun, in radians clockwise from -Z towards +X.
    pub sun_bearing: f32,
    /// Height of the sun over the horizon, in radians.
    pub sun_elevation: f32,
    #[serde(with = "srgb")]
    pub sun_color: Color,
    pub sun_illuminance: f32,
    #[serde(with = "srgb")]
    pub ambient_color: Color,
    pub ambient_brightness: f32,
    pub fills: Vec<Fill>,
    /// Colour of air seen a long way off, whichever way you look. The sun's light is added on top.
    #[serde(with = "srgb")]
    pub haze: Color,
    /// Metres at which the air hides things. The falloff is squared, so a long shot across a field
    /// stays clear and the far hills still go.
    pub fog_visibility: f32,
    /// How much of the lights' colour the haze picks up looking towards them, and how tightly.
    #[serde(with = "srgb")]
    pub haze_glow: Color,
    pub haze_glow_exponent: f32,
    pub sky: Sky,
    pub shadows: Shadows,
    /// Not a mod setting: the game is built without the lookup tables TonyMcMapface, AgX and BlenderFilmic
    /// need, so this has to be one of the others.
    #[serde(skip, default = "default_tonemapping")]
    pub tonemapping: Tonemapping,
    pub saturation: f32,
    pub contrast: f32,
    pub bake: Bake,
}

impl Default for Atmosphere {
    /// Late afternoon, clear with some cloud: the sun low in the west of the map.
    fn default() -> Self {
        let sun_bearing = 1.95_f32;
        let away = -Vec3::new(sun_bearing.sin(), 0.0, -sun_bearing.cos());
        Self {
            sun_bearing,
            sun_elevation: 21.0_f32.to_radians(),
            sun_color: Color::srgb(1.0, 0.8, 0.58),
            sun_illuminance: 10000.0,
            ambient_color: Color::srgb(0.62, 0.72, 0.95),
            ambient_brightness: 700.0,
            fills: vec![
                // The blue sky overhead and on the side the sun is not.
                Fill { towards: (Vec3::Y + away * 0.55).normalize(), color: Color::srgb(0.56, 0.7, 1.0), illuminance: 2000.0 },
                // Sunlit ground lighting whatever faces it, the only thing reaching under eaves.
                Fill { towards: (Vec3::NEG_Y * 0.8 - away * 0.6).normalize(), color: Color::srgb(1.0, 0.76, 0.5), illuminance: 800.0 },
            ],
            haze: Color::srgb(0.78, 0.76, 0.76),
            fog_visibility: 1500.0,
            haze_glow: Color::srgb(0.2, 0.135, 0.08),
            haze_glow_exponent: 5.0,
            sky: Sky {
                zenith: Color::srgb(0.17, 0.33, 0.62),
                high: Color::srgb(0.42, 0.61, 0.84),
                halo: Color::srgb(1.0, 0.8, 0.55),
                glow: Color::srgb(0.55, 0.36, 0.2),
                sun_disc: Color::srgb(1.0, 0.95, 0.82),
                sun_radius: 1.7,
                clouds: 26,
                cloud_lit: Color::srgb(1.0, 0.9, 0.78),
                cloud_shade: Color::srgb(0.62, 0.64, 0.78),
                ridges: [(3.2, 0.72), (2.2, 0.56), (1.2, 0.4)],
                ridge_tint: Color::srgb(0.42, 0.5, 0.66),
            },
            shadows: Shadows::default(),
            tonemapping: Tonemapping::SomewhatBoringDisplayTransform,
            saturation: 1.05,
            contrast: 1.0,
            bake: Bake::default(),
        }
    }
}

impl Atmosphere {
    /// Unit vector from the ground to the sun.
    pub fn to_sun(&self) -> Vec3 {
        let (up, flat) = self.sun_elevation.sin_cos();
        Vec3::new(self.sun_bearing.sin() * flat, up, -self.sun_bearing.cos() * flat)
    }

    /// The fills that get a light of their own. Bevy draws one directional light on WebGL2, and it keeps
    /// shadowless ones first, so a fill there would put out the sun.
    fn drawn_fills(&self) -> &[Fill] {
        if ONE_DIRECTIONAL_LIGHT { &[] } else { &self.fills }
    }

    /// The ambient light to use, with `fills` folded into it evenly in all directions: what a level
    /// looks like with no fill lights, as near as one number gets.
    fn ambient_with_fills(&self, fills: &[Fill]) -> (Color, f32) {
        let total = fills.iter().fold(linear(self.ambient_color) * self.ambient_brightness, |total, fill| {
            total + linear(fill.color) * fill.illuminance * FILL_AS_AMBIENT
        });
        let brightness = total.max_element().max(f32::EPSILON);
        let color = total / brightness;
        (LinearRgba::rgb(color.x, color.y, color.z).into(), brightness)
    }

    /// What to multiply the colour of a face pointing this way by, on a build with no fill lights: how
    /// much more or less the fills would have given it than the even share it gets through ambient.
    pub fn face_tint(&self, normal: Vec3) -> Vec3 {
        let (color, brightness) = self.ambient_with_fills(&self.fills);
        let even = linear(color) * brightness;
        let aimed = self.fills.iter().fold(linear(self.ambient_color) * self.ambient_brightness, |total, fill| {
            total + linear(fill.color) * fill.illuminance * normal.dot(fill.towards.normalize()).max(0.0) / std::f32::consts::PI
        });
        Vec3::ONE.lerp(aimed / even.max(Vec3::splat(f32::EPSILON)), self.bake.face_tint)
    }

    /// All the directional lights, as (towards it, colour times illuminance), the sun first.
    fn lights(&self) -> Vec<(Vec3, Vec3)> {
        let scaled = |color: Color, lux: f32| Vec3::from_array(color.to_linear().to_f32_array_no_alpha()) * lux;
        let mut lights = vec![(self.to_sun(), scaled(self.sun_color, self.sun_illuminance))];
        lights.extend(self.drawn_fills().iter().map(|f| (f.towards.normalize(), scaled(f.color, f.illuminance))));
        lights
    }

    /// What fully fogged things look like when seen in this direction: the shader's sum, here on the
    /// CPU, so the sky can meet the fog exactly at every bearing.
    pub fn haze_towards(&self, dir: Vec3) -> Vec3 {
        let exposure = Exposure::default().exposure();
        let tint = linear(self.haze_glow);
        self.lights().iter().fold(linear(self.haze), |haze, (towards, light)| {
            let facing = dir.dot(*towards).max(0.0).powf(self.haze_glow_exponent);
            haze + facing * *light * exposure * tint
        })
    }

    /// What a camera in this world wears.
    pub fn camera(&self) -> impl Bundle {
        let section = ColorGradingSection { contrast: self.contrast, ..default() };
        (
            DistanceFog {
                color: self.haze,
                directional_light_color: self.haze_glow,
                directional_light_exponent: self.haze_glow_exponent,
                falloff: FogFalloff::from_visibility_squared(self.fog_visibility),
            },
            self.tonemapping,
            ColorGrading {
                global: ColorGradingGlobal { post_saturation: self.saturation, ..default() },
                shadows: section,
                midtones: section,
                highlights: section,
            },
            Exposure::default(),
        )
    }
}

pub(crate) fn linear(color: Color) -> Vec3 {
    Vec3::from_array(color.to_linear().to_f32_array_no_alpha())
}

pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(prs_mods::tuned("atmosphere", Atmosphere::default()))
            .add_systems(Startup, (spawn_lights, sky::spawn_sky))
            .add_systems(PostUpdate, sky::follow_camera.before(TransformSystems::Propagate));
    }
}

fn spawn_lights(mut commands: Commands, atmosphere: Res<Atmosphere>) {
    let shadows = atmosphere.shadows;
    // Whatever is left uncovered, for a frame or by a gap at the far plane, should look like the air.
    let clear = atmosphere.haze_towards(Vec3::X);
    commands.insert_resource(ClearColor(LinearRgba::rgb(clear.x, clear.y, clear.z).into()));
    commands.insert_resource(DirectionalLightShadowMap { size: shadows.map_size });
    let lost = if ONE_DIRECTIONAL_LIGHT { atmosphere.fills.as_slice() } else { &[] };
    let (color, brightness) = atmosphere.ambient_with_fills(lost);
    commands.insert_resource(GlobalAmbientLight { color, brightness, ..default() });
    commands.spawn((
        DirectionalLight {
            color: atmosphere.sun_color,
            illuminance: atmosphere.sun_illuminance,
            shadow_maps_enabled: true,
            shadow_depth_bias: shadows.depth_bias,
            shadow_normal_bias: shadows.normal_bias,
            ..default()
        },
        Transform::default().looking_to(-atmosphere.to_sun(), Vec3::Y),
        RenderLayers::from_layers(&[0, SHADOW_ONLY_LAYER]),
        CascadeShadowConfigBuilder {
            num_cascades: shadows.cascades,
            first_cascade_far_bound: shadows.first_bound,
            maximum_distance: shadows.distance,
            ..default()
        }
        .build(),
    ));
    for fill in atmosphere.drawn_fills() {
        commands.spawn((
            DirectionalLight { color: fill.color, illuminance: fill.illuminance, shadow_maps_enabled: false, ..default() },
            Transform::default().looking_to(-fill.towards, Vec3::Y),
        ));
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_mod_can_change_the_light_and_leaves_everything_else_as_it_was() {
        let active = prs_mods::Mod::parse("test", "atmosphere.haze = [0.2, 0.3, 0.4]\natmosphere.sun_illuminance = 500").unwrap();
        let (plain, modded) = (Atmosphere::default(), active.over("atmosphere", Atmosphere::default()).unwrap());
        assert_eq!(modded.sun_illuminance, 500.0);
        assert_eq!(modded.haze, Color::srgb(0.2, 0.3, 0.4));
        assert_eq!(modded.sun_elevation, plain.sun_elevation);
        assert_eq!(modded.tonemapping, plain.tonemapping);
    }

    use super::*;

    #[test]
    fn the_sun_is_where_its_bearing_says() {
        let mut a = Atmosphere { sun_bearing: 0.0, sun_elevation: 0.0, ..default() };
        assert!(a.to_sun().distance(Vec3::NEG_Z) < 1e-5);
        a.sun_bearing = std::f32::consts::FRAC_PI_2;
        assert!(a.to_sun().distance(Vec3::X) < 1e-5);
        a.sun_elevation = std::f32::consts::FRAC_PI_2;
        assert!(a.to_sun().distance(Vec3::Y) < 1e-5);
    }

    /// The horizon has to come out the colour of fully fogged land in every direction, or the land
    /// shows as a cut-out against the sky.
    #[test]
    fn the_sky_meets_the_fog_all_the_way_round() {
        let a = Atmosphere::default();
        for i in 0..64 {
            let bearing = i as f32 / 64.0 * std::f32::consts::TAU;
            let dir = Vec3::new(bearing.sin(), 0.0, bearing.cos());
            let sky = sky::sky_color(&a, dir);
            assert!(sky.distance(a.haze_towards(dir)) < 1e-3, "{sky} at bearing {bearing}");
        }
    }

    #[test]
    fn fills_folded_into_ambient_add_their_light_and_lose_none() {
        let a = Atmosphere::default();
        let (same, brightness) = a.ambient_with_fills(&[]);
        assert!((linear(same) * brightness).distance(linear(a.ambient_color) * a.ambient_brightness) < 0.01);
        let (color, brightness) = a.ambient_with_fills(&a.fills);
        let folded = linear(color) * brightness;
        let by_hand = a.fills.iter().fold(linear(a.ambient_color) * a.ambient_brightness, |t, f| t + linear(f.color) * f.illuminance * FILL_AS_AMBIENT);
        assert!(folded.distance(by_hand) < 0.1, "{folded} {by_hand}");
        assert!(folded.min_element() > a.ambient_brightness * linear(a.ambient_color).min_element());
    }

    #[test]
    fn a_face_tint_only_shows_when_asked_for_and_tells_faces_apart() {
        let mut a = Atmosphere::default();
        a.bake.face_tint = 0.0;
        assert_eq!(a.face_tint(Vec3::Y), Vec3::ONE);
        a.bake.face_tint = 1.0;
        let (up, down) = (a.face_tint(Vec3::Y), a.face_tint(Vec3::NEG_Y));
        assert!(up.z > down.z, "the sky is blue: {up} {down}");
        assert!(down.x / down.z > up.x / up.z, "the ground is warm: {up} {down}");
    }

    #[test]
    fn looking_at_the_sun_makes_the_haze_warmer_and_brighter() {
        let a = Atmosphere::default();
        let (towards, away) = (a.haze_towards(a.to_sun()), a.haze_towards(-a.to_sun()));
        assert!(towards.x > away.x);
        assert!(towards.x / towards.z > away.x / away.z);
    }
}
