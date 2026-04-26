/*
prologue
Name of program: pedestrian.rs
Description: Procedural pedestrian system. Spawns pedestrians at intersections on randomized timers and walks them across to a connected node.
Author: Maren Proplesch
Date Created: 4/26/2026
Date Revised: 4/26/2026
Revision History: Initial implementation. Fixed crossing direction: pedestrians now walk perpendicular to the road (across it) rather than along it.
               Replaced rectangle mesh with pedestrian.glb animated model.
Preconditions: assets/pedestrian.glb must exist and contain at least one animation clip
Postconditions: Not applicable/Redundant
Citation: Used AI copilot for limited code generation - claude.ai
*/

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::scene::SceneInstanceReady;

use crate::ROAD_WIDTH;
use crate::map_parser::CityData;

pub const PEDESTRIAN_SLOW_RADIUS: f32 = 60.0;
pub const PEDESTRIAN_SLOW_FRACTION: f32 = 0.25;
const PED_Y_OFFSET: f32 = 7.0;

const PEDESTRIAN_SPEED: f32 = 3.6;
const PEDESTRIAN_HEIGHT: f32 = 1.0;
const PROXIMITY: f32 = 5.0;
const TIMER_MIN: f32 = 5.0;
const TIMER_MAX: f32 = 20.0;
const PATH_LINE_HEIGHT: f32 = 11.5;
const PATH_LINE_THICKNESS: f32 = 1.0;
// How far either side of the road centerline the pedestrian starts and ends
const CROSS_HALF: f32 = ROAD_WIDTH * 0.7;

// Background dancer crowd placed outside the city at startup
const DANCER_COUNT: usize = 6;
const DANCER_SCALE: f32 = 3200.0;
const DANCER_RADIUS_MIN: f32 = 16000.0;
const DANCER_RADIUS_MAX: f32 = 17000.0;
const DANCER_HEIGHT: f32 = 8.0;

// Holds the countdown timer and all pre-baked spawn data for one intersection crossing;
// one timer is created per directed road edge so every road at every intersection gets crossings;
// world positions are stored directly so the per-frame spawn system never needs CityData
// spawn_pos: start of the crossing, on one side of the road
// dst_pos: end of the crossing, on the other side of the road
// dir: normalized direction across the road (perpendicular to traffic)
// remaining: seconds until the next spawn
#[derive(Component)]
pub struct PedestrianSpawnTimer {
    pub spawn_pos: Vec3,
    pub dst_pos: Vec3,
    pub dir: Vec3,
    pub remaining: f32,
}

#[derive(Component)]
pub struct DancerRoot;

// Active pedestrian walking across the road
// dst: world-space destination
// dir: normalized XZ movement direction (Y=0)
// pos: current world position updated each frame for car avoidance
#[derive(Component)]
pub struct Pedestrian {
    pub dst: Vec3,
    pub dir: Vec3,
    pub pos: Vec3,
}

// Attached to the path line entity so it is despawned alongside its pedestrian
#[derive(Component)]
pub struct PedestrianPathLine {
    pub owner: Entity,
}

// Marks a pedestrian scene root so the animation starter system can find it and
// play the walk clip once the GLB scene and its AnimationPlayer have been spawned
#[derive(Component)]
pub struct PedestrianSceneRoot;

// Shared GPU asset handles created once at startup and reused for every pedestrian
#[derive(Resource)]
pub struct PedestrianAssets {
    pub scene: Handle<Scene>,
    pub path_material: Handle<StandardMaterial>,
    // AnimationGraph shared by all pedestrians; contains the single walk clip
    pub anim_graph: Handle<AnimationGraph>,
    // Index of the walk node inside the shared graph
    pub walk_node: AnimationNodeIndex,
}

// Advances an LCG and maps the result to an f32 in [lo, hi)
// Input: seed: &mut u64 LCG state mutated in place; lo: f32; hi: f32
// Returns: f32 in [lo, hi)
fn lcg_range(seed: &mut u64, lo: f32, hi: f32) -> f32 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let f = (*seed >> 33) as f32 / u32::MAX as f32;
    lo + f * (hi - lo)
}

// Computes a crossing from one side of a road to the other at the intersection node;
// the road runs from (sx,sz) toward (nx,nz), so the crossing direction is perpendicular to that;
// the pedestrian starts at node_center - perp * CROSS_HALF and ends at node_center + perp * CROSS_HALF,
// positioned slightly along the road from the node so they cross just at the intersection edge
// Input: nx, nz: f32 intersection node world XZ (crossing center); sx, sz: f32 neighbor node XZ (defines road direction)
// Returns: Option<(Vec3 spawn_pos, Vec3 dst_pos, Vec3 dir)> or None when road direction is degenerate
fn crossing_positions(nx: f32, nz: f32, sx: f32, sz: f32) -> Option<(Vec3, Vec3, Vec3)> {
    let road = Vec2::new(sx - nx, sz - nz);
    let road_len = road.length();
    if road_len < 1.0 {
        return None;
    }
    let along = road / road_len;
    let perp = Vec2::new(-along.y, along.x);
    let offset_along = along * (ROAD_WIDTH * 0.5);
    let center = Vec3::new(nx + offset_along.x, PEDESTRIAN_HEIGHT, nz + offset_along.y);
    let perp3 = Vec3::new(perp.x, 0.0, perp.y);
    let spawn_pos = center - perp3 * CROSS_HALF;
    let dst_pos = center + perp3 * CROSS_HALF;
    Some((spawn_pos, dst_pos, perp3))
}

// Startup system that creates shared PedestrianAssets and spawns one PedestrianSpawnTimer per
// road edge at each intersection; each timer represents a crosswalk perpendicular to that edge;
// all world positions are pre-baked so per-frame systems never access CityData.
// Loads pedestrian.glb and pre-builds a shared AnimationGraph containing the first clip.
// Input: commands: Commands; asset_server: Res<AssetServer>; meshes: ResMut<Assets<Mesh>>; materials: ResMut<Assets<StandardMaterial>>; anim_graphs: ResMut<Assets<AnimationGraph>>; city: NonSend<CityData>
// Returns: none
pub fn setup_pedestrian_timers(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut anim_graphs: ResMut<Assets<AnimationGraph>>,
    city: NonSend<CityData>,
) {
    let scene: Handle<Scene> = asset_server.load("pedestrian.glb#Scene0");
    let walk_clip: Handle<AnimationClip> = asset_server.load("pedestrian.glb#Animation0");
    let mut graph = AnimationGraph::new();
    let walk_node = graph.add_clip(walk_clip, 1.0, graph.root);
    let anim_graph = anim_graphs.add(graph);
    let path_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.85, 0.3, 0.6),
        emissive: LinearRgba::new(0.6, 0.5, 0.1, 1.0),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let scene_handle = scene.clone();
    commands.insert_resource(PedestrianAssets {
        scene,
        path_material,
        anim_graph,
        walk_node,
    });
    let mut seed: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0xc0ffee_u64);

    for (id, node) in &city.nodes {
        if id.starts_with('E') {
            continue;
        }
        for nb_id in &node.connects {
            if nb_id.starts_with('E') {
                continue;
            }
            let Some(nb_node) = city.nodes.get(nb_id.as_str()) else {
                continue;
            };
            let Some((spawn_pos, dst_pos, dir)) =
                crossing_positions(node.x, node.y, nb_node.x, nb_node.y)
            else {
                continue;
            };
            let remaining = lcg_range(&mut seed, TIMER_MIN, TIMER_MAX);
            commands.spawn(PedestrianSpawnTimer {
                spawn_pos,
                dst_pos,
                dir,
                remaining,
            });
        }
    }
    let radius = (DANCER_RADIUS_MIN + DANCER_RADIUS_MAX) * 0.5;
    for i in 0..DANCER_COUNT {
        let angle = (i as f32 / DANCER_COUNT as f32) * std::f32::consts::TAU;
        let x = angle.cos() * radius;
        let z = angle.sin() * radius;
        let face = angle + std::f32::consts::PI;
        commands
            .spawn((
                SceneRoot(scene_handle.clone()),
                Transform::from_xyz(x, DANCER_HEIGHT, z)
                    .with_rotation(Quat::from_rotation_y(face))
                    .with_scale(Vec3::splat(DANCER_SCALE)),
                Visibility::Inherited,
                PedestrianSceneRoot,
                DancerRoot,
            ))
            .observe(on_pedestrian_scene_ready);
    }
}

// Ticks all PedestrianSpawnTimer components each frame; when a timer expires it spawns a GLB
// pedestrian scene and a matching path line showing the crossing, then resets with a new random delay.
// The spawned root carries PedestrianSceneRoot so pedestrian_animation_system can find new arrivals.
// Input: commands: Commands; time: Res<Time>; assets: Res<PedestrianAssets>; timers: Query<&mut PedestrianSpawnTimer>; meshes: ResMut<Assets<Mesh>>
// Returns: none
pub fn pedestrian_spawn_system(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<PedestrianAssets>,
    mut timers: Query<&mut PedestrianSpawnTimer>,
    mut meshes: ResMut<Assets<Mesh>>,
    peds: Query<&Pedestrian>,
) {
    let dt = time.delta_secs();
    let mut seed: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0xbeefdead_u64);

    for mut timer in timers.iter_mut() {
        timer.remaining -= dt;
        if timer.remaining > 0.0 {
            continue;
        }
        timer.remaining = lcg_range(&mut seed, TIMER_MIN, TIMER_MAX);
        let spawn_pos = timer.spawn_pos;
        let crossing_blocked = peds.iter().any(|ped| {
            let dx = ped.pos.x - spawn_pos.x;
            let dz = ped.pos.z - spawn_pos.z;
            let dist_sq = dx * dx + dz * dz;
            dist_sq < (CROSS_HALF * 2.0) * (CROSS_HALF * 2.0)
        });
        if crossing_blocked {
            continue;
        }
        let dst_pos = timer.dst_pos;
        let dir = timer.dir;
        let facing_angle = (dir.z).atan2(dir.x);
        let rotation = Quat::from_rotation_y(-facing_angle);
        let ped_entity = commands
            .spawn((
                SceneRoot(assets.scene.clone()),
                Transform::from_translation(spawn_pos + Vec3::new(0.0, PED_Y_OFFSET, 0.0))
                    .with_rotation(rotation)
                    .with_scale(Vec3::splat(4.5)),
                Visibility::Inherited,
                Pedestrian {
                    dst: dst_pos,
                    dir,
                    pos: spawn_pos,
                },
                PedestrianSceneRoot,
            ))
            .observe(on_pedestrian_scene_ready)
            .id();
        let flat_len = Vec2::new(dst_pos.x - spawn_pos.x, dst_pos.z - spawn_pos.z).length();
        let mid = (spawn_pos + dst_pos) * 0.5;
        let angle = (dst_pos.z - spawn_pos.z).atan2(dst_pos.x - spawn_pos.x);
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(flat_len, 1.0, PATH_LINE_THICKNESS))),
            MeshMaterial3d(assets.path_material.clone()),
            Transform::from_xyz(mid.x, PATH_LINE_HEIGHT, mid.z)
                .with_rotation(Quat::from_rotation_y(-angle)),
            Visibility::Inherited,
            PedestrianPathLine { owner: ped_entity },
        ));
    }
}

// Observer callback triggered by SceneInstanceReady on each pedestrian root entity.
// Bevy 0.15+ uses observers instead of EventReader; by attaching this observer directly
// to the spawned entity, it fires exactly once when that scene finishes loading.
// Walks all descendants to find the AnimationPlayer and starts the walk clip looping.
// Input: trigger: Trigger<SceneInstanceReady>; assets: Res<PedestrianAssets>;
//        children_query: Query to walk the hierarchy; players: Query<&mut AnimationPlayer>
// Returns: none
pub fn on_pedestrian_scene_ready(
    trigger: On<SceneInstanceReady>,
    mut commands: Commands,
    assets: Res<PedestrianAssets>,
    children_query: Query<&Children>,
    mut players: Query<&mut AnimationPlayer>,
    dancer_roots: Query<Entity, With<DancerRoot>>,
) {
    let root = trigger.event_target();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if let Ok(mut player) = players.get_mut(entity) {
            // AnimationGraphHandle must be on the same entity as AnimationPlayer in Bevy 0.15+
            commands
                .entity(entity)
                .insert(AnimationGraphHandle(assets.anim_graph.clone()));
            player.play(assets.walk_node).repeat();
            break;
        }
        if let Ok(children) = children_query.get(entity) {
            stack.extend(children.iter());
        }
    }
    if dancer_roots.contains(root) {
        let mut shadow_stack = vec![root];
        while let Some(e) = shadow_stack.pop() {
            commands
                .entity(e)
                .insert((NotShadowCaster, NotShadowReceiver));
            if let Ok(children) = children_query.get(e) {
                shadow_stack.extend(children.iter());
            }
        }
    }
}

// Advances all active pedestrians toward their destination each frame, despawning both the
// pedestrian and its path line once it arrives within PROXIMITY of the destination
// Input: commands: Commands; time: Res<Time>; peds: Query over Pedestrian entities; lines: Query over PedestrianPathLine entities
// Returns: none
pub fn pedestrian_move_system(
    mut commands: Commands,
    time: Res<Time>,
    mut peds: Query<(Entity, &mut Transform, &mut Pedestrian)>,
    lines: Query<(Entity, &PedestrianPathLine)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut ped) in peds.iter_mut() {
        let flat_dist = Vec2::new(
            ped.dst.x - transform.translation.x,
            ped.dst.z - transform.translation.z,
        )
        .length();

        if flat_dist < PROXIMITY {
            for (line_entity, line) in lines.iter() {
                if line.owner == entity {
                    commands.entity(line_entity).despawn();
                }
            }
            commands.entity(entity).despawn();
            continue;
        }

        transform.translation += ped.dir * PEDESTRIAN_SPEED * dt;
        ped.pos = transform.translation;
    }
}
