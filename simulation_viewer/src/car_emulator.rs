/*
prologue
Name of program: car_emulator.rs
Description: Systems for emulating car. Updating physics of each car. Connecting to the main server.
Author: Maren Proplesch
Date Created: 3/13/2026
Date Revised: 3/13/2026
Revision History: None
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
*/

use bevy::prelude::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread::spawn,
    time::Duration,
};
use ureq::post;

use crate::{
    ACCELERATION, EXIT_DRIVE_SPEED,
    map_parser::{CityData, Waypoint, parse_waypoints},
    parse_form,
};

// Tracks which stage of the pre-road entry sequence a car is currently in
#[derive(Debug, Clone, PartialEq)]
pub enum PreRoadPhase {
    DrivingToWait, // car is actively driving toward its designated waiting position near the road entry point
    WaitingForEntry, // car has stopped at the wait position and is polling the server for roadway entry approval
}

// ECS component attached to a car entity while it is still in the pre-road phase, holding all the state needed to register with the server and transition onto the roadway
// phase: which step of the pre-road sequence the car is currently executing
// wait_target: world position the car should drive to before attempting to register
// road_entry: world position the car will be teleported to once entry is granted
// license: unique license plate string used to identify this car with the server
// car_url: the HTTP callback URL the server can use to query this car's position
// register_url: the server endpoint used to register the car and request a path
// validate_url: the server endpoint polled to check whether entry has been approved
// src_node_id: string ID of the graph node where the car's route begins
// dst_node_id: string ID of the graph node where the car's route ends
// polling_in_flight: flag indicating that a validate_entry HTTP request is currently in progress, preventing duplicate polls
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

// Shared mutable state for a car's HTTP listener thread, including its current position, assigned waypoints, and movement parameters
// pos_x: current world X position of the car, updated each physics frame and readable by the HTTP listener
// pos_z: current world Z position of the car, updated each physics frame and readable by the HTTP listener
// waypoints: ordered list of waypoints assigned by the server that the car will follow
// wp_index: index into the waypoints list pointing to the car's next target waypoint
// speed: target speed in world units per second that the physics system will accelerate toward
// stopped: flag indicating that the car has reached its destination or has not yet been granted entry
// pending_response: buffer holding the most recent raw HTTP response text from the server, consumed by the main thread each frame
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
    // Constructs a new CarHttp instance at the given world position with all movement fields set to their stopped defaults
    // Input: x: f32 initial world X position; z: f32 initial world Z position
    // Returns: CarHttp with empty waypoints, zero speed, and stopped set to true
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

// ECS component holding the physics state and shared HTTP data for a car that is actively on the roadway
// http: thread-safe shared reference to the CarHttp state, accessed by both the physics system and the HTTP listener thread
// speed: the car's current actual speed in world units per second, smoothly adjusted toward the target speed each frame
// dir_x: the X component of the car's current normalized movement direction
// dir_z: the Z component of the car's current normalized movement direction
#[derive(Component)]
pub struct CarPhysics {
    pub http: Arc<Mutex<CarHttp>>,
    pub speed: f32,
    pub dir_x: f32,
    pub dir_z: f32,
}

// ECS component that stores a car entity's license plate string, used to associate path segment entities with their owning car
// 0: the license plate string identifying which car this component belongs to
#[derive(Component, Clone)]
pub struct CarLicense(pub String);

// Advances the physics simulation for all active roadway cars each frame, steering each car toward its next waypoint, applying acceleration, and despawning the car and its path segments when the route is complete
// Input: commands: Commands for despawning entities; time: Res<Time> for the frame delta; q: Query over car entities with Transform, CarPhysics, and CarLicense but without PreRoad; path_segs: Query over path segment entities with CarLicense but without CarPhysics or PreRoad
// Returns: none
pub fn update_car_physics(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut CarPhysics, &CarLicense), Without<PreRoad>>,
    path_segs: Query<(Entity, &CarLicense), (Without<CarPhysics>, Without<PreRoad>)>,
) {
    const PROXIMITY: f32 = 4.0;
    let dt = time.delta_secs();
    for (car_entity, mut transform, mut physics, car_license) in q.iter_mut() {
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
                println!("{}: reached destination, despawning", car_license.0);
                commands.entity(car_entity).despawn();
                for (seg_entity, seg_license) in path_segs.iter() {
                    if seg_license.0 == car_license.0 {
                        commands.entity(seg_entity).despawn();
                    }
                }
            }
            continue;
        }
        let delta = target_speed - physics.speed;
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
        let to_wp = Vec2::new(
            wp_x - transform.translation.x,
            wp_z - transform.translation.z,
        );
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

// Computes the Y-axis rotation quaternion for a movement direction vector, applying the same offset used by update_car_rotation so pre-road and on-road cars rotate identically; exported as pub so main.rs can call it from update_car_rotation without duplicating the angle formula
// Input: dir_x: f32 normalized X component of the movement direction; dir_z: f32 normalized Z component of the movement direction
// Returns: Quat representing the correct Y-axis rotation for a car moving in the given direction
pub fn car_facing_quat(dir_x: f32, dir_z: f32) -> Quat {
    let angle = dir_z.atan2(dir_x);
    Quat::from_rotation_y(-angle + std::f32::consts::FRAC_PI_2)
}

// Formats a minimal HTTP response string with the given status line and plain text body
// Input: status: &str HTTP status line such as "200 OK"; body: &str plain text response body
// Returns: String containing a complete HTTP/1.1 response ready to write to a TCP stream
fn http_reply(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        body.len(),
        body
    )
}

// Reads a single HTTP request from a TCP stream and responds to GET /position with the car's current world coordinates, returning 404 for all other routes
// Input: stream: TcpStream connected to the requesting client; state: Arc<Mutex<CarHttp>> providing access to the car's current position
// Returns: none
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
                            println!(
                                "{}: received {} waypoints: {}",
                                pre.license,
                                waypoints.len(),
                                waypoints
                                    .iter()
                                    .map(|wp| wp.node_id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" -> ")
                            );
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
