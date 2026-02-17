use crate::Point;

pub fn straight_line_path(start: &Point, dest: &Point) -> Vec<Point> {
    vec![start.clone(), dest.clone()]
}

pub fn distance(start: &Point, dest: &Point) -> f64 {
    let dx = dest.x - start.x;
    let dy = dest.y - start.y;
    dx.hypot(dy)  
}

pub const SPEED: f64 = 10.0;  
