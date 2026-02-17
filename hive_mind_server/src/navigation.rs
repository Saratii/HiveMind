use crate::Point;

pub struct Command {
    pub speed: f64,
    pub direction_x: f64,
    pub direction_y: f64,
}

pub fn straight_line_path(start: &Point, dest: &Point) -> Vec<Point> {
    vec![start.clone(), dest.clone()]
}

pub fn distance(start: &Point, dest: &Point) -> f64 {
    let dx = dest.x - start.x;
    let dy = dest.y - start.y;
    dx.hypot(dy)  
}

pub const SPEED: f64 = 10.0;

pub fn get_straight_line_command(start: &Point, dest: &Point) -> Command {
    let dx = dest.x - start.x;
    let dy = dest.y - start.y;
    let dist = distance(start, dest);
    
    let direction_x = if dist > 0.0 { dx / dist } else { 0.0 };
    let direction_y = if dist > 0.0 { dy / dist } else { 0.0 };
    
    Command {
        speed: SPEED,
        direction_x,
        direction_y,
    }
}  
