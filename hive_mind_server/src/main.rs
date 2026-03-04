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
const CITY_MAP_PATH: &str = "../city.json";
const BIND_ADDR: &str = "0.0.0.0:8080";
const SCENE_PATHS: [&str; 3] = ["../city_scene1.json", "../city_scene2.json", "../city_scene3.json"];
const WAYPOINT_PROXIMITY: f64 = 30.0;
/// Min distance the car must have moved from current waypoint before we consider it "near" the next (avoids instant advance).
const MIN_PROGRESS_FROM_WP: f64 = 15.0;
const POLL_INTERVAL_SECS: f64 = 0.5;

/// Snapshot of a single car as tracked by the server (current URL and last-known position).
#[derive(Clone)]
struct CarState {
    license: String,
    url: String,
    x: f64,
    y: f64,
}

/// Application-wide state shared across handlers.
/// - `cars`: last-known positions for debug/inspection when cars are unreachable.
/// - `registered_routes`: cached routes so we can restart cars on reset.
/// - `city_graph` / `parking_spawns`: current scene data, hot-swapped on `/switch-scene`.
struct AppState {
    cars: Mutex<HashMap<String, CarState>>,
    registered_routes: Mutex<HashMap<String, (String, Vec<Waypoint>, f64, f64)>>,
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

/// Drive loop for a single car: sends initial spawn, then advances along the waypoint path
/// while polling position to decide when to move on to the next segment.
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
                // Gets the position of the car
                match get_position(&client, &car_url).await {
                    Some((cx, cy)) => {
                        // Calculates the distance to the target
                        let dist_to_target = (cx - target_x).hypot(cy - target_y);
                        // Calculates the distance from the previous position
                        let dist_from_prev = (cx - prev_x).hypot(cy - prev_y);
                        let seg_dx = target_x - prev_x;
                        let seg_dy = target_y - prev_y;
                        // Calculates the length of the segment squared
                        let seg_len_sq = seg_dx * seg_dx + seg_dy * seg_dy;
                        // Calculates if the car has overshot the segment
                        let overshot = seg_len_sq > 1e-9
                            && (cx - prev_x) * seg_dx + (cy - prev_y) * seg_dy >= seg_len_sq;
                        // Calculates if the car has made progress
                        let made_progress = dist_from_prev >= MIN_PROGRESS_FROM_WP || overshot;
                        // Calculates if the car is near the target
                        let is_near = (dist_to_target < WAYPOINT_PROXIMITY || overshot) && made_progress;
                        // Updates the car's position in the map
                        if let Ok(mut map) = state.cars.lock() {
                            // Updates the car's position in the map
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
                // Waits for the next poll interval
                tokio::time::sleep(Duration::from_secs_f64(POLL_INTERVAL_SECS)).await;
            };
            // If the car has reached the last segment, stop the car
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
    // Gets the start x and y coordinates from the from lot
    let (start_x, start_y) = match state.parking_spawns.lock().unwrap().get(&from_lot) {
        // If the from lot is found, return the start x and y coordinates
        Some(cfg) => (cfg.spawn[0], cfg.spawn[1]),
        // If the from lot is not found, return a bad request
        None => {
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body(format!("Unknown parking lot: {}", from_lot));
        }
    };
    // Gets the destination x and y coordinates from the to lot
    let (dest_x, dest_y) = match state.parking_spawns.lock().unwrap().get(&to_lot) {
        // If the to lot is found, return the destination x and y coordinates
        Some(cfg) => (cfg.spawn[0], cfg.spawn[1]),
        // If the to lot is not found, return a bad request
        None => {
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body(format!("Unknown parking lot: {}", to_lot));
        }
    };
    // Computes the path from the start to the destination
    let path = match compute_path(&state.city_graph.lock().unwrap(), start_x, start_y, dest_x, dest_y) {
        // If the path is found, return the path
        Some(p) => p,
        // If the path is not found, return a bad request
        None => {
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body(format!("No path from {} to {}", from_lot, to_lot));
        }
    };
    // Creates a new car state
    let car = CarState {
        license: license.clone(),
        url: url.clone(),
        x: start_x,
        y: start_y,
    };
    // Inserts the car state into the cars map
    state.cars.lock().unwrap().insert(license.clone(), car);
    // Inserts the registered route into the registered routes map
    state
        .registered_routes
        .lock()
        .unwrap()
        .insert(license.clone(), (url.clone(), path.clone(), start_x, start_y));
    // Starts the drive loop
    start_drive_loop(
        url.clone(),
        license.clone(),
        path,
        DEFAULT_SPEED,
        start_x,
        start_y,
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
                "{}: center=({:.1},{:.1}) exit=({:.1},{:.1})",
                id, cfg.spawn[0], cfg.spawn[1], cfg.exit[0], cfg.exit[1]
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

/// `POST /switch-scene` – swap to a new city JSON, rebuild the graph, and clear all cars/routes.
async fn switch_scene(state: web::Data<Arc<AppState>>, body: String) -> HttpResponse {
    // Gets the scene from the body
    let scene = body
        // For each pair in the body, get the key and value
        .split('&')
        .find_map(|p| {
            let mut kv = p.splitn(2, '=');
            // Gets the key from the pair
            let k = kv.next()?.trim();
            let v = kv.next()?.trim();
            if k == "scene" {
                v.parse::<usize>().ok()
            } else {
                None
            }
        })
        .and_then(|n| if (1..=3).contains(&n) { Some(n) } else { None });
    // If the scene is not found, return a bad request
    let Some(scene) = scene else {
        return HttpResponse::BadRequest()
            .content_type("text/plain")
            .body("Missing or invalid scene=1|2|3");
    };
    // Gets the path from the scene
    let path = SCENE_PATHS[scene - 1];
    // Loads the city map from the path
    let city_map = match CityMap::load(path) {
        // If the city map is found, return the city map
        Ok(m) => m,
        // If the city map is not found, return a bad request
        Err(e) => {
            return HttpResponse::InternalServerError()
                .content_type("text/plain")
                .body(format!("Failed to load {}: {}", path, e));
        }
    };
    // Builds the graph for the city map
    let graph = city_map.build_graph();
    // Clears the cars in the state
    state.cars.lock().unwrap().clear();
    // Clears the registered routes in the state
    state.registered_routes.lock().unwrap().clear();
    // Updates the city graph in the state
    *state.city_graph.lock().unwrap() = graph;
    // Updates the parking spawns in the state
    *state.parking_spawns.lock().unwrap() = city_map.parking_spawns;
    println!("Switched to scene {}", scene);
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(format!("ok scene={}", scene))
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
    // Gets the routes from the state
    let routes = state.registered_routes.lock().unwrap();
    // For each license, start the drive loop
    for license in licenses {
        // If the route is found, start the drive loop
        if let Some((ref url, ref path, start_x, start_y)) = routes.get(&license) {
            // Starts the drive loop
            start_drive_loop(
                url.clone(),
                license.clone(),
                path.clone(),
                DEFAULT_SPEED,
                *start_x,
                *start_y,
                Arc::clone(state.get_ref()),
            );
            // Prints the car reset message
            println!("Car {} reset to start", license);
        }
    }
    // Returns the ok status
    HttpResponse::Ok()
        .content_type("text/plain")
        .body("ok")
}

/// Server entrypoint: load initial city, build graph, create shared state and bind all routes.
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Loads the city map from the path
    let city_map = CityMap::load(CITY_MAP_PATH).unwrap_or_else(|e| {
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
            .route("/switch-scene", web::post().to(switch_scene))
            .route("/reset-car", web::post().to(reset_car))
    })
    // Binds the server to the address
    .bind(BIND_ADDR)?
    // Runs the server
    .run()
    .await
}
