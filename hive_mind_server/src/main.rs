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
    state: Arc<AppState>,
) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        tokio::time::sleep(Duration::from_millis(300)).await;

        if let Some(first) = path.first() {
            send_command(
                &client,
                &car_url,
                &[
                    ("type", "set_route"),
                    ("license", &license),
                    ("speed", &format!("{:.2}", speed)),
                    ("direction_x", &format!("{:.4}", first.dir_x)),
                    ("direction_y", &format!("{:.4}", first.dir_y)),
                    ("pos_x", &format!("{:.2}", first.x)),
                    ("pos_y", &format!("{:.2}", first.y)),
                ],
            )
            .await;
        }

        for i in 0..path.len().saturating_sub(1) {
            let wp = &path[i];
            let next = &path[i + 1];

            if speed <= 0.0 || wp.dist_to_next <= 0.0 {
                continue;
            }

            let travel_secs = wp.dist_to_next / speed;
            let sleep_secs = (travel_secs - POLL_INTERVAL_SECS).max(0.0);
            tokio::time::sleep(Duration::from_secs_f64(sleep_secs)).await;

            let near = match get_position(&client, &car_url).await {
                Some((cx, cy)) => {
                    let dist = (cx - next.x).hypot(cy - next.y);
                    if dist < WAYPOINT_PROXIMITY {
                        if let Ok(mut map) = state.cars.lock() {
                            if let Some(c) = map.get_mut(&license) {
                                c.x = cx;
                                c.y = cy;
                            }
                        }
                        true
                    } else {
                        if let Ok(mut map) = state.cars.lock() {
                            if let Some(c) = map.get_mut(&license) {
                                c.x = cx;
                                c.y = cy;
                            }
                        }
                        false
                    }
                }
                None => true,
            };

            if !near {
                tokio::time::sleep(Duration::from_secs_f64(POLL_INTERVAL_SECS)).await;
            }

            if i + 1 == path.len() - 1 {
                send_command(&client, &car_url, &[("type", "stop"), ("license", &license)]).await;
                println!("Car {} reached destination", license);
                break;
            }

            send_command(
                &client,
                &car_url,
                &[
                    ("type", "set_route"),
                    ("license", &license),
                    ("speed", &format!("{:.2}", speed)),
                    ("direction_x", &format!("{:.4}", next.dir_x)),
                    ("direction_y", &format!("{:.4}", next.dir_y)),
                    ("pos_x", &format!("{:.2}", next.x)),
                    ("pos_y", &format!("{:.2}", next.y)),
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
