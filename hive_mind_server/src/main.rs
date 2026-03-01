/*
prologue
Name of program: main.rs
Description: Main server logic for HiveMind
Author: Maren Proplesch
Date Created: 2/11/2026
Date Revised: 3/1/2026
Revision History: Included in the numerous sprint artifacts.
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
*/

use actix_web::{App, HttpResponse, HttpServer, web};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod map;
mod pathfinding;

use map::{CityGraph, CityMap};
use pathfinding::compute_path;

const DEFAULT_SPEED: f64 = 10.0;
const ENDPOINT_HEALTH: &str = "/health";
const ENDPOINT_CAR_COUNT: &str = "/car-count";
const CITY_MAP_PATH: &str = "../city.json";
const BIND_ADDR: &str = "0.0.0.0:8080";
const REGISTRY_HOST: &str = "http://127.0.0.1:9000";
const REGISTRY_CAR_REGISTERED: &str = "/car-registered";
const CAR_COMMAND: &str = "/command";
const CAR_POSITION: &str = "/position";
const ENDPOINT_REGISTER_CAR: &str = "/register-car";
const WAYPOINT_PROXIMITY: f64 = 10.0;
const POLL_EARLY_SECS: f64 = 2.0;

//struct for a point with x and y coordinates
#[derive(Clone)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

//struct for a car with license plate, URL, and destination point
#[derive(Clone)]
struct Car {
    license: String,
    url: String,
    dest: Point,
}

//struct for application state, containing registered cars and the city graph
struct AppState {
    cars: Mutex<Vec<Car>>,
    city_graph: CityGraph,
}

// TODO: Implement actual roadway validation logic
//should determine if a car can enter based on other cars in the immediate roadway, may also need to signal them to make roomn
//inputs: car to validate
//returns true if car is allowed to enter roadway, false otherwise
fn validate_enter_roadway(_car: &Car) -> bool {
    true
}

// Forward the registered car information to the registry service asynchronously
//inputs: car license plate, car URL
//returns: None
fn forward_car(license: String, url: String) {
    tokio::spawn(async move {
        let body = format!("license={}&url={}", license, url);
        let client = reqwest::Client::new();
        let endpoint = format!("{}{}", REGISTRY_HOST, REGISTRY_CAR_REGISTERED);
        match client.post(&endpoint).body(body).send().await {
            Ok(_) => {}
            Err(e) => eprintln!("Failed to forward car to {}: {}", endpoint, e),
        }
    });
}

//send a command to the car's API endpoint asynchronously
//inputs: reqwest client, car URL, list of (key, value) parameters to include in the command
//returns: None
async fn send_car_command(client: &reqwest::Client, car_url: &str, params: &[(&str, &str)]) {
    let body = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");

    let url = format!("{}{}", car_url, CAR_COMMAND);
    match client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
    {
        Ok(r) => println!("Command to {}: status {}", url, r.status()),
        Err(e) => eprintln!("Failed to send command to {}: {}", url, e),
    }
}

// Poll the car's position from its API endpoint asynchronously
//inputs: reqwest client, car URL
//returns Some((x, y)) coordinates of the car or None if the poll fails
async fn get_car_position(client: &reqwest::Client, car_url: &str) -> Option<(f64, f64)> {
    let url = format!("{}{}", car_url, CAR_POSITION);
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

//drive loop, sending commands to the car to follow the path of waypoints and polling its position to adjust timing
//inputs: car URL, car license plate, path of waypoints, speed to drive
//returns: None
fn start_drive_loop(
    car_url: String,
    license: String,
    path: Vec<pathfinding::Waypoint>,
    speed: f64,
) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        if let Some(first) = path.first() {
            let speed_str = format!("{:.2}", speed);
            let dir_x_str = format!("{:.4}", first.dir_x);
            let dir_y_str = format!("{:.4}", first.dir_y);
            send_car_command(
                &client,
                &car_url,
                &[
                    ("type", "set_route"),
                    ("license", &license),
                    ("speed", &speed_str),
                    ("direction_x", &dir_x_str),
                    ("direction_y", &dir_y_str),
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
            let sleep_secs = (travel_secs - POLL_EARLY_SECS).max(0.0);
            tokio::time::sleep(Duration::from_secs_f64(sleep_secs)).await;
            let near = match get_car_position(&client, &car_url).await {
                Some((cx, cy)) => {
                    let dist = (cx - next.x).hypot(cy - next.y);
                    println!(
                        "Car {} position ({:.1}, {:.1}), dist to waypoint {}: {:.1}m",
                        license,
                        cx,
                        cy,
                        i + 1,
                        dist
                    );
                    dist < WAYPOINT_PROXIMITY
                }
                None => {
                    eprintln!(
                        "Car {} position poll failed at waypoint {}, proceeding anyway",
                        license,
                        i + 1
                    );
                    true
                }
            };
            if !near {
                tokio::time::sleep(Duration::from_secs_f64(POLL_EARLY_SECS)).await;
            }
            if i + 1 == path.len() - 1 {
                println!("Car {} reached destination", license);
                send_car_command(
                    &client,
                    &car_url,
                    &[("type", "stop"), ("license", &license)],
                )
                .await;
                break;
            }
            let speed_str = format!("{:.2}", speed);
            let dir_x_str = format!("{:.4}", next.dir_x);
            let dir_y_str = format!("{:.4}", next.dir_y);
            println!(
                "Car {} turning at waypoint {}: dir ({}, {})",
                license,
                i + 1,
                dir_x_str,
                dir_y_str
            );
            send_car_command(
                &client,
                &car_url,
                &[
                    ("type", "set_route"),
                    ("license", &license),
                    ("speed", &speed_str),
                    ("direction_x", &dir_x_str),
                    ("direction_y", &dir_y_str),
                ],
            )
            .await;
        }
    });
}

//endpoint for registering a car with a start and destination
//inputs: HTTP POST with body containing license, URL, start_x, start_y, dest_x, dest_y
//returns: HTTP response indicating success or failure of registration and pathfinding
async fn register_car(state: web::Data<Arc<AppState>>, body: String) -> HttpResponse {
    let mut license = String::new();
    let mut url = String::new();
    let mut start_x = 0.0;
    let mut start_y = 0.0;
    let mut dest_x = 0.0;
    let mut dest_y = 0.0;
    for pair in body.split('&') {
        let mut kv = pair.splitn(2, '=');
        let key = kv.next().unwrap_or("");
        let val = kv.next().unwrap_or("");
        match key {
            "license" => license = val.to_string(),
            "url" => url = val.to_string(),
            "start_x" => start_x = val.parse().unwrap_or(0.0),
            "start_y" => start_y = val.parse().unwrap_or(0.0),
            "dest_x" => dest_x = val.parse().unwrap_or(0.0),
            "dest_y" => dest_y = val.parse().unwrap_or(0.0),
            _ => {}
        }
    }
    let start = Point {
        x: start_x,
        y: start_y,
    };
    let dest = Point {
        x: dest_x,
        y: dest_y,
    };
    let path = match compute_path(&state.city_graph, &start, &dest) {
        Some(p) => p,
        None => {
            eprintln!(
                "No path found for car {} from ({}, {}) to ({}, {})",
                license, start_x, start_y, dest_x, dest_y
            );
            return HttpResponse::BadRequest()
                .content_type("text/plain")
                .body(format!("No path found for car {}", license));
        }
    };
    println!(
        "Car {}: path computed with {} waypoints",
        license,
        path.len()
    );
    let speed = DEFAULT_SPEED; //make this more smart later
    let car = Car {
        license: license.clone(),
        url: url.clone(),
        dest,
    };
    println!(
        "Car registered: {} url={} ({:.2}, {:.2}) -> ({:.2}, {:.2})",
        car.license, car.url, start_x, start_y, car.dest.x, car.dest.y
    );
    if !validate_enter_roadway(&car) {
        println!("Car {} denied entry to roadway", car.license);
        return HttpResponse::Forbidden()
            .content_type("text/plain")
            .body(format!("Car {} not allowed to enter roadway", license));
    }
    state.cars.lock().unwrap().push(car.clone());
    println!("Car {} validated, starting drive loop", car.license);
    start_drive_loop(url.clone(), license.clone(), path, speed);
    forward_car(license.clone(), url.clone());
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(format!("Car registered: {} url={}", license, url))
}

//endpoint for getting the total count of registered cars
//inputs: HTTP GET request
//returns: HTTP response with the total number of registered cars
async fn car_count(state: web::Data<Arc<AppState>>) -> HttpResponse {
    let count = state.cars.lock().unwrap().len();
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(format!("Total cars registered: {}", count))
}

//endpoint for health check
//inputs: HTTP GET request
//returns: HTTP response indicating server is healthy
async fn health() -> HttpResponse {
    HttpResponse::Ok().content_type("text/plain").body("OK")
}

//main server loop, run each of the endpoints
//inputs: None
//returns: Unknown server errors
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let city_map = CityMap::load(CITY_MAP_PATH).unwrap_or_else(|e| {
        eprintln!("Error loading city map: {}", e);
        std::process::exit(1);
    });
    println!("City map loaded: {} segments", city_map.segment_count());
    let city_graph = city_map.build_graph();
    println!(
        "City graph built: {} nodes, {} edges",
        city_graph.nodes.len(),
        city_graph.edges.len()
    );
    let state = Arc::new(AppState {
        cars: Mutex::new(Vec::new()),
        city_graph,
    });
    println!("Server running on http://{}", BIND_ADDR);
    HttpServer::new(move || {
        let state = web::Data::new(Arc::clone(&state));
        App::new()
            .app_data(state)
            .route(ENDPOINT_REGISTER_CAR, web::post().to(register_car))
            .route(ENDPOINT_HEALTH, web::get().to(health))
            .route(ENDPOINT_CAR_COUNT, web::get().to(car_count))
    })
    .bind(BIND_ADDR)?
    .run()
    .await
}
