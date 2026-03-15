/*
prologue
Name of program: main.rs
Description: Main file for the rendering. Sets up a bevy app with various game display elements.
Author: Maren Proplesch
Date Created: 3/13/2026
Date Revised: 3/13/2026
Revision History: None
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
Citation: Used AI copilot for limited code generation - claude.ai
*/

pub mod cameras;
pub mod car_emulator;
pub mod map_parser;

use bevy::{
    core_pipeline::Skybox,
    diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin},
    image::{ImageArrayLayout, ImageLoaderSettings, ImageSampler},
    pbr::wireframe::WireframePlugin,
    picking::prelude::*,
    prelude::*,
    render::render_resource::{TextureViewDescriptor, TextureViewDimension},
    ui::{Node as UiNode, UiTargetCamera},
    winit::{UpdateMode, WinitSettings},
};
use iyes_perf_ui::PerfUiPlugin;
use iyes_perf_ui::prelude::PerfUiDefaultEntries;
use std::collections::{HashMap, HashSet};
use std::f32::consts::PI;
use std::sync::{Arc, Mutex};

use crate::{
    cameras::{CameraMode, FlyCamState, OrbitMomentum, flycam_system, orbit_camera, zoom_camera},
    car_emulator::{
        CarHttp, CarLicense, CarPhysics, ParkingIn, PostRoad, PreRoad, PreRoadPhase,
        car_facing_quat, parking_in_system, pre_road_system, spawn_car_listener,
        update_car_physics,
    },
    map_parser::{CityData, PortalMarker, Waypoint, parse_city},
};

const CITY_JSON_PATH: &str = "../city.json";
// const SERVER_URL: &str = "http://127.0.0.1:8080";
const SERVER_URL: &str = "http://52.15.156.213:8080";
const REGISTER_CAR_ENDPOINT: &str = "/register-car";
const VALIDATE_ENTRY_ENDPOINT: &str = "/validate-entry";
const ACCELERATION: f32 = 50.0;
const EXIT_DRIVE_SPEED: f32 = 80.0;
const BATCH_SPAWN_COUNT: usize = 20;
const CAR_SCALE: f32 = 5.0;
const ROAD_WIDTH: f32 = 24.0;

// Tracks which portal the user has clicked first when selecting a source-destination pair for spawning a car
// first: the index into the city portals list of the first selected portal, or None if no portal is currently selected
// highlighted_entity: the ECS entity of the currently highlighted portal mesh, used to restore its material on deselection
// next_car_id: monotonically increasing counter used to generate unique license plate strings for newly spawned cars
// next_port: the next TCP port number to assign to a car's HTTP listener, starting at 8081 and incrementing per car
#[derive(Resource, Default)]
struct PortalSelection {
    first: Option<usize>,
    highlighted_entity: Option<Entity>,
    next_car_id: usize,
    next_port: u16,
}

impl PortalSelection {
    // Constructs a PortalSelection with no active selection and counters set to their starting values
    // Input: none
    // Returns: PortalSelection with first and highlighted_entity set to None, next_car_id set to 1, and next_port set to 8081
    fn new() -> Self {
        Self {
            first: None,
            highlighted_entity: None,
            next_car_id: 1,
            next_port: 8081,
        }
    }
}

// Holds the two StandardMaterial handles used to visually distinguish unselected and selected portal entities
// normal: material applied to portals in their default unselected state
// highlighted: material applied to the first selected portal while waiting for the user to click a destination
#[derive(Resource)]
struct PortalMaterials {
    normal: Handle<StandardMaterial>,
    highlighted: Handle<StandardMaterial>,
}

// Holds the preloaded scene handle for the car GLB model and the shared hitbox mesh and material handles, inserted as a resource during startup and shared across all car spawn sites
// scene: handle to the first scene extracted from Car2.glb, used as the SceneRoot for every spawned car entity
// hitbox_mesh: handle to a box mesh sized to approximate the car footprint, rendered as a debug overlay on the physics parent
// hitbox_mat: handle to a semi-transparent red material applied to the hitbox mesh so it is visible over the road
// skybox: handle to the column PNG image so the fix_skybox_view system can set its texture view dimension to Cube after load
#[derive(Resource)]
struct CarAssets {
    scene: Handle<Scene>,
    skybox: Handle<Image>,
}

// Marker component attached to the UI button that triggers a batch spawn of randomly routed cars
#[derive(Component)]
struct SpawnBatchButton;

// Marker component attached to the UI button that toggles between orbit and fly camera modes
#[derive(Component)]
struct ToggleFlyCamButton;

// Parses a URL-encoded form body string into a key-value map, trimming whitespace from both keys and values
// Input: body: &str containing the raw URL-encoded form data
// Returns: HashMap<String, String> mapping each field name to its value
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

// Determines the local IP address that the OS would use to reach the remote server by opening a
// non-sending UDP socket toward it and reading back the chosen local address; this is the IP the
// remote server must use to call back into the car's HTTP listener
// Input: none
// Returns: String containing the local IP address as a dotted-decimal string
fn local_ip() -> String {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind UDP for IP detection");
    socket
        .connect("52.15.156.213:8080")
        .expect("connect UDP for IP detection");
    socket.local_addr().expect("local_addr").ip().to_string()
}

// Bevy resource storing the camera's current orbital state relative to a fixed focus point
// yaw: horizontal rotation angle in radians around the Y axis
// pitch: vertical rotation angle in radians, clamped to prevent flipping over the poles
// radius: distance from the focus point to the camera, controlling zoom level
// focus: the world space point the camera continuously looks at
#[derive(Resource)]
pub struct Orbit {
    yaw: f32,
    pitch: f32,
    radius: f32,
    focus: Vec3,
}

impl Default for Orbit {
    // Returns an Orbit positioned at a 45 degree angle and comfortable starting distance from the origin
    // Input: none
    // Returns: Orbit with yaw at -PI/4, pitch at PI/4, radius at 3200, and focus at the world origin
    fn default() -> Self {
        Self {
            yaw: -PI / 4.0,
            pitch: PI / 4.0,
            radius: 3200.0,
            focus: Vec3::ZERO,
        }
    }
}

// Computes the world space camera position for a given orbital state using spherical coordinates
// Input: o: &Orbit containing the current yaw, pitch, radius, and focus
// Returns: Vec3 representing the camera's position in world space
fn orbit_pos(o: &Orbit) -> Vec3 {
    o.focus
        + Vec3::new(
            o.radius * o.pitch.cos() * o.yaw.sin(),
            o.radius * o.pitch.sin(),
            o.radius * o.pitch.cos() * o.yaw.cos(),
        )
}

// ECS component storing the display color assigned to a car, used when rendering its path segments
// 0: the Bevy Color value assigned to this car at spawn time
#[derive(Component, Clone, Copy)]
struct CarColor(Color);

// Generates a unique fully saturated random hue color for each car by combining a monotonic counter with the current system nanoseconds to seed an LCG
// Input: none
// Returns: Color with a randomly selected hue, full saturation, and full brightness
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

// ECS component that stores the fixed world position of a node so that its 2D text label can be projected to screen space each frame
// world_pos: the 3D world space position corresponding to the graph node this label belongs to
#[derive(Component)]
struct NodeLabel {
    world_pos: Vec3,
}

// Projects each node label's stored world position into screen space each frame and updates the label's 2D transform so it tracks its node in the viewport
// Input: windows: Query<&Window> for the primary window dimensions; camera_q: Query for the 3D camera's Camera and GlobalTransform; labels: Query over NodeLabel and Transform pairs
// Returns: none
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

// Computes a right-lane lateral offset vector perpendicular to a direction of travel, displacing the car to the right side of the road by one quarter of ROAD_WIDTH
// Input: dir: Vec2 normalized direction of travel from source node toward destination node
// Returns: Vec2 lateral offset to add to a centerline position to place the car in the right lane
fn right_lane_offset(dir: Vec2) -> Vec2 {
    let right = Vec2::new(dir.y, -dir.x);
    right * (ROAD_WIDTH * 0.25)
}

// Handles primary click events on portal entities, managing a two-click selection flow where the first click highlights the source portal and the second click spawns a car routed between the two selected portals
// Input: event: On<Pointer<Click>> carrying the clicked entity and button; commands: Commands for spawning the car entity; portal_mats: Res<PortalMaterials> for highlight toggling; car_assets: Res<CarAssets> for the shared car scene handle; city: NonSend<CityData> for portal coordinates and node IDs; selection: ResMut<PortalSelection> tracking current selection state; portal_markers: Query<&PortalMarker> to read the clicked portal's index; existing_cars: Query<&PreRoad> to prevent duplicate routes; mat_handles: Query for mutating portal mesh materials
// Returns: none
fn on_portal_click(
    event: On<Pointer<Click>>,
    mut commands: Commands,
    portal_mats: Res<PortalMaterials>,
    car_assets: Res<CarAssets>,
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
            let travel_dir = if to_exit.length() > 1e-6 {
                to_exit.normalize()
            } else {
                Vec2::X
            };
            let lane_offset = right_lane_offset(travel_dir);
            let spawn_xz = Vec2::new(sx, sz) + lane_offset;
            let wait_xz_offset = wait_xz + lane_offset;
            let road_entry_xz = exit_xz + lane_offset;
            let i = selection.next_car_id;
            selection.next_car_id += 1;
            let port = selection.next_port;
            selection.next_port += 1;
            let license = format!("CAR-{:03}", i);
            let car_url = format!("http://{}:{}", local_ip(), port);
            let register_url = format!("{}{}", SERVER_URL, REGISTER_CAR_ENDPOINT);
            let validate_url = format!("{}{}", SERVER_URL, VALIDATE_ENTRY_ENDPOINT);
            let car_color = rand_car_color();
            println!(
                "Spawning {} on port {} from portal {} to portal {}",
                license, port, src_idx, clicked_idx
            );
            let dst_center = city.portals[clicked_idx].center;
            let http_state = Arc::new(Mutex::new(CarHttp::new(spawn_xz.x, spawn_xz.y)));
            spawn_car_listener(port, Arc::clone(&http_state));
            commands
                .spawn((
                    Transform::from_xyz(spawn_xz.x, 13.5, spawn_xz.y),
                    Visibility::Inherited,
                    CarColor(car_color),
                    CarLicense(license.clone()),
                    PreRoad {
                        phase: PreRoadPhase::DrivingToWait,
                        wait_target: Vec3::new(wait_xz_offset.x, 13.5, wait_xz_offset.y),
                        road_entry: Vec3::new(road_entry_xz.x, 13.5, road_entry_xz.y),
                        license,
                        car_url,
                        register_url,
                        validate_url,
                        src_node_id: src.node.id.clone(),
                        dst_node_id: city.portals[clicked_idx].node.id.clone(),
                        polling_in_flight: false,
                    },
                    PostRoad {
                        center: Vec3::new(dst_center[0], 13.5, dst_center[1]),
                    },
                    CarPhysics {
                        http: http_state,
                        speed: 0.0,
                        dir_x: 1.0,
                        dir_z: 0.0,
                    },
                ))
                .with_children(|parent| {
                    parent.spawn((
                        SceneRoot(car_assets.scene.clone()),
                        Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::splat(CAR_SCALE)),
                    ));
                    parent.spawn((Transform::IDENTITY, Pickable::IGNORE));
                });
        }
    }
}

// Entry point that loads the city JSON, builds the Bevy app with all plugins and systems, and starts the main loop
// Input: none
// Returns: none
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
        .insert_resource(OrbitMomentum::default())
        .insert_resource(CameraMode::default())
        .insert_resource(FlyCamState::default())
        .insert_resource(PortalSelection::new())
        .insert_non_send_resource(city)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                orbit_camera,
                zoom_camera,
                flycam_system,
                toggle_flycam_system,
                update_node_labels,
                pre_road_system,
                update_car_physics,
                parking_in_system,
                update_car_rotation,
                spawn_path_segments,
                despawn_passed_segments,
                spawn_batch_button_system,
                fix_skybox_view,
            ),
        )
        .run();
}

// Startup system that spawns all static scene geometry including road segments, portal pads, exit markers, node labels, lighting, and both the 3D orbital camera and the 2D overlay camera, and preloads the car FBX model into the CarAssets resource
// Input: commands: Commands for spawning all scene entities; meshes: ResMut<Assets<Mesh>> for road and portal geometry; materials: ResMut<Assets<StandardMaterial>> for road, portal, and exit materials; city: NonSend<CityData> providing the full city node and portal data; asset_server: Res<AssetServer> for loading the car FBX model
// Returns: none
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    city: NonSend<CityData>,
    asset_server: Res<AssetServer>,
) {
    let car_scene = asset_server.load("porsche.glb#Scene0");
    let skybox_image =
        asset_server.load_with_settings("skybox.png", |s: &mut ImageLoaderSettings| {
            s.sampler = ImageSampler::linear();
            s.array_layout = Some(ImageArrayLayout::RowCount { rows: 6 });
        });
    commands.insert_resource(CarAssets {
        scene: car_scene,
        skybox: skybox_image.clone(),
    });
    let portal_normal_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.08, 0.08),
        perceptual_roughness: 0.95,
        ..default()
    });
    let portal_highlight_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.85, 0.1),
        emissive: LinearRgba::new(0.6, 0.5, 0.0, 1.0),
        ..default()
    });
    commands.insert_resource(PortalMaterials {
        normal: portal_normal_mat,
        highlighted: portal_highlight_mat,
    });
    let exit_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.0, 1.0),
        emissive: LinearRgba::new(0.4, 0.0, 1.0, 1.0),
        ..default()
    });
    let road_height = 10.0_f32;
    let exit_mesh = meshes.add(Sphere::new(4.0));
    let mut rendered_edges: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    let road_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.45, 0.45),
        perceptual_roughness: 0.95,
        ..default()
    });
    let junction_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.30, 0.30, 0.30),
        perceptual_roughness: 0.95,
        ..default()
    });
    for node in city.nodes.values() {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(ROAD_WIDTH, road_height, ROAD_WIDTH))),
            MeshMaterial3d(junction_mat.clone()),
            Transform::from_xyz(node.x, road_height * 0.5, node.y),
            Pickable::IGNORE,
        ));
    }
    let shrink = ROAD_WIDTH * 0.5;
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
            let trimmed_len = (len - shrink * 2.0).max(0.1);
            let mid = Vec2::new((ax + bx) * 0.5, (az + bz) * 0.5);
            let angle = diff.y.atan2(diff.x);
            let rot = Quat::from_rotation_y(-angle);
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(trimmed_len, road_height, ROAD_WIDTH))),
                MeshMaterial3d(road_mat.clone()),
                Transform::from_xyz(mid.x, road_height * 0.5, mid.y).with_rotation(rot),
                Pickable::IGNORE,
            ));
            let divider_mat = materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.85, 0.0),
                emissive: LinearRgba::new(0.3, 0.25, 0.0, 1.0),
                unlit: false,
                ..default()
            });
            let dash_len = 40.0;
            let dash_gap = 30.0;
            let dash_w = 1.5;
            let dash_h = road_height + 0.1;
            let dash_period = dash_len + dash_gap;
            let num_dashes = (trimmed_len / dash_period).floor() as i32;
            let total_dashes_len = num_dashes as f32 * dash_period;
            let start_offset = -total_dashes_len * 0.5 + dash_len * 0.5;
            for d in 0..num_dashes {
                let local_x = start_offset + d as f32 * dash_period;
                let offset = rot * Vec3::new(local_x, 0.0, 0.0);
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(dash_len, 1.0, dash_w))),
                    MeshMaterial3d(divider_mat.clone()),
                    Transform::from_xyz(mid.x + offset.x, dash_h, mid.y + offset.z)
                        .with_rotation(rot),
                    Pickable::IGNORE,
                ));
            }
            let edge_mat = materials.add(StandardMaterial {
                base_color: Color::srgb(0.95, 0.95, 0.95),
                emissive: LinearRgba::new(0.15, 0.15, 0.15, 1.0),
                ..default()
            });
            let edge_inset = ROAD_WIDTH * 0.5 - 2.5;
            let edge_y = road_height + 0.1;
            for sign in [-1.0_f32, 1.0] {
                let local_offset = rot * Vec3::new(0.0, 0.0, sign * edge_inset);
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(trimmed_len, 1.0, 2.0))),
                    MeshMaterial3d(edge_mat.clone()),
                    Transform::from_xyz(mid.x + local_offset.x, edge_y, mid.y + local_offset.z)
                        .with_rotation(rot),
                    Pickable::IGNORE,
                ));
            }
        }
    }
    let (mut min_x, mut max_x, mut min_z, mut max_z) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for node in city.nodes.values() {
        min_x = min_x.min(node.x);
        max_x = max_x.max(node.x);
        min_z = min_z.min(node.y);
        max_z = max_z.max(node.y);
    }
    let pad = 200.0;
    let gx = (min_x + max_x) * 0.5;
    let gz = (min_z + max_z) * 0.5;
    let gw = (max_x - min_x) + pad * 2.0;
    let gd = (max_z - min_z) + pad * 2.0;
    let ground_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.18, 0.12),
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(gw, 2.0, gd))),
        MeshMaterial3d(ground_mat.clone()),
        Transform::from_xyz(gx, -1.0, gz),
        Pickable::IGNORE,
    ));
    let wall_h = 40.0;
    let wall_t = 20.0;
    let border_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.25, 0.28),
        perceptual_roughness: 0.7,
        ..default()
    });
    let wall_y = wall_h * 0.5;
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(gw + wall_t * 2.0, wall_h, wall_t))),
        MeshMaterial3d(border_mat.clone()),
        Transform::from_xyz(gx, wall_y, gz - gd * 0.5 - wall_t * 0.5),
        Pickable::IGNORE,
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(gw + wall_t * 2.0, wall_h, wall_t))),
        MeshMaterial3d(border_mat.clone()),
        Transform::from_xyz(gx, wall_y, gz + gd * 0.5 + wall_t * 0.5),
        Pickable::IGNORE,
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(wall_t, wall_h, gd))),
        MeshMaterial3d(border_mat.clone()),
        Transform::from_xyz(gx - gw * 0.5 - wall_t * 0.5, wall_y, gz),
        Pickable::IGNORE,
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(wall_t, wall_h, gd))),
        MeshMaterial3d(border_mat.clone()),
        Transform::from_xyz(gx + gw * 0.5 + wall_t * 0.5, wall_y, gz),
        Pickable::IGNORE,
    ));
    let lot_size = 200.0_f32;
    let lot_h = 3.0_f32;
    let lot_surf_y = lot_h + 0.1;
    let lot_asphalt_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.08, 0.08),
        perceptual_roughness: 0.95,
        ..default()
    });
    let lot_border_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.9, 0.9),
        emissive: LinearRgba::new(0.1, 0.1, 0.1, 1.0),
        ..default()
    });
    let lot_line_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.7, 0.7, 0.7),
        ..default()
    });
    let lot_border_inset = 4.0_f32;
    let lot_border_w = 1.5_f32;
    let lot_border_h = 0.8_f32;
    let spot_w = 22.0_f32;
    let spot_depth = 50.0_f32;
    let spot_line_t = 1.2_f32;
    let spot_line_h = 0.6_f32;
    let inner = lot_size - lot_border_inset * 2.0;
    let num_spots = ((inner / spot_w).floor() as i32).max(1);
    let spots_total_w = num_spots as f32 * spot_w;
    for (portal_index, portal) in city.portals.iter().enumerate() {
        let [cx, cz] = portal.center;
        let (ex, ez) = (portal.node.x, portal.node.y);
        commands
            .spawn((
                Mesh3d(meshes.add(Cuboid::new(lot_size, lot_h, lot_size))),
                MeshMaterial3d(lot_asphalt_mat.clone()),
                Transform::from_xyz(cx, lot_h * 0.5, cz),
                PortalMarker { portal_index },
                Pickable::default(),
            ))
            .observe(on_portal_click);
        let border_edge = lot_size * 0.5 - lot_border_inset;
        for sign in [-1.0_f32, 1.0] {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(
                    lot_size - lot_border_inset * 2.0,
                    lot_border_h,
                    lot_border_w,
                ))),
                MeshMaterial3d(lot_border_mat.clone()),
                Transform::from_xyz(cx, lot_surf_y, cz + sign * border_edge),
                Pickable::IGNORE,
            ));
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(
                    lot_border_w,
                    lot_border_h,
                    lot_size - lot_border_inset * 2.0,
                ))),
                MeshMaterial3d(lot_border_mat.clone()),
                Transform::from_xyz(cx + sign * border_edge, lot_surf_y, cz),
                Pickable::IGNORE,
            ));
        }
        let start_x = cx - spots_total_w * 0.5;
        for row in 0..=num_spots {
            let lx = start_x + row as f32 * spot_w;
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(spot_line_t, spot_line_h, spot_depth))),
                MeshMaterial3d(lot_line_mat.clone()),
                Transform::from_xyz(lx, lot_surf_y, cz),
                Pickable::IGNORE,
            ));
        }
        commands.spawn((
            Mesh3d(exit_mesh.clone()),
            MeshMaterial3d(exit_mat.clone()),
            Transform::from_xyz(ex, 12.0, ez),
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
            shadows_enabled: true,
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
        Skybox {
            image: skybox_image.clone(),
            brightness: 1000.0,
            rotation: Quat::IDENTITY,
        },
        Transform::from_translation(pos).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let ui_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
        ))
        .id();
    commands.spawn((PerfUiDefaultEntries::default(), UiTargetCamera(ui_camera)));
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
            BorderColor {
                top: Color::srgb(0.6, 0.6, 0.7),
                right: Color::srgb(0.6, 0.6, 0.7),
                bottom: Color::srgb(0.6, 0.6, 0.7),
                left: Color::srgb(0.6, 0.6, 0.7),
            },
            BackgroundColor(Color::srgba(0.1, 0.1, 0.15, 0.85)),
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
    commands
        .spawn((
            UiNode {
                position_type: PositionType::Absolute,
                bottom: Val::Px(20.0),
                left: Val::Px(200.0),
                width: Val::Px(160.0),
                height: Val::Px(48.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            Button,
            ToggleFlyCamButton,
            BorderColor {
                top: Color::srgb(0.6, 0.6, 0.7),
                right: Color::srgb(0.6, 0.6, 0.7),
                bottom: Color::srgb(0.6, 0.6, 0.7),
                left: Color::srgb(0.6, 0.6, 0.7),
            },
            BackgroundColor(Color::srgba(0.1, 0.1, 0.15, 0.85)),
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("Orbit Cam"),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::WHITE),
                ToggleFlyCamButton,
            ));
        });
}

// Waits for the skybox image to finish loading then sets its texture view dimension to Cube so the GPU binds it correctly; runs every frame but exits immediately once the descriptor is set
// Input: car_assets: Res<CarAssets> holding the skybox handle; images: ResMut<Assets<Image>> for mutating the loaded image
// Returns: none
fn fix_skybox_view(car_assets: Res<CarAssets>, mut images: ResMut<Assets<Image>>) {
    if let Some(image) = images.get_mut(&car_assets.skybox) {
        if image
            .texture_view_descriptor
            .as_ref()
            .map(|d| d.dimension == Some(TextureViewDimension::Cube))
            .unwrap_or(false)
        {
            return;
        }
        image.texture_view_descriptor = Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::Cube),
            ..default()
        });
    }
}

// Marker component added to a car entity once its path segment meshes have been spawned, preventing the segments from being created more than once
#[derive(Component)]
struct PathRendered;

// Component attached to each path debug segment mesh identifying which car and which waypoint interval it visualizes
// car_license: license plate of the car this segment belongs to, used to match against CarLicense on the car entity
// seg_index: the index i of the waypoint pair (i, i+1) this segment represents, used to despawn it once the car passes waypoint i+1
#[derive(Component)]
struct PathSegment {
    car_license: String,
    seg_index: usize,
}

// Rotates each active roadway car each frame to face the direction it is currently moving, reading dir_x and dir_z from CarPhysics and delegating to car_facing_quat for the angle calculation
// Input: q: Query over entities with Transform and CarPhysics but without PreRoad, covering all cars currently on the roadway
// Returns: none
fn update_car_rotation(
    mut q: Query<(&mut Transform, &CarPhysics), (Without<PreRoad>, Without<ParkingIn>)>,
) {
    for (mut transform, physics) in q.iter_mut() {
        if physics.speed < 0.1 {
            continue;
        }
        transform.rotation = car_facing_quat(physics.dir_x, physics.dir_z);
    }
}

// Spawns colored road segment meshes along each active car's assigned waypoint path, then marks the car entity so segments are not spawned again next frame
// Input: commands: Commands for spawning segment entities and inserting PathRendered; meshes, materials: asset resources for building segment geometry; q: Query over cars that are on the roadway, have a color, and have not yet had their path rendered
// Returns: none
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
        let seg_height = 2.0;
        let seg_thickness = 5.0;
        let y_offset = 11.0;
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
                PathSegment {
                    car_license: car_license.0.clone(),
                    seg_index: i,
                },
                Pickable::IGNORE,
            ));
        }
        commands.entity(entity).insert(PathRendered);
    }
}

// Despawns each path debug segment whose waypoint interval the car has already passed, checking wp_index against seg_index each frame so segments are removed as the car drives through them rather than all at once at route completion
// Input: commands: Commands for despawning segment entities; cars: Query over active roadway cars providing their license and CarPhysics for wp_index lookup; segments: Query over PathSegment entities to check and despawn
// Returns: none
fn despawn_passed_segments(
    mut commands: Commands,
    cars: Query<(&CarLicense, &CarPhysics), Without<PreRoad>>,
    segments: Query<(Entity, &PathSegment)>,
) {
    for (entity, seg) in segments.iter() {
        for (car_license, physics) in cars.iter() {
            if car_license.0 != seg.car_license {
                continue;
            }
            let wp_index = physics.http.lock().unwrap().wp_index;
            if wp_index > seg.seg_index + 1 {
                commands.entity(entity).despawn();
            }
            break;
        }
    }
}

// Handles presses of the fly cam toggle button, switching CameraMode between Orbit and Fly and updating the button label; when switching to Fly the camera moves to the last fly position and rotation; when switching to Orbit the orbit state is reconstructed from the current camera position so it resumes from where fly left off
// Input: mode: ResMut<CameraMode> the current camera mode toggled on press; orbit: ResMut<Orbit> updated from current camera position when returning to Orbit; fly_state: ResMut<FlyCamState> updated from current camera transform when entering Fly; interaction_q: Query detecting button presses on ToggleFlyCamButton; label_q: Query over Text entities with ToggleFlyCamButton used to update the button label; cam_q: Query<&mut Transform, With<Camera3d>> to reposition the camera on mode switch
// Returns: none
fn toggle_flycam_system(
    mut mode: ResMut<CameraMode>,
    mut orbit: ResMut<Orbit>,
    mut fly_state: ResMut<FlyCamState>,
    interaction_q: Query<&Interaction, (Changed<Interaction>, With<ToggleFlyCamButton>)>,
    mut label_q: Query<&mut Text, With<ToggleFlyCamButton>>,
    mut cam_q: Query<&mut Transform, With<Camera3d>>,
) {
    let clicked = interaction_q.iter().any(|i| *i == Interaction::Pressed);
    if !clicked {
        return;
    }
    let Ok(mut t) = cam_q.single_mut() else {
        return;
    };
    match *mode {
        CameraMode::Orbit => {
            fly_state.position = t.translation;
            let (yaw, pitch, _) = t.rotation.to_euler(EulerRot::YXZ);
            fly_state.yaw = yaw;
            fly_state.pitch = pitch;
            *mode = CameraMode::Fly;
        }
        CameraMode::Fly => {
            let pos = t.translation;
            let flat = Vec3::new(pos.x, 0.0, pos.z);
            let radius = pos.length().max(150.0);
            orbit.radius = radius;
            orbit.focus = Vec3::ZERO;
            orbit.yaw = flat.x.atan2(flat.z);
            orbit.pitch = (pos.y / radius).asin().clamp(0.05, PI / 2.0 - 0.05);
            *t = Transform::from_translation(orbit_pos(&orbit)).looking_at(orbit.focus, Vec3::Y);
            *mode = CameraMode::Orbit;
        }
    }
    for mut text in label_q.iter_mut() {
        text.0 = match *mode {
            CameraMode::Orbit => "Orbit Cam".to_string(),
            CameraMode::Fly => "Fly Cam".to_string(),
        };
    }
    if *mode == CameraMode::Fly {
        t.translation = fly_state.position;
        t.rotation = Quat::from_euler(EulerRot::YXZ, fly_state.yaw, fly_state.pitch, 0.0);
    }
}

// Responds to presses of the batch spawn button by picking up to BATCH_SPAWN_COUNT random unique portal pairs, skipping any routes already in use, and spawning a car for each valid pair
// Input: commands: Commands for spawning car entities; interaction_q: Query for detecting button press interactions; car_assets: Res<CarAssets> for the shared car scene handle; city: NonSend<CityData> for portal data; selection: ResMut<PortalSelection> for car ID and port counters; existing_cars: Query<&PreRoad> to avoid duplicate routes
// Returns: none
fn spawn_batch_button_system(
    mut commands: Commands,
    interaction_q: Query<&Interaction, (Changed<Interaction>, With<SpawnBatchButton>)>,
    car_assets: Res<CarAssets>,
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
        let travel_dir = if to_exit.length() > 1e-6 {
            to_exit.normalize()
        } else {
            Vec2::X
        };
        let lane_offset = right_lane_offset(travel_dir);
        let spawn_xz = Vec2::new(sx, sz) + lane_offset;
        let wait_xz_offset = wait_xz + lane_offset;
        let road_entry_xz = exit_xz + lane_offset;
        let i = selection.next_car_id;
        selection.next_car_id += 1;
        let port = selection.next_port;
        selection.next_port += 1;
        let license = format!("CAR-{:03}", i);
        let car_url = format!("http://{}:{}", local_ip(), port);
        let register_url = format!("{}{}", SERVER_URL, REGISTER_CAR_ENDPOINT);
        let validate_url = format!("{}{}", SERVER_URL, VALIDATE_ENTRY_ENDPOINT);
        let car_color = rand_car_color();
        println!(
            "Batch spawning {} port {} : {} -> {}",
            license, port, src_idx, dst_idx
        );
        let dst_center = portals[dst_idx].center;
        let http_state = Arc::new(Mutex::new(CarHttp::new(spawn_xz.x, spawn_xz.y)));
        spawn_car_listener(port, Arc::clone(&http_state));
        commands
            .spawn((
                Transform::from_xyz(spawn_xz.x, 13.5, spawn_xz.y),
                Visibility::Inherited,
                CarColor(car_color),
                CarLicense(license.clone()),
                PreRoad {
                    phase: PreRoadPhase::DrivingToWait,
                    wait_target: Vec3::new(wait_xz_offset.x, 13.5, wait_xz_offset.y),
                    road_entry: Vec3::new(road_entry_xz.x, 13.5, road_entry_xz.y),
                    license,
                    car_url,
                    register_url,
                    validate_url,
                    src_node_id: src_id,
                    dst_node_id: dst_id,
                    polling_in_flight: false,
                },
                PostRoad {
                    center: Vec3::new(dst_center[0], 13.5, dst_center[1]),
                },
                CarPhysics {
                    http: http_state,
                    speed: 0.0,
                    dir_x: 1.0,
                    dir_z: 0.0,
                },
            ))
            .with_children(|parent| {
                parent.spawn((
                    SceneRoot(car_assets.scene.clone()),
                    Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::splat(CAR_SCALE)),
                ));
                parent.spawn((Transform::IDENTITY, Pickable::IGNORE));
            });
        spawned += 1;
    }
    println!(
        "Batch spawn: {} cars spawned ({} attempts)",
        spawned, attempts
    );
}
