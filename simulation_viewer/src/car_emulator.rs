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
