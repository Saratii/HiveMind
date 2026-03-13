pub mod cameras;
pub mod car_emulator;
pub mod map_parser;

use bevy::{
    diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin},
    pbr::wireframe::WireframePlugin,
    picking::prelude::*,
    prelude::*,
    ui::Node as UiNode,
    winit::{UpdateMode, WinitSettings},
};
use iyes_perf_ui::PerfUiPlugin;
use iyes_perf_ui::prelude::PerfUiDefaultEntries;
use std::collections::{HashMap, HashSet};
use std::f32::consts::PI;
use std::sync::{Arc, Mutex};

use crate::{
    cameras::{orbit_camera, zoom_camera},
    car_emulator::{
        CarHttp, CarLicense, CarPhysics, PreRoad, PreRoadPhase, pre_road_system,
        spawn_car_listener, update_car_physics,
    },
    map_parser::{CityData, PortalMarker, Waypoint, parse_city},
};

const CITY_JSON_PATH: &str = "../city.json";
const SERVER_URL: &str = "http://127.0.0.1:8080";
const REGISTER_CAR_ENDPOINT: &str = "/register-car";
const VALIDATE_ENTRY_ENDPOINT: &str = "/validate-entry";
const ACCELERATION: f32 = 50.0;
const EXIT_DRIVE_SPEED: f32 = 80.0;
const BATCH_SPAWN_COUNT: usize = 20;

#[derive(Resource, Default)]
struct PortalSelection {
    first: Option<usize>,
    highlighted_entity: Option<Entity>,
    next_car_id: usize,
    next_port: u16,
}

impl PortalSelection {
    fn new() -> Self {
        Self {
            first: None,
            highlighted_entity: None,
            next_car_id: 1,
            next_port: 8081,
        }
    }
}

#[derive(Resource)]
struct PortalMaterials {
    normal: Handle<StandardMaterial>,
    highlighted: Handle<StandardMaterial>,
}

#[derive(Component)]
struct SpawnBatchButton;

fn parse_form(body: &str) -> HashMap<String, String> {
    body.split('&')
        .filter_map(|pair| {
            let mut kv = pair.splitn(2, '=');
            let k = kv.next()?.trim().to_string();
            let v = kv.next().unwrap_or("").trim().to_string();
            Some((k, v))
        })
        .collect()
}

#[derive(Resource)]
pub struct Orbit {
    yaw: f32,
    pitch: f32,
    radius: f32,
    focus: Vec3,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            yaw: -PI / 4.0,
            pitch: PI / 4.0,
            radius: 3200.0,
            focus: Vec3::ZERO,
        }
    }
}

fn orbit_pos(o: &Orbit) -> Vec3 {
    o.focus
        + Vec3::new(
            o.radius * o.pitch.cos() * o.yaw.sin(),
            o.radius * o.pitch.sin(),
            o.radius * o.pitch.cos() * o.yaw.cos(),
        )
}

#[derive(Component, Clone, Copy)]
struct CarColor(Color);

fn lcg_step(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *seed
}

fn rand_car_color() -> Color {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(12345);
    let mut seed = nanos ^ count.wrapping_mul(2654435761);
    seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let hue = ((seed >> 33) as f32 / u32::MAX as f32) * 360.0;
    let h = hue / 60.0;
    let i = h as u32 % 6;
    let f = h - h.floor();
    let (r, g, b) = match i {
        0 => (1.0_f32, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };
    Color::srgb(r, g, b)
}

#[derive(Component)]
struct NodeLabel {
    world_pos: Vec3,
}

fn update_node_labels(
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut labels: Query<(&NodeLabel, &mut Transform)>,
) {
    let Ok((camera, cam_gtf)) = camera_q.single() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let win_size = Vec2::new(window.width(), window.height());
    for (label, mut transform) in labels.iter_mut() {
        if let Some(ndc) = camera.world_to_ndc(cam_gtf, label.world_pos) {
            if ndc.z < 0.0 || ndc.z > 1.0 {
                continue;
            }
            let screen_x = ndc.x * win_size.x * 0.5;
            let screen_y = ndc.y * win_size.y * 0.5;
            transform.translation.x = screen_x;
            transform.translation.y = screen_y;
            transform.translation.z = 1.0;
        }
    }
}

fn on_portal_click(
    event: On<Pointer<Click>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    portal_mats: Res<PortalMaterials>,
    city: NonSend<CityData>,
    mut selection: ResMut<PortalSelection>,
    portal_markers: Query<&PortalMarker>,
    existing_cars: Query<&PreRoad>,
    mut mat_handles: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let clicked_entity = event.entity;
    let Ok(marker) = portal_markers.get(clicked_entity) else {
        return;
    };
    let clicked_idx = marker.portal_index;
    match selection.first {
        None => {
            if let Ok(mut mat) = mat_handles.get_mut(clicked_entity) {
                mat.0 = portal_mats.highlighted.clone();
            }
            selection.first = Some(clicked_idx);
            selection.highlighted_entity = Some(clicked_entity);
        }
        Some(src_idx) => {
            if let Some(prev_ent) = selection.highlighted_entity {
                if let Ok(mut mat) = mat_handles.get_mut(prev_ent) {
                    mat.0 = portal_mats.normal.clone();
                }
            }
            selection.first = None;
            selection.highlighted_entity = None;
            if clicked_idx == src_idx {
                return;
            }
            let route_taken = existing_cars.iter().any(|pre| {
                pre.src_node_id == city.portals[src_idx].node.id
                    && pre.dst_node_id == city.portals[clicked_idx].node.id
            });
            if route_taken {
                println!(
                    "Route {}->{} already in use, ignoring",
                    src_idx, clicked_idx
                );
                return;
            }
            let src = &city.portals[src_idx];
            let [sx, sz] = src.center;
            let (ex, ez) = (src.node.x, src.node.y);
            let center_xz = Vec2::new(sx, sz);
            let exit_xz = Vec2::new(ex, ez);
            let to_exit = exit_xz - center_xz;
            let wait_xz = if to_exit.length() > 3.0 {
                exit_xz - to_exit.normalize() * 3.0
            } else {
                center_xz
            };
            let i = selection.next_car_id;
            selection.next_car_id += 1;
            let port = selection.next_port;
            selection.next_port += 1;
            let license = format!("CAR-{:03}", i);
            let car_url = format!("http://127.0.0.1:{}", port);
            let register_url = format!("{}{}", SERVER_URL, REGISTER_CAR_ENDPOINT);
            let validate_url = format!("{}{}", SERVER_URL, VALIDATE_ENTRY_ENDPOINT);
            let car_color = rand_car_color();
            let car_mat = materials.add(StandardMaterial {
                base_color: car_color,
                emissive: {
                    let lc = car_color.to_linear();
                    LinearRgba::new(lc.red * 0.8, lc.green * 0.8, lc.blue * 0.8, 1.0)
                },
                ..default()
            });
            println!(
                "Spawning {} on port {} from portal {} to portal {}",
                license, port, src_idx, clicked_idx
            );
            let http_state = Arc::new(Mutex::new(CarHttp::new(sx, sz)));
            spawn_car_listener(port, Arc::clone(&http_state));
            let car_mesh = meshes.add(Sphere::new(10.0));
            commands.spawn((
                Mesh3d(car_mesh),
                MeshMaterial3d(car_mat),
                Transform::from_xyz(sx, 30.0, sz),
                CarColor(car_color),
                CarLicense(license.clone()),
                PreRoad {
                    phase: PreRoadPhase::DrivingToWait,
                    wait_target: Vec3::new(wait_xz.x, 30.0, wait_xz.y),
                    road_entry: Vec3::new(ex, 30.0, ez),
                    license,
                    car_url,
                    register_url,
                    validate_url,
                    src_node_id: src.node.id.clone(),
                    dst_node_id: city.portals[clicked_idx].node.id.clone(),
                    polling_in_flight: false,
                },
                CarPhysics {
                    http: http_state,
                    speed: 0.0,
                    dir_x: 1.0,
                    dir_z: 0.0,
                },
            ));
        }
    }
}

fn main() {
    let json = std::fs::read_to_string(CITY_JSON_PATH)
        .unwrap_or_else(|e| panic!("Could not read {}: {}", CITY_JSON_PATH, e));
    let city = parse_city(&json);
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "City Renderer".into(),
                    resolution: [1280_u32, 720_u32].into(),
                    present_mode: bevy::window::PresentMode::AutoNoVsync,
                    ..default()
                }),
                ..default()
            }),
            WireframePlugin {
                debug_flags: default(),
            },
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin::default(),
            PerfUiPlugin,
            bevy::picking::mesh_picking::MeshPickingPlugin,
        ))
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        .insert_resource(Orbit::default())
        .insert_resource(PortalSelection::new())
        .insert_non_send_resource(city)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                orbit_camera,
                zoom_camera,
                update_node_labels,
                pre_road_system,
                update_car_physics,
                spawn_path_segments,
                spawn_batch_button_system,
            ),
        )
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    city: NonSend<CityData>,
) {
    commands.spawn(PerfUiDefaultEntries::default());
    let portal_normal_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.8, 0.3),
        ..default()
    });
    let portal_highlight_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.85, 0.1),
        emissive: LinearRgba::new(0.6, 0.5, 0.0, 1.0),
        ..default()
    });
    commands.insert_resource(PortalMaterials {
        normal: portal_normal_mat.clone(),
        highlighted: portal_highlight_mat,
    });
    let portal_mat = portal_normal_mat;
    let exit_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.0, 1.0),
        emissive: LinearRgba::new(0.4, 0.0, 1.0, 1.0),
        ..default()
    });
    let road_thickness = 20.0_f32;
    let road_height = 10.0_f32;
    let exit_mesh = meshes.add(Sphere::new(8.0));
    let mut road_color_seed: u64 = 0xf00dcafe_deadbeef;
    let mut rendered_edges: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    for (id, node) in &city.nodes {
        for nb_id in &node.connects {
            let key = if id.as_str() < nb_id.as_str() {
                (id.clone(), nb_id.clone())
            } else {
                (nb_id.clone(), id.clone())
            };
            if rendered_edges.contains(&key) {
                continue;
            }
            rendered_edges.insert(key);
            let (ax, az) = (node.x, node.y);
            let (bx, bz) = city.node_pos(nb_id);
            let diff = Vec2::new(bx - ax, bz - az);
            let len = diff.length();
            if len < 0.01 {
                continue;
            }
            let mid = Vec2::new((ax + bx) * 0.5, (az + bz) * 0.5);
            let angle = diff.y.atan2(diff.x);
            let raw = lcg_step(&mut road_color_seed);
            let grey = 0.25 + ((raw >> 33) as f32 / u32::MAX as f32) * 0.45;
            let road_mat = materials.add(StandardMaterial {
                base_color: Color::srgb(grey, grey, grey),
                ..default()
            });
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(len, road_height, road_thickness))),
                MeshMaterial3d(road_mat),
                Transform::from_xyz(mid.x, road_height * 0.5, mid.y)
                    .with_rotation(Quat::from_rotation_y(-angle)),
                Pickable::IGNORE,
            ));
        }
    }
    for (portal_index, portal) in city.portals.iter().enumerate() {
        let [cx, cz] = portal.center;
        let (ex, ez) = (portal.node.x, portal.node.y);
        commands
            .spawn((
                Mesh3d(meshes.add(Cuboid::new(200.0, 5.0, 200.0))),
                MeshMaterial3d(portal_mat.clone()),
                Transform::from_xyz(cx, 2.5, cz),
                PortalMarker { portal_index },
                Pickable::default(),
            ))
            .observe(on_portal_click);
        commands.spawn((
            Mesh3d(exit_mesh.clone()),
            MeshMaterial3d(exit_mat.clone()),
            Transform::from_xyz(ex, 18.0, ez),
            Pickable::IGNORE,
        ));
    }
    let font = TextFont {
        font_size: 14.0,
        ..default()
    };
    for (id, node) in &city.nodes {
        if id.starts_with('E') {
            continue;
        }
        let world_pos = Vec3::new(node.x, 0.0, node.y);
        let label_color = if id.starts_with('P') {
            Color::srgb(1.0, 0.4, 1.0)
        } else {
            Color::srgb(1.0, 1.0, 0.6)
        };
        commands.spawn((
            Text2d::new(id.clone()),
            font.clone(),
            TextColor(label_color),
            Transform::from_xyz(0.0, 0.0, 1.0),
            NodeLabel { world_pos },
        ));
    }

    commands.spawn(AmbientLight {
        color: Color::WHITE,
        brightness: 500.0,
        affects_lightmapped_meshes: true,
    });
    commands.spawn((
        DirectionalLight {
            illuminance: 15_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -PI / 4.0, PI / 5.0, 0.0)),
    ));
    let pos = orbit_pos(&Orbit::default());
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            ..default()
        },
        Transform::from_translation(pos).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
    ));
    commands
        .spawn((
            UiNode {
                position_type: PositionType::Absolute,
                bottom: Val::Px(20.0),
                left: Val::Px(20.0),
                width: Val::Px(160.0),
                height: Val::Px(48.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            Button,
            SpawnBatchButton,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("Spawn 20"),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
        });
}

#[derive(Component)]
struct PathRendered;

fn spawn_path_segments(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    q: Query<
        (Entity, &CarPhysics, &CarColor, &CarLicense),
        (Without<PreRoad>, Without<PathRendered>),
    >,
) {
    for (entity, physics, car_color, car_license) in q.iter() {
        let waypoints: Vec<Waypoint> = {
            let h = physics.http.lock().unwrap();
            h.waypoints.clone()
        };
        if waypoints.is_empty() {
            continue;
        }
        let seg_height = 6.0;
        let seg_thickness = 12.0;
        let y_offset = 20.0;
        let path_mat = materials.add(StandardMaterial {
            base_color: car_color.0,
            emissive: {
                let lc = car_color.0.to_linear();
                LinearRgba::new(lc.red * 1.5, lc.green * 1.5, lc.blue * 1.5, 1.0)
            },
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        for i in 0..waypoints.len().saturating_sub(1) {
            let a = &waypoints[i];
            let b = &waypoints[i + 1];
            let diff = Vec2::new(b.x - a.x, b.z - a.z);
            let len = diff.length();
            if len < 0.01 {
                continue;
            }
            let mid = Vec2::new((a.x + b.x) * 0.5, (a.z + b.z) * 0.5);
            let angle = diff.y.atan2(diff.x);
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(len, seg_height, seg_thickness))),
                MeshMaterial3d(path_mat.clone()),
                Transform::from_xyz(mid.x, y_offset, mid.y)
                    .with_rotation(Quat::from_rotation_y(-angle)),
                CarLicense(car_license.0.clone()),
                Pickable::IGNORE,
            ));
        }
        commands.entity(entity).insert(PathRendered);
    }
}

fn spawn_batch_button_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    interaction_q: Query<&Interaction, (Changed<Interaction>, With<SpawnBatchButton>)>,
    city: NonSend<CityData>,
    mut selection: ResMut<PortalSelection>,
    existing_cars: Query<&PreRoad>,
) {
    let clicked = interaction_q.iter().any(|i| *i == Interaction::Pressed);
    if !clicked {
        return;
    }
    let portals = &city.portals;
    if portals.len() < 2 {
        eprintln!("Not enough portals to spawn batch");
        return;
    }
    let mut used: HashSet<(String, String)> = existing_cars
        .iter()
        .map(|p| (p.src_node_id.clone(), p.dst_node_id.clone()))
        .collect();
    let mut rng_seed: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0xbada55_c0ffee)
        ^ (selection.next_car_id as u64).wrapping_mul(2654435761);
    let mut spawned = 0;
    let mut attempts = 0;
    let max_attempts = BATCH_SPAWN_COUNT * portals.len() * 2;
    while spawned < BATCH_SPAWN_COUNT && attempts < max_attempts {
        attempts += 1;
        rng_seed = rng_seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let src_idx = (rng_seed >> 33) as usize % portals.len();
        rng_seed = rng_seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let dst_idx = (rng_seed >> 33) as usize % portals.len();
        if src_idx == dst_idx {
            continue;
        }
        let src_id = portals[src_idx].node.id.clone();
        let dst_id = portals[dst_idx].node.id.clone();
        let route_key = (src_id.clone(), dst_id.clone());
        if used.contains(&route_key) {
            continue;
        }
        used.insert(route_key);
        let src = &portals[src_idx];
        let [sx, sz] = src.center;
        let (ex, ez) = (src.node.x, src.node.y);
        let center_xz = Vec2::new(sx, sz);
        let exit_xz = Vec2::new(ex, ez);
        let to_exit = exit_xz - center_xz;
        let wait_xz = if to_exit.length() > 3.0 {
            exit_xz - to_exit.normalize() * 3.0
        } else {
            center_xz
        };
        let i = selection.next_car_id;
        selection.next_car_id += 1;
        let port = selection.next_port;
        selection.next_port += 1;
        let license = format!("CAR-{:03}", i);
        let car_url = format!("http://127.0.0.1:{}", port);
        let register_url = format!("{}{}", SERVER_URL, REGISTER_CAR_ENDPOINT);
        let validate_url = format!("{}{}", SERVER_URL, VALIDATE_ENTRY_ENDPOINT);
        let car_color = rand_car_color();
        let car_mat = materials.add(StandardMaterial {
            base_color: car_color,
            emissive: {
                let lc = car_color.to_linear();
                LinearRgba::new(lc.red * 0.8, lc.green * 0.8, lc.blue * 0.8, 1.0)
            },
            ..default()
        });
        println!(
            "Batch spawning {} port {} : {} -> {}",
            license, port, src_idx, dst_idx
        );
        let http_state = Arc::new(Mutex::new(CarHttp::new(sx, sz)));
        spawn_car_listener(port, Arc::clone(&http_state));
        let car_mesh = meshes.add(Sphere::new(25.0));
        commands.spawn((
            Mesh3d(car_mesh),
            MeshMaterial3d(car_mat),
            Transform::from_xyz(sx, 30.0, sz),
            CarColor(car_color),
            CarLicense(license.clone()),
            PreRoad {
                phase: PreRoadPhase::DrivingToWait,
                wait_target: Vec3::new(wait_xz.x, 30.0, wait_xz.y),
                road_entry: Vec3::new(ex, 30.0, ez),
                license,
                car_url,
                register_url,
                validate_url,
                src_node_id: src_id,
                dst_node_id: dst_id,
                polling_in_flight: false,
            },
            CarPhysics {
                http: http_state,
                speed: 0.0,
                dir_x: 1.0,
                dir_z: 0.0,
            },
        ));
        spawned += 1;
    }
    println!(
        "Batch spawn: {} cars spawned ({} attempts)",
        spawned, attempts
    );
}
