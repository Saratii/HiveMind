use actix_web::{App, HttpRequest, HttpResponse, HttpServer, web};
use std::sync::{Arc, Mutex};

mod navigation;

#[derive(Clone)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

struct Car {
    license: String,
    url: String,
    start: Point,
    dest: Point,
}

struct AppState {
    cars: Mutex<Vec<Car>>,
}

async fn register_car(state: web::Data<Arc<AppState>>, body: String) -> HttpResponse {
    let mut license = String::new();
    let mut url = String::new();
    let mut start_x = 0.0f64;
    let mut start_y = 0.0f64;
    let mut dest_x = 0.0f64;
    let mut dest_y = 0.0f64;
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
    let car = Car {
        license: license.clone(),
        url: url.clone(),
        start: Point {
            x: start_x,
            y: start_y,
        },
        dest: Point {
            x: dest_x,
            y: dest_y,
        },
    };
    println!(
        "Car registered: {} url={} ({:.2}, {:.2}) -> ({:.2}, {:.2})",
        car.license, car.url, car.start.x, car.start.y, car.dest.x, car.dest.y
    );
    state.cars.lock().unwrap().push(car);
    let response_body = format!("Car registered: {} url={}", license, url);
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(response_body)
}

async fn car_count(state: web::Data<Arc<AppState>>) -> HttpResponse {
    let count = state.cars.lock().unwrap().len();
    let response_body = format!("Total cars registered: {}", count);
    HttpResponse::Ok()
        .content_type("text/plain")
        .body(response_body)
}

async fn fallback(req: HttpRequest, state: web::Data<Arc<AppState>>, body: String) -> HttpResponse {
    if req.method() == "POST" && req.path() == "/register-car" {
        register_car(state, body).await
    } else {
        car_count(state).await
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let state = Arc::new(AppState {
        cars: Mutex::new(Vec::new()),
    });
    println!("Server running on http://0.0.0.0:8080");
    HttpServer::new(move || {
        let state = web::Data::new(Arc::clone(&state));
        App::new()
            .app_data(state)
            .route("/register-car", web::post().to(register_car))
            .default_service(web::to(fallback))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
