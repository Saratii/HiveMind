/*
prologue
Name of program: buildings.rs
Description: Procedural seeded building generation placed along road edges without intersecting road corridors.
Author: Maren Proplesch
Date Created: 3/31/2026
Date Revised: 4/26/2026
Revision History: Fixed placement logic. Promoted all magic numbers to named constants. Added emissive window strips to all building archetypes.
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
Citation: Used AI copilot for limited code generation - claude.ai
*/

use crate::ROAD_WIDTH;
use crate::map_parser::CityData;
use bevy::prelude::*;

// placement geometry
const BUILDING_SEED: u64 = 0xdeadbeef_c0ffee42;
// minimum perpendicular distance from road centerline to building footprint edge
const ROAD_CLEAR_HALF: f32 = ROAD_WIDTH * 0.5 + 4.0;
// how far the building center sits from the road centerline; must exceed ROAD_CLEAR_HALF + FOOTPRINT_R
const SIDE_OFFSET: f32 = ROAD_CLEAR_HALF + 40.0;
// spacing along the road between building slots
const SIDE_STEP_SPACING: f32 = 120.0;
// maximum building slots per road edge
const SIDE_STEP_COUNT: usize = 8;
// placement retries per slot before giving up
const BUILDING_ATTEMPTS: usize = 6;
// footprint radius for road-clearance testing; must be < SIDE_OFFSET - ROAD_CLEAR_HALF
const FOOTPRINT_R: f32 = 38.0;
// maximum random extra lateral offset added on top of SIDE_OFFSET
const EXTRA_OFFSET_MAX: f32 = 20.0;
// maximum random jitter along the road axis as a fraction of the step length
const JITTER_FRACTION: f32 = 0.3;

// shared material properties
const ROUGHNESS_MIN: f32 = 0.55;
const ROUGHNESS_MAX: f32 = 0.85;
const ACCENT_ROUGHNESS: f32 = 0.30;
const ACCENT_METALLIC: f32 = 0.40;
const GLASS_R: f32 = 0.55;
const GLASS_G: f32 = 0.70;
const GLASS_B: f32 = 0.80;
const GLASS_A: f32 = 0.55;
const GLASS_EMISSIVE_R: f32 = 0.02;
const GLASS_EMISSIVE_G: f32 = 0.06;
const GLASS_EMISSIVE_B: f32 = 0.10;
const GLASS_ROUGHNESS: f32 = 0.05;
const GLASS_METALLIC: f32 = 0.80;
const EMISSIVE_SCALE: f32 = 0.05;
const ACCENT_COLOR_MIN: f32 = 0.55;
const ACCENT_COLOR_MAX: f32 = 0.80;
const ACCENT_COLOR_B_MIN: f32 = 0.60;
const ACCENT_COLOR_B_MAX: f32 = 0.85;

// archetype 0 - glass curtain-wall tower
const TOWER_W_MIN: f32 = 45.0;
const TOWER_W_MAX: f32 = 80.0;
const TOWER_D_MIN: f32 = 45.0;
const TOWER_D_MAX: f32 = 80.0;
const TOWER_H_MIN: f32 = 180.0;
const TOWER_H_MAX: f32 = 480.0;
const TOWER_PLINTH_H: f32 = 14.0;
const TOWER_PLINTH_EXTRA: f32 = 10.0;
const TOWER_GLASS_FRACTION: f32 = 0.82;
const TOWER_GLASS_THICKNESS: f32 = 0.6;
const TOWER_GLASS_OFFSET: f32 = 0.4;
const TOWER_WIN_H_FRACTION: f32 = 0.82;
const TOWER_WIN_BOTTOM_FRACTION: f32 = 0.09;
const TOWER_CROWN_H_MIN: f32 = 5.0;
const TOWER_CROWN_H_MAX: f32 = 12.0;
const TOWER_CROWN_FRACTION: f32 = 0.35;

// archetype 1 - wide commercial block
const BLOCK_W_MIN: f32 = 70.0;
const BLOCK_W_MAX: f32 = 130.0;
const BLOCK_D_MIN: f32 = 70.0;
const BLOCK_D_MAX: f32 = 130.0;
const BLOCK_H_MIN: f32 = 40.0;
const BLOCK_H_MAX: f32 = 100.0;
const BLOCK_PLINTH_H: f32 = 10.0;
const BLOCK_PLINTH_EXTRA: f32 = 8.0;
const BLOCK_RIDGE_H: f32 = 5.0;
const BLOCK_RIDGE_W: f32 = 3.0;
const BLOCK_ROOFTOP_H_MIN: f32 = 4.0;
const BLOCK_ROOFTOP_H_MAX: f32 = 10.0;
const BLOCK_ROOFTOP_FRACTION: f32 = 0.4;

// archetype 2 - setback stepped tower
const STEPPED_W_MIN: f32 = 55.0;
const STEPPED_W_MAX: f32 = 110.0;
const STEPPED_D_MIN: f32 = 55.0;
const STEPPED_D_MAX: f32 = 110.0;
const STEPPED_TIER_H_MIN: f32 = 40.0;
const STEPPED_TIER_H_MAX: f32 = 90.0;
const STEPPED_PLINTH_H: f32 = 12.0;
const STEPPED_PLINTH_EXTRA: f32 = 10.0;
const STEPPED_LEDGE_H: f32 = 3.0;
const STEPPED_LEDGE_EXTRA: f32 = 5.0;
const STEPPED_TIER_SHRINK: f32 = 0.62;
const STEPPED_SPIRE_H_MIN: f32 = 10.0;
const STEPPED_SPIRE_H_MAX: f32 = 24.0;
const STEPPED_SPIRE_R_FRACTION: f32 = 0.22;
const STEPPED_SPIRE_R_MIN: f32 = 1.5;

// window lighting
const WIN_THICKNESS: f32 = 0.5;
const WIN_H_FRACTION: f32 = 0.72; // window strip height relative to floor height
const WIN_W_FRACTION: f32 = 0.78; // window strip width relative to face width
const WIN_FLOOR_H: f32 = 22.0; // approximate floor-to-floor height for slot count

// Marker component attached to every building mesh entity so the toggle system can query them
#[derive(Component)]
pub struct BuildingMarker;

// Advances an LCG state and returns the next pseudorandom value
// Input: state: &mut u64 the current LCG state, mutated in place
// Returns: u64 the next value after the shift extraction
fn lcg(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

// Maps a raw LCG output to an f32 in [0, 1)
// Input: v: u64 raw LCG value
// Returns: f32 in [0, 1)
fn lcg_f32(v: u64) -> f32 {
    (v as f32) / (u32::MAX as f32)
}

// Maps a raw LCG output to an f32 in [lo, hi)
// Input: v: u64 raw LCG value; lo: f32 lower bound; hi: f32 upper bound
// Returns: f32 in [lo, hi)
fn lcg_range(v: u64, lo: f32, hi: f32) -> f32 {
    lo + lcg_f32(v) * (hi - lo)
}

// Returns the closest distance from point (px, pz) to the finite segment (ax, az)-(bx, bz)
// Input: px, pz: f32 query point; ax, az, bx, bz: f32 segment endpoints
// Returns: f32 minimum distance from the point to the segment
fn point_segment_dist(px: f32, pz: f32, ax: f32, az: f32, bx: f32, bz: f32) -> f32 {
    let dx = bx - ax;
    let dz = bz - az;
    let len_sq = dx * dx + dz * dz;
    if len_sq < 1e-6 {
        let ex = px - ax;
        let ez = pz - az;
        return (ex * ex + ez * ez).sqrt();
    }
    let t = ((px - ax) * dx + (pz - az) * dz) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let nx = ax + t * dx;
    let nz = az + t * dz;
    let ex = px - nx;
    let ez = pz - nz;
    (ex * ex + ez * ez).sqrt()
}

// Returns true when a circle of radius r centered at (cx, cz) does not intersect any road
// corridor in the city graph; each corridor is a capsule of half-width ROAD_CLEAR_HALF centered
// on the segment between two connected nodes; only canonical edges (id_a < id_b) are tested to
// avoid double-checking and node-to-self degenerate segments are never produced
// Input: cx, cz: f32 footprint center; r: f32 footprint radius; city: &CityData road graph
// Returns: bool true when clear of all road corridors
fn footprint_clear(cx: f32, cz: f32, r: f32, city: &CityData) -> bool {
    let needed = ROAD_CLEAR_HALF + r;
    for (id, node) in &city.nodes {
        let (ax, az) = (node.x, node.y);
        for nb_id in &node.connects {
            if nb_id.as_str() <= id.as_str() {
                continue;
            }
            let nb = match city.nodes.get(nb_id) {
                Some(n) => n,
                None => continue,
            };
            let dist = point_segment_dist(cx, cz, ax, az, nb.x, nb.y);
            if dist < needed {
                return false;
            }
        }
    }
    true
}

// Selects a facade color from a small urban palette seeded from the LCG
// Input: rng: &mut u64 LCG state
// Returns: (Color, LinearRgba) base_color and a faint matching emissive
fn building_color(rng: &mut u64) -> (Color, LinearRgba) {
    let palette: &[(f32, f32, f32)] = &[
        (0.55, 0.52, 0.50),
        (0.62, 0.58, 0.52),
        (0.40, 0.43, 0.48),
        (0.70, 0.64, 0.57),
        (0.35, 0.38, 0.35),
        (0.60, 0.55, 0.55),
        (0.48, 0.50, 0.54),
        (0.72, 0.68, 0.60),
    ];
    let idx = (lcg(rng) as usize) % palette.len();
    let (r, g, b) = palette[idx];
    let emissive = LinearRgba::new(
        r * EMISSIVE_SCALE,
        g * EMISSIVE_SCALE,
        b * EMISSIVE_SCALE,
        1.0,
    );
    (Color::srgb(r, g, b), emissive)
}

// Spawns a column of emissive window quads on one face of a building; each floor slot gets its
// own thin unlit cuboid placed just proud of the face surface, with ~25% of floors randomly dark
// to simulate unoccupied rooms; unlit: true means these glow at full emissive strength regardless
// of the day-night ambient level, making them pop at night without any point lights or shadows
// Input: commands, meshes, materials: Bevy asset params; cx, cy_base, cz: world XZ center and
//   Y bottom of the building body; win_w, win_d: f32 quad dimensions along and across the face;
//   body_h: f32 total body height used to compute floor count; ox, oz: f32 offset from building
//   center to place the quad on the face surface; is_cool: bool warm-yellow vs cool-blue tint;
//   rng: &mut u64 LCG state
// Returns: none
fn spawn_window_strips(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    cx: f32,
    cy_base: f32,
    cz: f32,
    win_w: f32,
    win_d: f32,
    body_h: f32,
    ox: f32,
    oz: f32,
    is_cool: bool,
    rng: &mut u64,
) {
    let floors = ((body_h / WIN_FLOOR_H) as usize).max(1).min(24);
    let floor_h = body_h / floors as f32;
    let win_h = floor_h * WIN_H_FRACTION;
    let (base, er, eg, eb) = if is_cool {
        (Color::srgb(0.75, 0.90, 1.00), 15.0_f32, 22.0_f32, 30.0_f32)
    } else {
        (Color::srgb(1.00, 0.95, 0.75), 30.0_f32, 22.0_f32, 8.0_f32)
    };
    for floor in 0..floors {
        if lcg(rng) % 4 == 0 {
            continue;
        }
        let y = cy_base + floor_h * (floor as f32 + 0.5);
        let win_mat = materials.add(StandardMaterial {
            base_color: base,
            emissive: LinearRgba::new(er, eg, eb, 1.0),
            unlit: true,
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(win_w, win_h, win_d))),
            MeshMaterial3d(win_mat),
            Transform::from_xyz(cx + ox, y, cz + oz),
            Pickable::IGNORE,
            BuildingMarker,
        ));
    }
}

// Spawns one procedural building made of Cuboid and Cylinder primitives at world position (cx, cz);
// archetype 0 = glass curtain-wall tower, 1 = wide commercial block, 2 = setback stepped tower;
// all dimensions are derived from the LCG so the same seed always produces identical geometry;
// every variant includes an oversized ground-floor plinth whose base sits at y=0 so it reads as
// sitting flush on the ground and visually attached to the adjacent road edge
// Input: commands, meshes, materials: standard Bevy asset parameters; cx, cz: f32 footprint center in world space; rng: &mut u64 LCG state for this building; archetype: u64 selects variant modulo 3
// Returns: none
fn spawn_building(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    cx: f32,
    cz: f32,
    rng: &mut u64,
    archetype: u64,
) {
    let (base_color, emissive) = building_color(rng);
    let mat = materials.add(StandardMaterial {
        base_color,
        emissive,
        perceptual_roughness: lcg_range(lcg(rng), ROUGHNESS_MIN, ROUGHNESS_MAX),
        ..default()
    });
    let accent_color = Color::srgb(
        lcg_range(lcg(rng), ACCENT_COLOR_MIN, ACCENT_COLOR_MAX),
        lcg_range(lcg(rng), ACCENT_COLOR_MIN, ACCENT_COLOR_MAX),
        lcg_range(lcg(rng), ACCENT_COLOR_B_MIN, ACCENT_COLOR_B_MAX),
    );
    let accent_mat = materials.add(StandardMaterial {
        base_color: accent_color,
        perceptual_roughness: ACCENT_ROUGHNESS,
        metallic: ACCENT_METALLIC,
        ..default()
    });
    let glass_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(GLASS_R, GLASS_G, GLASS_B, GLASS_A),
        emissive: LinearRgba::new(GLASS_EMISSIVE_R, GLASS_EMISSIVE_G, GLASS_EMISSIVE_B, 1.0),
        perceptual_roughness: GLASS_ROUGHNESS,
        metallic: GLASS_METALLIC,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    match archetype % 3 {
        // glass curtain-wall tower: tall thin body with transparent panels on three faces and a small crown block
        0 => {
            let w = lcg_range(lcg(rng), TOWER_W_MIN, TOWER_W_MAX);
            let d = lcg_range(lcg(rng), TOWER_D_MIN, TOWER_D_MAX);
            let h = lcg_range(lcg(rng), TOWER_H_MIN, TOWER_H_MAX);
            let plinth_h = TOWER_PLINTH_H;
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(
                    w + TOWER_PLINTH_EXTRA,
                    plinth_h,
                    d + TOWER_PLINTH_EXTRA,
                ))),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(cx, plinth_h * 0.5, cz),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(w, h, d))),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(cx, plinth_h + h * 0.5, cz),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            let win_h = h * TOWER_WIN_H_FRACTION;
            let win_y = plinth_h + h * TOWER_WIN_BOTTOM_FRACTION + win_h * 0.5;
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(
                    w * TOWER_GLASS_FRACTION,
                    win_h,
                    TOWER_GLASS_THICKNESS,
                ))),
                MeshMaterial3d(glass_mat.clone()),
                Transform::from_xyz(cx, win_y, cz + d * 0.5 + TOWER_GLASS_OFFSET),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(
                    w * TOWER_GLASS_FRACTION,
                    win_h,
                    TOWER_GLASS_THICKNESS,
                ))),
                MeshMaterial3d(glass_mat.clone()),
                Transform::from_xyz(cx, win_y, cz - d * 0.5 - TOWER_GLASS_OFFSET),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(
                    TOWER_GLASS_THICKNESS,
                    win_h,
                    d * TOWER_GLASS_FRACTION,
                ))),
                MeshMaterial3d(glass_mat.clone()),
                Transform::from_xyz(cx + w * 0.5 + TOWER_GLASS_OFFSET, win_y, cz),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            let crown_h = lcg_range(lcg(rng), TOWER_CROWN_H_MIN, TOWER_CROWN_H_MAX);
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(
                    w * TOWER_CROWN_FRACTION,
                    crown_h,
                    d * TOWER_CROWN_FRACTION,
                ))),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(cx, plinth_h + h + crown_h * 0.5, cz),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            // window lighting: three faces, towers lean cool (offices/glass)
            let is_cool = lcg(rng) % 3 == 0;
            let body_base_y = plinth_h;
            spawn_window_strips(
                commands,
                meshes,
                materials,
                cx,
                body_base_y,
                cz,
                w * WIN_W_FRACTION,
                WIN_THICKNESS,
                h,
                0.0,
                d * 0.5 + 0.3,
                is_cool,
                rng,
            );
            spawn_window_strips(
                commands,
                meshes,
                materials,
                cx,
                body_base_y,
                cz,
                w * WIN_W_FRACTION,
                WIN_THICKNESS,
                h,
                0.0,
                -(d * 0.5 + 0.3),
                is_cool,
                rng,
            );
            spawn_window_strips(
                commands,
                meshes,
                materials,
                cx,
                body_base_y,
                cz,
                WIN_THICKNESS,
                d * WIN_W_FRACTION,
                h,
                w * 0.5 + 0.3,
                0.0,
                is_cool,
                rng,
            );
        }
        // wide commercial block: squat body with decorative parapet ridges along all four edges
        1 => {
            let w = lcg_range(lcg(rng), BLOCK_W_MIN, BLOCK_W_MAX);
            let d = lcg_range(lcg(rng), BLOCK_D_MIN, BLOCK_D_MAX);
            let h = lcg_range(lcg(rng), BLOCK_H_MIN, BLOCK_H_MAX);
            let plinth_h = BLOCK_PLINTH_H;
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(
                    w + BLOCK_PLINTH_EXTRA,
                    plinth_h,
                    d + BLOCK_PLINTH_EXTRA,
                ))),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(cx, plinth_h * 0.5, cz),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(w, h, d))),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(cx, plinth_h + h * 0.5, cz),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            let ridge_h = BLOCK_RIDGE_H;
            let ridge_w = BLOCK_RIDGE_W;
            let top_y = plinth_h + h + ridge_h * 0.5;
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(ridge_w, ridge_h, d + ridge_w * 2.0))),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(cx + w * 0.5 - ridge_w * 0.5, top_y, cz),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(ridge_w, ridge_h, d + ridge_w * 2.0))),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(cx - w * 0.5 + ridge_w * 0.5, top_y, cz),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(w, ridge_h, ridge_w))),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(cx, top_y, cz + d * 0.5 - ridge_w * 0.5),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(w, ridge_h, ridge_w))),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(cx, top_y, cz - d * 0.5 + ridge_w * 0.5),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            let rooftop_h = lcg_range(lcg(rng), BLOCK_ROOFTOP_H_MIN, BLOCK_ROOFTOP_H_MAX);
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(
                    w * BLOCK_ROOFTOP_FRACTION,
                    rooftop_h,
                    d * BLOCK_ROOFTOP_FRACTION,
                ))),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(cx, plinth_h + h + rooftop_h * 0.5, cz),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            // window lighting: all four faces, commercial blocks lean warm (shops/offices)
            let is_cool = lcg(rng) % 4 == 0;
            let body_base_y = plinth_h;
            spawn_window_strips(
                commands,
                meshes,
                materials,
                cx,
                body_base_y,
                cz,
                w * WIN_W_FRACTION,
                WIN_THICKNESS,
                h,
                0.0,
                d * 0.5 + 0.3,
                is_cool,
                rng,
            );
            spawn_window_strips(
                commands,
                meshes,
                materials,
                cx,
                body_base_y,
                cz,
                w * WIN_W_FRACTION,
                WIN_THICKNESS,
                h,
                0.0,
                -(d * 0.5 + 0.3),
                is_cool,
                rng,
            );
            spawn_window_strips(
                commands,
                meshes,
                materials,
                cx,
                body_base_y,
                cz,
                WIN_THICKNESS,
                d * WIN_W_FRACTION,
                h,
                w * 0.5 + 0.3,
                0.0,
                is_cool,
                rng,
            );
            spawn_window_strips(
                commands,
                meshes,
                materials,
                cx,
                body_base_y,
                cz,
                WIN_THICKNESS,
                d * WIN_W_FRACTION,
                h,
                -(w * 0.5 + 0.3),
                0.0,
                is_cool,
                rng,
            );
        }
        // setback stepped tower: 2-4 shrinking tiers with accent ledges and a cylinder spire on top
        _ => {
            let base_w = lcg_range(lcg(rng), STEPPED_W_MIN, STEPPED_W_MAX);
            let base_d = lcg_range(lcg(rng), STEPPED_D_MIN, STEPPED_D_MAX);
            let tiers = 2 + (lcg(rng) % 3) as usize;
            let tier_h = lcg_range(lcg(rng), STEPPED_TIER_H_MIN, STEPPED_TIER_H_MAX);
            let plinth_h = STEPPED_PLINTH_H;
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(
                    base_w + STEPPED_PLINTH_EXTRA,
                    plinth_h,
                    base_d + STEPPED_PLINTH_EXTRA,
                ))),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(cx, plinth_h * 0.5, cz),
                Pickable::IGNORE,
                BuildingMarker,
            ));
            let mut cur_y = plinth_h;
            let mut cur_w = base_w;
            let mut cur_d = base_d;
            for tier in 0..tiers {
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(cur_w, tier_h, cur_d))),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_xyz(cx, cur_y + tier_h * 0.5, cz),
                    Pickable::IGNORE,
                    BuildingMarker,
                ));
                // window lighting: two faces per tier, mix of warm and cool per tier
                let is_cool = lcg(rng) % 3 == 0;
                spawn_window_strips(
                    commands,
                    meshes,
                    materials,
                    cx,
                    cur_y,
                    cz,
                    cur_w * WIN_W_FRACTION,
                    WIN_THICKNESS,
                    tier_h,
                    0.0,
                    cur_d * 0.5 + 0.3,
                    is_cool,
                    rng,
                );
                spawn_window_strips(
                    commands,
                    meshes,
                    materials,
                    cx,
                    cur_y,
                    cz,
                    cur_w * WIN_W_FRACTION,
                    WIN_THICKNESS,
                    tier_h,
                    0.0,
                    -(cur_d * 0.5 + 0.3),
                    is_cool,
                    rng,
                );
                spawn_window_strips(
                    commands,
                    meshes,
                    materials,
                    cx,
                    cur_y,
                    cz,
                    WIN_THICKNESS,
                    cur_d * WIN_W_FRACTION,
                    tier_h,
                    cur_w * 0.5 + 0.3,
                    0.0,
                    is_cool,
                    rng,
                );
                if tier < tiers - 1 {
                    commands.spawn((
                        Mesh3d(meshes.add(Cuboid::new(
                            cur_w + STEPPED_LEDGE_EXTRA,
                            STEPPED_LEDGE_H,
                            cur_d + STEPPED_LEDGE_EXTRA,
                        ))),
                        MeshMaterial3d(accent_mat.clone()),
                        Transform::from_xyz(cx, cur_y + tier_h + STEPPED_LEDGE_H * 0.5, cz),
                        Pickable::IGNORE,
                        BuildingMarker,
                    ));
                    cur_y += tier_h + STEPPED_LEDGE_H;
                } else {
                    cur_y += tier_h;
                }
                cur_w *= STEPPED_TIER_SHRINK;
                cur_d *= STEPPED_TIER_SHRINK;
            }
            let spire_h = lcg_range(lcg(rng), STEPPED_SPIRE_H_MIN, STEPPED_SPIRE_H_MAX);
            let spire_r = cur_w.min(cur_d).max(STEPPED_SPIRE_R_MIN) * STEPPED_SPIRE_R_FRACTION;
            commands.spawn((
                Mesh3d(meshes.add(Cylinder::new(spire_r, spire_h))),
                MeshMaterial3d(accent_mat.clone()),
                Transform::from_xyz(cx, cur_y + spire_h * 0.5, cz),
                Pickable::IGNORE,
                BuildingMarker,
            ));
        }
    }
}

// Places procedural buildings along both sides of every road segment by sampling candidate
// positions at regular intervals perpendicular to each edge, checking each candidate against all
// road corridors using a capsule test, and spawning when clear; edges are sorted before iteration
// so the same BUILDING_SEED always produces an identical city regardless of HashMap order;
// each candidate's exact position includes a small LCG jitter along the road axis so the
// buildings read as organically placed rather than uniformly gridded
// Input: commands: Commands; meshes: ResMut<Assets<Mesh>>; materials: ResMut<Assets<StandardMaterial>>; city: NonSend<CityData>
// Returns: none
pub fn spawn_buildings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    city: NonSend<CityData>,
) {
    let mut global_rng = BUILDING_SEED;
    let mut edges: Vec<(String, String)> = Vec::new();
    for (id, node) in &city.nodes {
        for nb_id in &node.connects {
            if id.as_str() < nb_id.as_str() {
                edges.push((id.clone(), nb_id.clone()));
            }
        }
    }
    edges.sort();
    for (id_a, id_b) in &edges {
        let node_a = match city.nodes.get(id_a) {
            Some(n) => n,
            None => continue,
        };
        let node_b = match city.nodes.get(id_b) {
            Some(n) => n,
            None => continue,
        };
        let ax = node_a.x;
        let az = node_a.y;
        let bx = node_b.x;
        let bz = node_b.y;
        let dx = bx - ax;
        let dz = bz - az;
        let len = dx.hypot(dz);
        if len < SIDE_STEP_SPACING {
            continue;
        }
        let ux = dx / len;
        let uz = dz / len;
        // right-perpendicular in Bevy XZ plane: rotate (ux, uz) by -90 deg
        let rx = uz;
        let rz = -ux;
        let id_hash: u64 = id_a
            .bytes()
            .chain(id_b.bytes())
            .enumerate()
            .fold(0u64, |acc, (i, b)| {
                acc ^ (b as u64).wrapping_mul(2654435761u64.wrapping_add(i as u64))
            });
        let mut edge_rng = BUILDING_SEED ^ id_hash;
        let steps = ((len / SIDE_STEP_SPACING).floor() as usize)
            .min(SIDE_STEP_COUNT)
            .max(1);
        let step_len = len / steps as f32;
        for step in 0..steps {
            let t_center = step_len * (step as f32 + 0.5);
            let mid_x = ax + ux * t_center;
            let mid_z = az + uz * t_center;
            for &side in &[1.0_f32, -1.0_f32] {
                let archetype = lcg(&mut global_rng);
                let mut placed = false;
                for _attempt in 0..BUILDING_ATTEMPTS {
                    let jitter = lcg_range(
                        lcg(&mut edge_rng),
                        -step_len * JITTER_FRACTION,
                        step_len * JITTER_FRACTION,
                    );
                    let extra_off = lcg_range(lcg(&mut edge_rng), 0.0, EXTRA_OFFSET_MAX);
                    let total_off = SIDE_OFFSET + extra_off;
                    let cx = mid_x + ux * jitter + rx * side * total_off;
                    let cz = mid_z + uz * jitter + rz * side * total_off;
                    if !footprint_clear(cx, cz, FOOTPRINT_R, &city) {
                        continue;
                    }
                    spawn_building(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        cx,
                        cz,
                        &mut edge_rng,
                        archetype,
                    );
                    placed = true;
                    break;
                }
                let _ = placed;
            }
        }
    }
}
