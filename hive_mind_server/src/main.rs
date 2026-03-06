/*
prologue
Name of program: HiveMind Server
Description: Actix-web server that registers cars, computes routes, and orchestrates scenes for the HiveMind traffic simulation.
Author: Muhammad Ibrahim, Maren Proplesch
Date Created: 3/1/2026
Date Revised: 3/2/2026
Revision History: Included in the numerous sprint artifacts.
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
*/

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
const BIND_ADDR: &str = "0.0.0.0:8080";
const WAYPOINT_PROXIMITY: f64 = 60.0;
const MIN_PROGRESS_FROM_WP: f64 = 10.0;
const POLL_INTERVAL_SECS: f64 = 0.5;
const ENTRANCE_PROXIMITY: f64 = 5.0;

/// Snapshot of a single car as tracked by the server (current URL and last-known position).
#[derive(Clone)]
struct CarState {
    license: String,
    url: String,
    x: f64,
    y: f64,
}

struct AppState {
    cars: Mutex<HashMap<String, CarState>>,
    registered_routes: Mutex<HashMap<String, (String, Vec<Waypoint>, String, String)>>,
    city_graph: Mutex<CityGraph>,
    parking_spawns: Mutex<ParkingLotSpawns>,
}

/// Fire-and-forget helper that posts a command payload to a car's `/command` endpoint.
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

/// Query a car's `/position` endpoint and parse its `x`/`y` coordinates.
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

/// Leave lot (drive to entrance, wait, enter roadway), then drive loop to dest entrance, then go to dest center.
fn run_car_trip(
    car_url: String,
    license: String,
    path: Vec<Waypoint>,
    from_lot: String,
    to_lot: String,
    state: Arc<AppState>,
) {
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let (entrance_from, dest_center_x, dest_center_y) = {
            let spawns = state_clone.parking_spawns.lock().unwrap();
            let from_cfg = spawns.get(&from_lot).expect("from_lot");
            let to_cfg = spawns.get(&to_lot).expect("to_lot");
            (
                (from_cfg.entrance[0], from_cfg.entrance[1]),
                to_cfg.spawn[0],
                to_cfg.spawn[1],
            )
        };
        tokio::time::sleep(Duration::from_millis(500)).await;

        send_command(
            &client,
            &car_url,
            &[
                ("type", "drive_to_entrance"),
                ("entrance_x", &format!("{:.2}", entrance_from.0)),
                ("entrance_y", &format!("{:.2}", entrance_from.1)),
            ],
        )
        .await;

        loop {
            tokio::time::sleep(Duration::from_secs_f64(POLL_INTERVAL_SECS)).await;
            if let Some((cx, cy)) = get_position(&client, &car_url).await {
                if (cx - entrance_from.0).hypot(cy - entrance_from.1) <= ENTRANCE_PROXIMITY {
                    break;
                }
            }
        }

        send_command(
            &client,
            &car_url,
            &[
                ("type", "enter_roadway"),
                ("road_x", &format!("{:.2}", entrance_from.0)),
                ("road_y", &format!("{:.2}", entrance_from.1)),
            ],
        )
        .await;

        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if let Some((cx, cy)) = get_position(&client, &car_url).await {
                if (cx - entrance_from.0).hypot(cy - entrance_from.1) <= ENTRANCE_PROXIMITY {
                    break;
                }
            }
        }

        start_drive_loop(
            car_url,
            license,
            path,
            DEFAULT_SPEED,
            entrance_from.0,
            entrance_from.1,
            Some((dest_center_x, dest_center_y)),
            state_clone,
        );
    });
}

fn start_drive_loop(
    car_url: String,
    license: String,
    path: Vec<Waypoint>,
    speed: f64,
    start_x: f64,
    start_y: f64,
    dest_center: Option<(f64, f64)>,
    state: Arc<AppState>,
) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        tokio::time::sleep(Duration::from_millis(200)).await;

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

            if speed <= 0.0 {
                continue;
            }
            // Don't skip when dist_to_next is 0 (graph nodes) — we still must wait and send next direction
            let travel_secs = wp.dist_to_next / speed;
            let sleep_secs = (travel_secs - POLL_INTERVAL_SECS).max(0.0);
            tokio::time::sleep(Duration::from_secs_f64(sleep_secs)).await;

            // Wait until car is near target: for last segment wait for destination (next), else wait for wp
            let target_x = if is_last_segment { next.x } else { wp.x };
            let target_y = if is_last_segment { next.y } else { wp.y };
            // true = reached by being close (smooth, snap ok); false = reached by overshoot (don't snap, avoid backward warp)
            let reached_by_proximity = loop {
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
                            break dist_to_target < WAYPOINT_PROXIMITY;
                        }
                    }
                    None => {}
                }
                tokio::time::sleep(Duration::from_secs_f64(POLL_INTERVAL_SECS)).await;
            };
            if is_last_segment {
                if let Some((cx, cy)) = dest_center {
                    send_command(
                        &client,
                        &car_url,
                        &[
                            ("type", "go_to_lot_center"),
                            ("center_x", &format!("{:.2}", cx)),
                            ("center_y", &format!("{:.2}", cy)),
                        ],
                    )
                    .await;
                    println!("Car {} reached destination lot, driving to center", license);
                } else {
                    send_command(&client, &car_url, &[("type", "stop"), ("license", &license)]).await;
                    println!("Car {} reached destination", license);
                }
                break;
            }

            // Direction from this wp to next (wp.dir_x/dir_y can be 0 at graph nodes, so always compute)
            let dx = next.x - wp.x;
            let dy = next.y - wp.y;
            let dist = (dx * dx + dy * dy).sqrt();
            let (dir_x, dir_y) = if dist > 1e-9 {
                (dx / dist, dy / dist)
            } else {
                (wp.dir_x, wp.dir_y)
            };
            // Only snap to waypoint when we reached it by proximity; if we overshot, don't warp car backward
            let mut params: Vec<(&str, String)> = vec![
                ("type", "set_route".into()),
                ("license", license.clone()),
                ("speed", format!("{:.2}", speed)),
                ("direction_x", format!("{:.4}", dir_x)),
                ("direction_y", format!("{:.4}", dir_y)),
            ];
            if reached_by_proximity {
                params.push(("pos_x", format!("{:.2}", wp.x)));
                params.push(("pos_y", format!("{:.2}", wp.y)));
            }
            let params_ref: Vec<(&str, &str)> = params
                .iter()
                .map(|(k, v)| (*k, v.as_str()))
                .collect();
            send_command(&client, &car_url, &params_ref).await;
        }
    });
}

/// `POST /register-car` – parse form body, look up parking lots, compute path and start the drive loop.
async fn register_car(state: web::Data<Arc<AppState>>, body: String) -> HttpResponse {
    let mut license = String::new();
    let mut url = String::new();
    let mut from_lot = String::new();
    let mut to_lot = String::new();
    // For each pair in the body, get the key and value
    for pair in body.split('&') {
        let mut kv = pair.splitn(2, '=');
        let key = kv.next().unwrap_or("");
        // Gets the value from the pair
        let val = kv.next().unwrap_or("");
        match key {
            "license" => license = val.to_string(),
            "url" => url = val.to_string(),
            "from" => from_lot = val.to_string(),
            "to" => to_lot = val.to_string(),
            _ => {}
        }
    }
    // If the license, url, from lot, or to lot is empty, return a bad request
    if license.is_empty() || url.is_empty() || from_lot.is_empty() || to_lot.is_empty() {
        return HttpResponse::BadRequest()
            .content_type("text/plain")
            .body("Missing license, url, from, or to");
    }
    let (start_x, start_y, path) = {
        let spawns = state.parking_spawns.lock().unwrap();
        let from_cfg = match spawns.get(&from_lot) {
            Some(c) => c,
            None => {
                return HttpResponse::BadRequest()
                    .content_type("text/plain")
                    .body(format!("Unknown parking lot: {}", from_lot));
            }
        };
        let to_cfg = match spawns.get(&to_lot) {
            Some(c) => c,
            None => {
                return HttpResponse::BadRequest()
                    .content_type("text/plain")
                    .body(format!("Unknown parking lot: {}", to_lot));
            }
        };
        let entrance_from = (from_cfg.entrance[0], from_cfg.entrance[1]);
        let entrance_to = (to_cfg.entrance[0], to_cfg.entrance[1]);
        let path = match compute_path(
            &state.city_graph.lock().unwrap(),
            entrance_from.0,
            entrance_from.1,
            entrance_to.0,
            entrance_to.1,
        ) {
            Some(p) => p,
            None => {
                return HttpResponse::BadRequest()
                    .content_type("text/plain")
                    .body(format!("No path from {} to {}", from_lot, to_lot));
            }
        };
        (from_cfg.spawn[0], from_cfg.spawn[1], path)
    };

    let car = CarState {
        license: license.clone(),
        url: url.clone(),
        x: start_x,
        y: start_y,
    };
    state.cars.lock().unwrap().insert(license.clone(), car);
    state
        .registered_routes
        .lock()
        .unwrap()
        .insert(license.clone(), (url.clone(), path.clone(), from_lot.clone(), to_lot.clone()));

    run_car_trip(
        url.clone(),
        license.clone(),
        path,
        from_lot.clone(),
        to_lot.clone(),
        Arc::clone(state.get_ref()),
    );
    // Prints the car registered message
    println!("Car {} registered: {} -> {}", license, from_lot, to_lot);
    // Returns the approved status
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(format!("status=approved&license={}&url={}", license, url))
}

/// `GET /car-positions` – live-poll every registered car and return a text summary of locations.
async fn car_positions(state: web::Data<Arc<AppState>>) -> HttpResponse {
    // Gets the list of cars
    let cars_list: Vec<(String, String)> = {
        // Gets the cars from the state
        let cars = state.cars.lock().unwrap();
        cars.iter()
            .map(|(license, c)| (license.clone(), c.url.clone()))
            .collect()
    };
    // Creates a new client
    let client = reqwest::Client::new();
    // Initializes the output vector
    let mut out: Vec<String> = Vec::new();
    // Initializes the updates vector
    let mut updates: Vec<(String, f64, f64)> = Vec::new();
    // For each car, get the position
    for (license, url) in &cars_list {
        // Gets the position of the car
        match get_position(&client, url).await {
            // If the position is found, add the position to the output vector
            Some((cx, cy)) => {
                out.push(format!("{}: x={:.2} y={:.2}", license, cx, cy));
                updates.push((license.clone(), cx, cy));
            }
            // If the position is not found, add the car to the output vector
            None => {
                // Gets the cars from the state
                if let Ok(cars) = state.cars.lock() {
                    // If the car is found, add the car to the output vector
                    if let Some(c) = cars.get(license) {
                        // Adds the car to the output vector
                        out.push(format!("{}: x={:.2} y={:.2} (unreachable)", license, c.x, c.y));
                    }
                }
            }
        }
    }
    // If the updates vector is not empty, update the cars in the state
    if !updates.is_empty() {
        // Gets the cars from the state
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

/// `GET /parking-lots` – list parking lot centers and exits for the current scene.
async fn parking_lots(state: web::Data<Arc<AppState>>) -> HttpResponse {
    let body: Vec<String> = state
        .parking_spawns
        .lock()
        .unwrap()
        .iter()
        .map(|(id, cfg)| {
            format!(
                "{}: center=({:.1},{:.1}) entrance=({:.1},{:.1})",
                id, cfg.spawn[0], cfg.spawn[1], cfg.entrance[0], cfg.entrance[1]
            )
        })
        .collect();

    HttpResponse::Ok()
        .content_type("text/plain")
        .body(body.join("\n"))
}

/// Simple health probe used by the Python demo to wait for the server.
async fn health() -> HttpResponse {
    HttpResponse::Ok().content_type("text/plain").body("OK")
}

/// `POST /remove-all-cars` – clear all registered cars and routes (stops tracking; call before killing car processes).
async fn remove_all_cars(state: web::Data<Arc<AppState>>) -> HttpResponse {
    state.cars.lock().unwrap().clear();
    state.registered_routes.lock().unwrap().clear();
    println!("All cars removed");
    HttpResponse::Ok()
        .content_type("text/plain")
        .body("ok")
}

/// `POST /reset-car` – restart all cars (or a subset) from their original spawn positions.
async fn reset_car(state: web::Data<Arc<AppState>>, body: String) -> HttpResponse {
    // Gets the licenses from the body
    let licenses: Vec<String> = if body.is_empty() {
        // Gets the licenses from the state
        state
            .registered_routes
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    } else {
        body.split('&')
            .filter_map(|p| {
                let mut kv = p.splitn(2, '=');
                // Gets the key from the pair
                let k = kv.next()?.trim();
                // Gets the value from the pair
                let v = kv.next()?.trim();
                if k == "license" && !v.is_empty() {
                    Some(v.to_string())
                } else {
                    None
                }
            })
            .collect()
    };
    let routes = state.registered_routes.lock().unwrap();
    for license in licenses {
        if let Some((ref url, ref path, ref from_lot, ref to_lot)) = routes.get(&license) {
            run_car_trip(
                url.clone(),
                license.clone(),
                path.clone(),
                from_lot.clone(),
                to_lot.clone(),
                Arc::clone(state.get_ref()),
            );
            println!("Car {} reset to start", license);
        }
    }
    // Returns the ok status
    HttpResponse::Ok()
        .content_type("text/plain")
        .body("ok")
}

/// Resolve city.json path: prefer same dir as exe (go up from target/debug to hive_mind_server, then ../city.json).
fn city_map_path() -> String {
    if let Ok(exe) = std::env::current_exe() {
        // exe is typically .../hive_mind_server/target/debug/hive_mind_server.exe
        if let Some(base) = exe.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
            let path = base.join("..").join("city.json");
            if path.exists() {
                return path.to_string_lossy().into_owned();
            }
        }
    }
    "../city.json".to_string()
}

/// Server entrypoint: load initial city, build graph, create shared state and bind all routes.
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let city_path = city_map_path();
    eprintln!("Loading city from: {}", city_path);
    // Loads the city map from the path
    let city_map = CityMap::load(&city_path).unwrap_or_else(|e| {
        eprintln!("Error loading city: {}", e);
        std::process::exit(1);
    });
    // Builds the graph for the city map
    let city_graph = city_map.build_graph();
    // Prints the city loaded message
    println!(
        "City loaded: {} nodes, {} edges, {} parking lots",
        city_graph.nodes.len(),
        city_graph.edges.len(),
        city_map.parking_spawns.len()
    );

    // Creates a new state
    let state = Arc::new(AppState {
        // Initializes the cars map
        cars: Mutex::new(HashMap::new()),
        // Initializes the registered routes map
        registered_routes: Mutex::new(HashMap::new()),
        // Initializes the city graph
        city_graph: Mutex::new(city_graph),
        // Initializes the parking spawns
        parking_spawns: Mutex::new(city_map.parking_spawns),
    });

    // Prints the server running message
    println!("Server running on http://{}", BIND_ADDR);
    // Creates a new server
    HttpServer::new(move || {
        let state = web::Data::new(Arc::clone(&state));
        // Creates a new app
        App::new()
            // Adds the state to the app
            .app_data(state)
            .route("/register-car", web::post().to(register_car))
            .route("/car-positions", web::get().to(car_positions))
            .route("/parking-lots", web::get().to(parking_lots))
            .route("/health", web::get().to(health))
            .route("/reset-car", web::post().to(reset_car))
            .route("/remove-all-cars", web::post().to(remove_all_cars))
    })
    // Binds the server to the address
    .bind(BIND_ADDR)?
    // Runs the server
    .run()
    .await
}
