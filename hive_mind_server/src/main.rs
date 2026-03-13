/*
prologue
Name of program: main.rs
Description: Main server logic for HiveMind
Author: Maren Proplesch
Date Created: 2/11/2026
Date Revised: 3/13/2026
Revision History: Included in the numerous sprint artifacts.
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
*/

use actix_web::{App, HttpResponse, HttpServer, web};
use std::sync::{Arc, Mutex};

mod map;
mod pathfinding;

use map::CityMap;
use pathfinding::compute_path;

use crate::map::CityGraph;

const DEFAULT_SPEED: f64 = 40.0;
const ENDPOINT_HEALTH: &str = "/health";
const ENDPOINT_CAR_COUNT: &str = "/car-count";
const ENDPOINT_VALIDATE_ENTRY: &str = "/validate-entry";
const CITY_MAP_PATH: &str = "../city.json";
const BIND_ADDR: &str = "0.0.0.0:8080";
const ENDPOINT_REGISTER_CAR: &str = "/register-car";

#[derive(Clone)]
struct Car {
    license: String,
    _url: String, //leave
}

struct AppState {
    cars: Mutex<Vec<Car>>,
    pending: Mutex<std::collections::HashMap<String, (Car, Vec<pathfinding::Waypoint>)>>,
    city_graph: CityGraph,
}

fn validate_enter_roadway(_car: &Car) -> bool {
    true
}

fn encode_waypoints(path: &[pathfinding::Waypoint]) -> String {
    let mut parts = vec![format!("wp_count={}", path.len())];
    for (i, wp) in path.iter().enumerate() {
        parts.push(format!("wp{}={}", i, wp.node_id));
    }
    parts.join("&")
}
async fn register_car(state: web::Data<Arc<AppState>>, body: String) -> HttpResponse {
    let mut license = String::new();
    let mut url = String::new();
    let mut start_id = String::new();
    let mut dest_id = String::new();
    for pair in body.split('&') {
        let mut kv = pair.splitn(2, '=');
        let key = kv.next().unwrap_or("");
        let val = kv.next().unwrap_or("");
        match key {
            "license" => license = val.to_string(),
            "url" => url = val.to_string(),
            "start_id" => start_id = val.to_string(),
            "dest_id" => dest_id = val.to_string(),
            _ => {}
        }
    }
    let path = match compute_path(&state.city_graph, &start_id, &dest_id) {
        Some(p) => p,
        None => {
            eprintln!(
                "No path for car {} from '{}' to '{}'",
                license, start_id, dest_id
            );
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body(format!("No path found for car {}", license));
        }
    };
    println!(
        "Car {}: registered, path has {} waypoints. Waiting for entry validation.",
        license,
        path.len()
    );
    let car = Car {
        license: license.clone(),
        _url: url.clone(),
    };
    state
        .pending
        .lock()
        .unwrap()
        .insert(license.clone(), (car, path));
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(format!("registered={}", license))
}

async fn validate_entry(state: web::Data<Arc<AppState>>, body: String) -> HttpResponse {
    let mut license = String::new();
    for pair in body.split('&') {
        let mut kv = pair.splitn(2, '=');
        if kv.next().unwrap_or("").trim() == "license" {
            license = kv.next().unwrap_or("").trim().to_string();
        }
    }
    let entry = state.pending.lock().unwrap().remove(&license);
    let (car, path) = match entry {
        Some(v) => v,
        None => {
            eprintln!("validate_entry: unknown license {}", license);
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body(format!("unknown license: {}", license));
        }
    };
    if !validate_enter_roadway(&car) {
        state
            .pending
            .lock()
            .unwrap()
            .insert(license.clone(), (car, path));
        println!("Car {} denied roadway entry, will retry", license);
        return HttpResponse::Ok()
            .content_type("text/plain")
            .body("allowed=false");
    }
    println!(
        "Car {} granted roadway entry, sending {} waypoints",
        license,
        path.len()
    );
    state.cars.lock().unwrap().push(car.clone());
    let license_evict = license.clone();
    let state_evict = Arc::clone(&**state);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        state_evict
            .cars
            .lock()
            .unwrap()
            .retain(|c| c.license != license_evict);
    });
    let wp_payload = encode_waypoints(&path);
    let response_body = format!("allowed=true&speed={:.2}&{}", DEFAULT_SPEED, wp_payload);
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(response_body)
}

async fn car_count(state: web::Data<Arc<AppState>>) -> HttpResponse {
    let count = state.cars.lock().unwrap().len();
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(format!("Total cars registered: {}", count))
}

async fn health() -> HttpResponse {
    HttpResponse::Ok().content_type("text/plain").body("OK")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let city_map = CityMap::load(CITY_MAP_PATH).unwrap_or_else(|e| {
        eprintln!("Error loading city map: {}", e);
        std::process::exit(1);
    });
    println!("City map loaded: {} nodes", city_map.node_count());
    let city_graph = city_map.build_graph();
    println!(
        "City graph built: {} nodes, {} edges",
        city_graph.nodes.len(),
        city_graph.edges.len()
    );
    let state = Arc::new(AppState {
        cars: Mutex::new(Vec::new()),
        pending: Mutex::new(std::collections::HashMap::new()),
        city_graph,
    });
    println!("Server running on http://{}", BIND_ADDR);
    HttpServer::new(move || {
        let state = web::Data::new(Arc::clone(&state));
        App::new()
            .app_data(state)
            .route(ENDPOINT_REGISTER_CAR, web::post().to(register_car))
            .route(ENDPOINT_VALIDATE_ENTRY, web::post().to(validate_entry))
            .route(ENDPOINT_HEALTH, web::get().to(health))
            .route(ENDPOINT_CAR_COUNT, web::get().to(car_count))
    })
    .bind(BIND_ADDR)?
    .run()
    .await
}
