// HiveMind server: car registration, pathfinding, command dispatch, position tracking.

use actix_web::{web, App, HttpResponse, HttpServer};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod map;
mod pathfinding;

use map::{CityGraph, CityMap, ParkingLotSpawns};
use pathfinding::{compute_path, Waypoint};

const DEFAULT_SPEED: f64 = 75.0;
const CITY_MAP_PATH: &str = "../city.json";
const BIND_ADDR: &str = "0.0.0.0:8080";
const WAYPOINT_PROXIMITY: f64 = 30.0;
/// Min distance the car must have moved from current waypoint before we consider it "near" the next (avoids instant advance).
const MIN_PROGRESS_FROM_WP: f64 = 15.0;
const POLL_INTERVAL_SECS: f64 = 0.5;

#[derive(Clone)]
struct CarState {
    license: String,
    url: String,
    x: f64,
    y: f64,
}

struct AppState {
    cars: Mutex<HashMap<String, CarState>>,
    city_graph: CityGraph,
    parking_spawns: ParkingLotSpawns,
}

async fn send_command(client: &reqwest::Client, car_url: &str, params: &[(&str, &str)]) {
    let body = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");
    let url = format!("{}/command", car_url);
    if let Err(e) = client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
    {
        eprintln!("Command failed to {}: {}", url, e);
    }
}

async fn get_position(client: &reqwest::Client, car_url: &str) -> Option<(f64, f64)> {
    let url = format!("{}/position", car_url);
    let resp = client.get(&url).send().await.ok()?;
    let text = resp.text().await.ok()?;
    let mut x = None;
    let mut y = None;
    for pair in text.split('&') {
        let mut kv = pair.splitn(2, '=');
        let k = kv.next().unwrap_or("").trim();
        let v = kv.next().unwrap_or("").trim();
        match k {
            "x" => x = v.parse().ok(),
            "y" => y = v.parse().ok(),
            _ => {}
        }
    }
    Some((x?, y?))
}

fn start_drive_loop(
    car_url: String,
    license: String,
    path: Vec<Waypoint>,
    speed: f64,
    start_x: f64,
    start_y: f64,
    state: Arc<AppState>,
) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        tokio::time::sleep(Duration::from_millis(300)).await;

        // First command: spawn car at its lot coords (start_x, start_y), direction toward first waypoint
        if let Some(first) = path.first() {
            let dx = first.x - start_x;
            let dy = first.y - start_y;
            let dist = (dx * dx + dy * dy).sqrt();
            let (dir_x, dir_y) = if dist > 1e-9 {
                (dx / dist, dy / dist)
            } else {
                (first.dir_x, first.dir_y)
            };
            send_command(
                &client,
                &car_url,
                &[
                    ("type", "set_route"),
                    ("license", &license),
                    ("speed", &format!("{:.2}", speed)),
                    ("direction_x", &format!("{:.4}", dir_x)),
                    ("direction_y", &format!("{:.4}", dir_y)),
                    ("pos_x", &format!("{:.2}", start_x)),
                    ("pos_y", &format!("{:.2}", start_y)),
                ],
            )
            .await;
        }

        for i in 0..path.len().saturating_sub(1) {
            let wp = &path[i];
            let next = &path[i + 1];
            let (prev_x, prev_y) = if i == 0 {
                (start_x, start_y)
            } else {
                (path[i - 1].x, path[i - 1].y)
            };
            let is_last_segment = i + 1 == path.len() - 1;

            if speed <= 0.0 || wp.dist_to_next <= 0.0 {
                continue;
            }

            let travel_secs = wp.dist_to_next / speed;
            let sleep_secs = (travel_secs - POLL_INTERVAL_SECS).max(0.0);
            tokio::time::sleep(Duration::from_secs_f64(sleep_secs)).await;

            // Wait until car is near target: for last segment wait for destination (next), else wait for wp
            let target_x = if is_last_segment { next.x } else { wp.x };
            let target_y = if is_last_segment { next.y } else { wp.y };
            let _reached = loop {
                match get_position(&client, &car_url).await {
                    Some((cx, cy)) => {
                        let dist_to_target = (cx - target_x).hypot(cy - target_y);
                        let dist_from_prev = (cx - prev_x).hypot(cy - prev_y);
                        let seg_dx = target_x - prev_x;
                        let seg_dy = target_y - prev_y;
                        let seg_len_sq = seg_dx * seg_dx + seg_dy * seg_dy;
                        let overshot = seg_len_sq > 1e-9
                            && (cx - prev_x) * seg_dx + (cy - prev_y) * seg_dy >= seg_len_sq;
                        let made_progress = dist_from_prev >= MIN_PROGRESS_FROM_WP || overshot;
                        let is_near = (dist_to_target < WAYPOINT_PROXIMITY || overshot) && made_progress;
                        if let Ok(mut map) = state.cars.lock() {
                            if let Some(c) = map.get_mut(&license) {
                                c.x = cx;
                                c.y = cy;
                            }
                        }
                        if is_near {
                            break;
                        }
                    }
                    None => {}
                }
                tokio::time::sleep(Duration::from_secs_f64(POLL_INTERVAL_SECS)).await;
            };

            if is_last_segment {
                send_command(&client, &car_url, &[("type", "stop"), ("license", &license)]).await;
                println!("Car {} reached destination", license);
                break;
            }

            // Send direction from this wp toward next waypoint
            send_command(
                &client,
                &car_url,
                &[
                    ("type", "set_route"),
                    ("license", &license),
                    ("speed", &format!("{:.2}", speed)),
                    ("direction_x", &format!("{:.4}", wp.dir_x)),
                    ("direction_y", &format!("{:.4}", wp.dir_y)),
                ],
            )
            .await;
        }
    });
}

async fn register_car(state: web::Data<Arc<AppState>>, body: String) -> HttpResponse {
    let mut license = String::new();
    let mut url = String::new();
    let mut from_lot = String::new();
    let mut to_lot = String::new();

    for pair in body.split('&') {
        let mut kv = pair.splitn(2, '=');
        let key = kv.next().unwrap_or("");
        let val = kv.next().unwrap_or("");
        match key {
            "license" => license = val.to_string(),
            "url" => url = val.to_string(),
            "from" => from_lot = val.to_string(),
            "to" => to_lot = val.to_string(),
            _ => {}
        }
    }

    if license.is_empty() || url.is_empty() || from_lot.is_empty() || to_lot.is_empty() {
        return HttpResponse::BadRequest()
            .content_type("text/plain")
            .body("Missing license, url, from, or to");
    }

    let (start_x, start_y) = match state.parking_spawns.get(&from_lot) {
        Some(cfg) => (cfg.spawn[0], cfg.spawn[1]),
        None => {
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body(format!("Unknown parking lot: {}", from_lot));
        }
    };

    let (dest_x, dest_y) = match state.parking_spawns.get(&to_lot) {
        Some(cfg) => (cfg.spawn[0], cfg.spawn[1]),
        None => {
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body(format!("Unknown parking lot: {}", to_lot));
        }
    };

    let path = match compute_path(&state.city_graph, start_x, start_y, dest_x, dest_y) {
        Some(p) => p,
        None => {
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body(format!("No path from {} to {}", from_lot, to_lot));
        }
    };

    let car = CarState {
        license: license.clone(),
        url: url.clone(),
        x: start_x,
        y: start_y,
    };
    state.cars.lock().unwrap().insert(license.clone(), car);

    start_drive_loop(
        url.clone(),
        license.clone(),
        path,
        DEFAULT_SPEED,
        start_x,
        start_y,
        Arc::clone(state.get_ref()),
    );

    println!("Car {} registered: {} -> {}", license, from_lot, to_lot);
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(format!("status=approved&license={}&url={}", license, url))
}

async fn car_positions(state: web::Data<Arc<AppState>>) -> HttpResponse {
    let cars_list: Vec<(String, String)> = {
        let cars = state.cars.lock().unwrap();
        cars.iter()
            .map(|(license, c)| (license.clone(), c.url.clone()))
            .collect()
    };

    let client = reqwest::Client::new();
    let mut out: Vec<String> = Vec::new();
    let mut updates: Vec<(String, f64, f64)> = Vec::new();
    for (license, url) in &cars_list {
        match get_position(&client, url).await {
            Some((cx, cy)) => {
                out.push(format!("{}: x={:.2} y={:.2}", license, cx, cy));
                updates.push((license.clone(), cx, cy));
            }
            None => {
                if let Ok(cars) = state.cars.lock() {
                    if let Some(c) = cars.get(license) {
                        out.push(format!("{}: x={:.2} y={:.2} (unreachable)", license, c.x, c.y));
                    }
                }
            }
        }
    }
    if !updates.is_empty() {
        if let Ok(mut map) = state.cars.lock() {
            for (license, cx, cy) in updates {
                if let Some(c) = map.get_mut(&license) {
                    c.x = cx;
                    c.y = cy;
                }
            }
        }
    }
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(out.join("\n"))
}

async fn parking_lots(state: web::Data<Arc<AppState>>) -> HttpResponse {
    let body: Vec<String> = state
        .parking_spawns
        .iter()
        .map(|(id, cfg)| {
            format!(
                "{}: center=({:.1},{:.1}) exit=({:.1},{:.1})",
                id, cfg.spawn[0], cfg.spawn[1], cfg.exit[0], cfg.exit[1]
            )
        })
        .collect();
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(body.join("\n"))
}

async fn health() -> HttpResponse {
    HttpResponse::Ok().content_type("text/plain").body("OK")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let city_map = CityMap::load(CITY_MAP_PATH).unwrap_or_else(|e| {
        eprintln!("Error loading city: {}", e);
        std::process::exit(1);
    });
    let city_graph = city_map.build_graph();
    println!(
        "City loaded: {} nodes, {} edges, {} parking lots",
        city_graph.nodes.len(),
        city_graph.edges.len(),
        city_map.parking_spawns.len()
    );

    let state = Arc::new(AppState {
        cars: Mutex::new(HashMap::new()),
        city_graph,
        parking_spawns: city_map.parking_spawns,
    });

    println!("Server running on http://{}", BIND_ADDR);
    HttpServer::new(move || {
        let state = web::Data::new(Arc::clone(&state));
        App::new()
            .app_data(state)
            .route("/register-car", web::post().to(register_car))
            .route("/car-positions", web::get().to(car_positions))
            .route("/parking-lots", web::get().to(parking_lots))
            .route("/health", web::get().to(health))
    })
    .bind(BIND_ADDR)?
    .run()
    .await
}
