/*
prologue
Name of program: car_emulator.rs
Description: Systems for emulating car. Updating physics of each car. Connecting to the main server.
Author: Maren Proplesch
Date Created: 3/13/2026
Date Revised: 3/29/2026
Revision History: Added spawn queue and car-following collision avoidance.
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
Citation: Used AI copilot for limited code generation - claude.ai
*/

use bevy::prelude::*;
use std::{
    collections::{HashMap, VecDeque},
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread::spawn,
    time::Duration,
};
use ureq::post;

use crate::{
    ACCELERATION, AMBULANCE_SCALE, EXIT_DRIVE_SPEED, ROAD_WIDTH,
    map_parser::{CityData, Waypoint, parse_waypoints},
    parse_form,
};

const SPAWN_CLEAR_RADIUS: f32 = 30.0;
const FOLLOW_DISTANCE: f32 = 120.0;
const FOLLOW_MIN_FRACTION: f32 = 0.15;

// Holds the complete set of arguments needed to spawn a single car entity, queued when the user requests a spawn and released one at a time with spacing
// portal_index: index of the source portal this car belongs to, used to route it into the correct per-portal sub-queue
// is_ambulance: when true the car uses ambulance.glb instead of the default model, assigned with a 1-in-10 probability at enqueue time
pub struct QueuedCar {
    pub portal_index: usize,
    pub spawn_xz: Vec2,
    pub wait_xz_offset: Vec2,
    pub road_entry_xz: Vec2,
    pub license: String,
    pub car_url: String,
    pub register_url: String,
    pub validate_url: String,
    pub src_node_id: String,
    pub dst_node_id: String,
    pub dst_center: [f32; 2],
    pub car_color: Color,
    pub port: u16,
    pub is_ambulance: bool,
}

// Shared resource holding one independent VecDeque per source portal so cars queued at different
// portals never block each other; the key is the portal index into CityData::portals
#[derive(Resource, Default)]
pub struct CarSpawnQueue {
    pub queues: HashMap<usize, VecDeque<QueuedCar>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PreRoadPhase {
    DrivingToWait,
    WaitingForEntry,
}

#[derive(Component)]
pub struct PreRoad {
    pub phase: PreRoadPhase,
    pub wait_target: Vec3,
    pub road_entry: Vec3,
    pub license: String,
    pub car_url: String,
    pub register_url: String,
    pub validate_url: String,
    pub src_node_id: String,
    pub dst_node_id: String,
    pub polling_in_flight: bool,
}

#[derive(Component)]
pub struct PostRoad {
    pub center: Vec3,
}

pub struct CarHttp {
    pub pos_x: f32,
    pub pos_z: f32,
    pub waypoints: Vec<Waypoint>,
    pub wp_index: usize,
    pub speed: f32,
    pub stopped: bool,
    pub pending_response: Option<String>,
}

impl CarHttp {
    pub fn new(x: f32, z: f32) -> Self {
        CarHttp {
            pos_x: x,
            pos_z: z,
            waypoints: Vec::new(),
            wp_index: 0,
            speed: 0.0,
            stopped: true,
            pending_response: None,
        }
    }
}

#[derive(Component)]
pub struct CarPhysics {
    pub http: Arc<Mutex<CarHttp>>,
    pub speed: f32,
    pub dir_x: f32,
    pub dir_z: f32,
}

#[derive(Component, Clone)]
pub struct CarLicense(pub String);

#[derive(Component)]
pub struct ParkingIn;

// Displaces waypoints into the right lane using per-segment perpendiculars averaged at corners; for
// a forward direction (dx, dz) the right lane in Bevy's Y-up right-handed XZ plane is the
// left-hand perpendicular (-dz, dx)/mag — a car moving in +X has its right lane in -Z, and
// (-dz/mag, dx/mag) gives (0, -1) for (dx=1,dz=0), which is the -Z offset that places it in the
// right lane when viewed from above with +X pointing right and +Z pointing toward the viewer
// Input: waypoints: Vec<Waypoint> centerline waypoints parsed from the server response
// Returns: Vec<Waypoint> with x and z fields shifted by ROAD_WIDTH * 0.25 into the right lane
fn offset_waypoints_to_right_lane(waypoints: Vec<Waypoint>) -> Vec<Waypoint> {
    let n = waypoints.len();
    if n < 2 {
        return waypoints;
    }
    let seg_right: Vec<Vec2> = (0..n - 1)
        .map(|i| {
            let dx = waypoints[i + 1].x - waypoints[i].x;
            let dz = waypoints[i + 1].z - waypoints[i].z;
            let mag = dx.hypot(dz);
            if mag < 1e-6 {
                return Vec2::ZERO;
            }
            Vec2::new(-dz / mag, dx / mag)
        })
        .collect();
    let shift = ROAD_WIDTH * 0.25;
    let mut result = Vec::with_capacity(n);
    for i in 0..n {
        let right = if i == 0 {
            seg_right[0]
        } else if i == n - 1 {
            seg_right[n - 2]
        } else {
            let avg = seg_right[i - 1] + seg_right[i];
            let mag = avg.length();
            if mag > 1e-6 { avg / mag } else { seg_right[i] }
        };
        result.push(Waypoint {
            node_id: waypoints[i].node_id.clone(),
            x: waypoints[i].x + right.x * shift,
            z: waypoints[i].z + right.y * shift,
        });
    }
    result
}

// Computes a following-adjusted speed by scanning the pre-built slice of other car positions and
// directions, skipping oncoming traffic (dot product below 0.5) and cars that are behind, then
// blending between closest_speed and target_speed proportionally to the gap fraction so cars
// smoothly decelerate as they close in rather than snapping to a fixed speed
// Input: pos_x, pos_z: f32 current position; dir_x, dir_z: f32 normalized movement direction; target_speed: f32 server-assigned speed; others: &[(f32, f32, f32, f32)] positions and directions of all other active cars
// Returns: f32 the gap-blended speed, clamped to at least FOLLOW_MIN_FRACTION of target_speed
fn following_speed(
    pos_x: f32,
    pos_z: f32,
    dir_x: f32,
    dir_z: f32,
    target_speed: f32,
    others: &[(f32, f32, f32, f32)],
) -> f32 {
    let mut closest_dist = f32::MAX;
    let mut closest_speed = target_speed;
    for &(ox, oz, odir_x, odir_z) in others {
        let same_dir = odir_x * dir_x + odir_z * dir_z;
        if same_dir < 0.5 {
            continue;
        }
        let dx = ox - pos_x;
        let dz = oz - pos_z;
        let dot = dx * dir_x + dz * dir_z;
        if dot <= 0.0 {
            continue;
        }
        let dist = (dx * dx + dz * dz).sqrt();
        if dist < FOLLOW_DISTANCE && dist < closest_dist {
            closest_dist = dist;
            closest_speed = target_speed * same_dir;
        }
    }
    if closest_dist == f32::MAX {
        return target_speed;
    }
    let gap_fraction = (closest_dist / FOLLOW_DISTANCE).clamp(0.0, 1.0);
    let blended = closest_speed + (target_speed - closest_speed) * gap_fraction;
    blended.max(target_speed * FOLLOW_MIN_FRACTION)
}

// Advances the physics simulation for all active roadway cars each frame; reads target speed,
// stopped flag, wp_index, and wp_count from CarHttp in a single lock acquisition per car to
// minimize mutex contention, then writes pos_x/pos_z back in a second acquisition only after
// position is updated; applies car-following speed adjustment against a snapshot of all car
// positions built once before the per-car loop to avoid re-querying the query inside the loop
// Input: commands: Commands for despawning entities; time: Res<Time> for the frame delta; q: Query over car entities on the roadway that are not yet parking; path_segs: Query over path segment entities for cleanup on despawn
// Returns: none
pub fn update_car_physics(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &mut CarPhysics,
            &CarLicense,
            Option<&PostRoad>,
        ),
        (Without<PreRoad>, Without<ParkingIn>),
    >,
    path_segs: Query<(Entity, &CarLicense), (Without<CarPhysics>, Without<PreRoad>)>,
) {
    const PROXIMITY: f32 = 4.0;
    let dt = time.delta_secs();
    let others: Vec<(f32, f32, f32, f32)> = q
        .iter()
        .map(|(_, t, physics, _, _)| {
            (
                t.translation.x,
                t.translation.z,
                physics.dir_x,
                physics.dir_z,
            )
        })
        .collect();
    for (car_entity, mut transform, mut physics, car_license, post_road) in q.iter_mut() {
        let (target_speed, stopped, wp_index, wp_count) = {
            let h = physics.http.lock().unwrap();
            (h.speed, h.stopped, h.wp_index, h.waypoints.len())
        };
        if stopped || wp_count == 0 {
            physics.speed -= (ACCELERATION * dt).min(physics.speed);
            if physics.speed > 0.0 {
                transform.translation.x += physics.dir_x * physics.speed * dt;
                transform.translation.z += physics.dir_z * physics.speed * dt;
                let mut h = physics.http.lock().unwrap();
                h.pos_x = transform.translation.x;
                h.pos_z = transform.translation.z;
            } else if wp_count > 0 {
                if post_road.is_some() {
                    commands.entity(car_entity).insert(ParkingIn);
                } else {
                    commands.entity(car_entity).despawn();
                    for (seg_entity, seg_license) in path_segs.iter() {
                        if seg_license.0 == car_license.0 {
                            commands.entity(seg_entity).despawn();
                        }
                    }
                }
            }
            continue;
        }
        let self_x = transform.translation.x;
        let self_z = transform.translation.z;
        let others_excluding_self: Vec<(f32, f32, f32, f32)> = others
            .iter()
            .copied()
            .filter(|&o| {
                let dx = o.0 - self_x;
                let dz = o.1 - self_z;
                dx * dx + dz * dz > 1.0
            })
            .collect();
        let adjusted_speed = following_speed(
            self_x,
            self_z,
            physics.dir_x,
            physics.dir_z,
            target_speed,
            &others_excluding_self,
        );
        let delta = adjusted_speed - physics.speed;
        physics.speed += delta.clamp(-ACCELERATION * dt, ACCELERATION * dt);
        let (wp_x, wp_z) = {
            let h = physics.http.lock().unwrap();
            if wp_index >= h.waypoints.len() {
                drop(h);
                physics.http.lock().unwrap().stopped = true;
                continue;
            }
            let wp = &h.waypoints[wp_index];
            (wp.x, wp.z)
        };
        let to_wp = Vec2::new(wp_x - self_x, wp_z - self_z);
        let dist_to_wp = to_wp.length();
        if dist_to_wp > 1.0 {
            physics.dir_x = to_wp.x / dist_to_wp;
            physics.dir_z = to_wp.y / dist_to_wp;
        }
        transform.translation.x += physics.dir_x * physics.speed * dt;
        transform.translation.z += physics.dir_z * physics.speed * dt;
        if dist_to_wp < PROXIMITY {
            let mut h = physics.http.lock().unwrap();
            h.wp_index += 1;
            if h.wp_index >= h.waypoints.len() {
                h.stopped = true;
            }
        }
        let mut h = physics.http.lock().unwrap();
        h.pos_x = transform.translation.x;
        h.pos_z = transform.translation.z;
    }
}

// Drives cars that have finished their route into their destination parking lot, steering toward the lot center and despawning once they arrive
// Input: commands: Commands for despawning entities; time: Res<Time> for the frame delta; q: Query over car entities with ParkingIn and PostRoad; path_segs: Query over path segment entities for cleanup on despawn
// Returns: none
pub fn parking_in_system(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &mut CarPhysics,
            &CarLicense,
            &PostRoad,
        ),
        With<ParkingIn>,
    >,
    path_segs: Query<(Entity, &CarLicense), (Without<CarPhysics>, Without<PreRoad>)>,
) {
    const PARK_SPEED: f32 = 40.0;
    const PARK_PROXIMITY: f32 = 6.0;
    let dt = time.delta_secs();
    for (car_entity, mut transform, mut physics, car_license, post_road) in q.iter_mut() {
        let diff = Vec2::new(
            post_road.center.x - transform.translation.x,
            post_road.center.z - transform.translation.z,
        );
        let dist = diff.length();
        if dist < PARK_PROXIMITY {
            commands.entity(car_entity).despawn();
            for (seg_entity, seg_license) in path_segs.iter() {
                if seg_license.0 == car_license.0 {
                    commands.entity(seg_entity).despawn();
                }
            }
            continue;
        }
        let dir = diff.normalize();
        physics.dir_x = dir.x;
        physics.dir_z = dir.y;
        transform.rotation = car_facing_quat(dir.x, dir.y);
        let delta = PARK_SPEED - physics.speed;
        physics.speed += delta.clamp(-ACCELERATION * dt, ACCELERATION * dt);
        transform.translation.x += physics.dir_x * physics.speed * dt;
        transform.translation.z += physics.dir_z * physics.speed * dt;
        let mut h = physics.http.lock().unwrap();
        h.pos_x = transform.translation.x;
        h.pos_z = transform.translation.z;
    }
}

// Computes the Y-axis rotation quaternion for a movement direction vector, applying the same offset used by update_car_rotation so pre-road and on-road cars rotate identically; exported as pub so main.rs can call it from update_car_rotation without duplicating the angle formula
// Input: dir_x: f32 normalized X component of the movement direction; dir_z: f32 normalized Z component of the movement direction
// Returns: Quat representing the correct Y-axis rotation for a car moving in the given direction
pub fn car_facing_quat(dir_x: f32, dir_z: f32) -> Quat {
    let angle = dir_z.atan2(dir_x);
    Quat::from_rotation_y(-angle + std::f32::consts::FRAC_PI_2)
}

fn http_reply(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        body.len(),
        body
    )
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<CarHttp>>) {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    if request_line.trim().is_empty() {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_uppercase();
    let path = parts.next().unwrap_or("").to_string();
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        if line.trim().is_empty() {
            break;
        }
        let lower = line.to_lowercase();
        if lower.starts_with("content-length:") {
            content_length = lower
                .split(':')
                .nth(1)
                .unwrap_or("0")
                .trim()
                .parse()
                .unwrap_or(0);
        }
    }
    let _body = if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        let mut total = 0;
        while total < content_length {
            match std::io::Read::read(&mut reader, &mut buf[total..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => total += n,
            }
        }
        String::from_utf8_lossy(&buf[..total]).to_string()
    } else {
        String::new()
    };
    let response = match (method.as_str(), path.split('?').next().unwrap_or("")) {
        ("GET", "/position") => {
            let s = state.lock().unwrap();
            http_reply("200 OK", &format!("x={:.6}&y={:.6}", s.pos_x, s.pos_z))
        }
        _ => http_reply("404 Not Found", "not found"),
    };
    let _ = stream.write_all(response.as_bytes());
}

// Starts a background TCP listener on the given port that handles incoming HTTP position requests for a single car by dispatching each connection to its own thread
// Input: port: u16 the local port to bind the listener to; state: Arc<Mutex<CarHttp>> shared car state passed to each connection handler
// Returns: none
pub fn spawn_car_listener(port: u16, state: Arc<Mutex<CarHttp>>) {
    spawn(move || {
        let listener = match TcpListener::bind(format!("127.0.0.1:{}", port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Car listener failed on port {}: {}", port, e);
                return;
            }
        };
        for stream in listener.incoming().flatten() {
            let s = Arc::clone(&state);
            spawn(move || handle_connection(stream, s));
        }
    });
}

// Iterates every per-portal sub-queue independently each frame; for each portal checks whether any
// existing pre-road car is within SPAWN_CLEAR_RADIUS of that portal's spawn point and skips it if
// blocked, otherwise pops and spawns the front entry; portals with empty queues are skipped
// immediately so the cost scales with the number of portals that actually have pending cars
// Input: commands: Commands for spawning car entities; car_assets: Res<CarAssets> for the shared scene handles; queue: ResMut<CarSpawnQueue> holding per-portal pending spawns; pre_road_q: Query<&Transform> over all pre-road cars to check for occupancy at each spawn point
// Returns: none
pub fn car_spawn_queue_system(
    mut commands: Commands,
    car_assets: Res<crate::CarAssets>,
    mut queue: ResMut<CarSpawnQueue>,
    pre_road_q: Query<&Transform, With<PreRoad>>,
) {
    for sub_queue in queue.queues.values_mut() {
        let Some(queued) = sub_queue.front() else {
            continue;
        };
        let spawn_pos = queued.spawn_xz;
        let blocked = pre_road_q.iter().any(|t| {
            let dx = t.translation.x - spawn_pos.x;
            let dz = t.translation.z - spawn_pos.y;
            dx * dx + dz * dz < SPAWN_CLEAR_RADIUS * SPAWN_CLEAR_RADIUS
        });
        if blocked {
            continue;
        }
        let queued = sub_queue.pop_front().unwrap();
        let spawn_pos = queued.spawn_xz;
        let http_state = Arc::new(Mutex::new(CarHttp::new(spawn_pos.x, spawn_pos.y)));
        spawn_car_listener(queued.port, Arc::clone(&http_state));
        let model_scene = if queued.is_ambulance {
            car_assets.ambulance_scene.clone()
        } else {
            car_assets.scene.clone()
        };
        commands
            .spawn((
                Transform::from_xyz(spawn_pos.x, 13.5, spawn_pos.y),
                Visibility::Inherited,
                crate::CarColor(queued.car_color),
                CarLicense(queued.license.clone()),
                PreRoad {
                    phase: PreRoadPhase::DrivingToWait,
                    wait_target: Vec3::new(queued.wait_xz_offset.x, 13.5, queued.wait_xz_offset.y),
                    road_entry: Vec3::new(queued.road_entry_xz.x, 13.5, queued.road_entry_xz.y),
                    license: queued.license.clone(),
                    car_url: queued.car_url,
                    register_url: queued.register_url,
                    validate_url: queued.validate_url,
                    src_node_id: queued.src_node_id,
                    dst_node_id: queued.dst_node_id,
                    polling_in_flight: false,
                },
                PostRoad {
                    center: Vec3::new(queued.dst_center[0], 13.5, queued.dst_center[1]),
                },
                CarPhysics {
                    http: http_state,
                    speed: 0.0,
                    dir_x: 1.0,
                    dir_z: 0.0,
                },
            ))
            .with_children(|parent| {
                let scale = if queued.is_ambulance {
                    AMBULANCE_SCALE
                } else {
                    crate::CAR_SCALE
                };
                parent.spawn((
                    SceneRoot(model_scene),
                    Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::splat(scale)),
                ));
                parent.spawn((
                    Transform::IDENTITY,
                    bevy::picking::prelude::Pickable::IGNORE,
                ));
            });
        println!(
            "Queue released {} (ambulance: {})",
            queued.license, queued.is_ambulance
        );
    }
}

// Drives the pre-road state machine for all cars not yet on the roadway, handling both driving to the wait position and polling the server for entry approval, and removing the PreRoad component once entry is granted; rotates the car model to face its current movement direction during DrivingToWait using the same angle formula as update_car_rotation
// Input: commands: Commands for removing the PreRoad component; time: Res<Time> for the frame delta; city: NonSend<CityData> for resolving waypoint coordinates; q: Query over entities with Transform, CarPhysics, and PreRoad
// Returns: none
pub fn pre_road_system(
    mut commands: Commands,
    time: Res<Time>,
    city: NonSend<CityData>,
    mut q: Query<(Entity, &mut Transform, &mut CarPhysics, &mut PreRoad)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut physics, mut pre) in q.iter_mut() {
        match pre.phase.clone() {
            PreRoadPhase::DrivingToWait => {
                let target = pre.wait_target;
                let diff = Vec2::new(
                    target.x - transform.translation.x,
                    target.z - transform.translation.z,
                );
                let dist = diff.length();
                if dist < 0.5 {
                    transform.translation.x = target.x;
                    transform.translation.z = target.z;
                    physics.speed = 0.0;
                    {
                        let mut h = physics.http.lock().unwrap();
                        h.pos_x = transform.translation.x;
                        h.pos_z = transform.translation.z;
                        h.stopped = true;
                    }
                    let license = pre.license.clone();
                    let car_url = pre.car_url.clone();
                    let register_url = pre.register_url.clone();
                    let src_node_id = pre.src_node_id.clone();
                    let dst_node_id = pre.dst_node_id.clone();
                    spawn(move || {
                        let body = format!(
                            "license={}&url={}&start_id={}&dest_id={}",
                            license, car_url, src_node_id, dst_node_id
                        );
                        match post(&register_url)
                            .header("Content-Type", "application/x-www-form-urlencoded")
                            .send(&body)
                        {
                            Ok(resp) => println!("registered {}: {}", license, resp.status()),
                            Err(ureq::Error::StatusCode(c)) => {
                                eprintln!("{} register failed: http {}", license, c)
                            }
                            Err(e) => eprintln!("{} register failed: {}", license, e),
                        }
                    });
                    pre.phase = PreRoadPhase::WaitingForEntry;
                } else {
                    let delta = EXIT_DRIVE_SPEED - physics.speed;
                    physics.speed += delta.clamp(-ACCELERATION * dt, ACCELERATION * dt);
                    let dir = diff.normalize();
                    transform.translation.x += dir.x * physics.speed * dt;
                    transform.translation.z += dir.y * physics.speed * dt;
                    transform.rotation = car_facing_quat(dir.x, dir.y);
                    let mut h = physics.http.lock().unwrap();
                    h.pos_x = transform.translation.x;
                    h.pos_z = transform.translation.z;
                }
            }
            PreRoadPhase::WaitingForEntry => {
                if physics.speed > 0.1 {
                    continue;
                }
                let granted = {
                    let h = physics.http.lock().unwrap();
                    !h.stopped && !h.waypoints.is_empty()
                };
                if granted {
                    transform.translation = pre.road_entry;
                    physics.speed = 0.0;
                    {
                        let (init_dir_x, init_dir_z, init_speed) = {
                            let h = physics.http.lock().unwrap();
                            let (dx, dz) = if h.waypoints.len() > 1 {
                                let wp = &h.waypoints[1];
                                let ddx = wp.x - transform.translation.x;
                                let ddz = wp.z - transform.translation.z;
                                let mag = ddx.hypot(ddz);
                                if mag > 1e-6 {
                                    (ddx / mag, ddz / mag)
                                } else {
                                    (1.0, 0.0)
                                }
                            } else {
                                (1.0, 0.0)
                            };
                            (dx, dz, h.speed)
                        };
                        physics.dir_x = init_dir_x;
                        physics.dir_z = init_dir_z;
                        physics.speed = init_speed;
                        let mut h = physics.http.lock().unwrap();
                        h.pos_x = transform.translation.x;
                        h.pos_z = transform.translation.z;
                    }
                    commands.entity(entity).remove::<PreRoad>();
                    continue;
                }
                {
                    let pending = {
                        let mut h = physics.http.lock().unwrap();
                        h.pending_response.take()
                    };
                    if let Some(text) = pending {
                        if text.contains("allowed=true") {
                            let params = parse_form(&text);
                            let waypoints = parse_waypoints(&params, &city);
                            let speed: f32 = params
                                .get("speed")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(40.0);
                            let waypoints = offset_waypoints_to_right_lane(waypoints);
                            let mut h = physics.http.lock().unwrap();
                            h.waypoints = waypoints;
                            h.wp_index = 1;
                            h.speed = speed;
                            h.stopped = false;
                        } else {
                            pre.polling_in_flight = false;
                            println!("{}: entry denied, will retry", pre.license);
                        }
                        continue;
                    }
                }
                if pre.polling_in_flight {
                    continue;
                }
                pre.polling_in_flight = true;
                let license = pre.license.clone();
                let validate_url = pre.validate_url.clone();
                let http = Arc::clone(&physics.http);
                spawn(move || {
                    let body = format!("license={}", license);
                    match post(&validate_url)
                        .header("Content-Type", "application/x-www-form-urlencoded")
                        .send(&body)
                    {
                        Ok(mut resp) => {
                            let text = resp.body_mut().read_to_string().unwrap_or_default();
                            let mut h = http.lock().unwrap();
                            h.pending_response = Some(text);
                        }
                        Err(e) => {
                            eprintln!("{}: validate_entry error: {}", license, e);
                            let mut h = http.lock().unwrap();
                            h.pending_response = Some(String::new());
                        }
                    }
                });
            }
        }
    }
}
