use bevy::prelude::*;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ParkingPortal {
    pub node: GraphNode,
    pub center: [f32; 2],
}

#[derive(Debug)]
pub struct CityData {
    pub nodes: HashMap<String, GraphNode>,
    pub portals: Vec<ParkingPortal>,
}

impl CityData {
    pub fn node_pos(&self, id: &str) -> (f32, f32) {
        self.nodes.get(id).map(|n| (n.x, n.y)).unwrap_or((0.0, 0.0))
    }
}

#[derive(Debug, Clone)]
pub struct Waypoint {
    pub node_id: String,
    pub x: f32,
    pub z: f32,
}

#[derive(Component)]
pub struct PortalMarker {
    pub portal_index: usize,
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub connects: Vec<String>,
}

pub fn parse_waypoints(params: &HashMap<String, String>, city: &CityData) -> Vec<Waypoint> {
    let count: usize = params
        .get("wp_count")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    (0..count)
        .filter_map(|i| {
            let id = params.get(&format!("wp{}", i))?.clone();
            let (x, z) = city.node_pos(&id);
            Some(Waypoint { node_id: id, x, z })
        })
        .collect()
}

pub fn parse_city(src: &str) -> CityData {
    let mut p = Parser {
        s: src.as_bytes(),
        i: 0,
    };
    p.parse_city()
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> u8 {
        self.s[self.i]
    }
    fn eat(&mut self) {
        self.i += 1;
    }
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && matches!(self.peek(), b' ' | b'\t' | b'\r' | b'\n') {
            self.eat();
        }
    }
    fn expect(&mut self, b: u8) {
        self.skip_ws();
        assert_eq!(self.peek(), b, "expected '{}' at {}", b as char, self.i);
        self.eat();
    }
    fn parse_string(&mut self) -> String {
        self.skip_ws();
        self.expect(b'"');
        let mut out = String::new();
        loop {
            let c = self.peek();
            self.eat();
            if c == b'"' {
                break;
            }
            if c == b'\\' {
                let e = self.peek();
                self.eat();
                out.push(e as char);
            } else {
                out.push(c as char);
            }
        }
        out
    }
    fn parse_f32(&mut self) -> f32 {
        self.skip_ws();
        let start = self.i;
        if self.peek() == b'-' {
            self.eat();
        }
        while self.i < self.s.len()
            && matches!(self.peek(), b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        {
            self.eat();
        }
        std::str::from_utf8(&self.s[start..self.i])
            .unwrap()
            .parse()
            .unwrap()
    }
    fn parse_connects(&mut self) -> Vec<String> {
        self.expect(b'[');
        let mut out = Vec::new();
        self.skip_ws();
        while self.peek() != b']' {
            out.push(self.parse_string());
            self.skip_ws();
            if self.peek() == b',' {
                self.eat();
                self.skip_ws();
            }
        }
        self.eat();
        out
    }

    fn parse_pt2(&mut self) -> [f32; 2] {
        self.expect(b'[');
        let x = self.parse_f32();
        self.skip_ws();
        self.expect(b',');
        let y = self.parse_f32();
        self.skip_ws();
        self.expect(b']');
        [x, y]
    }

    fn parse_plain_node(&mut self, id: String) -> GraphNode {
        self.expect(b'{');
        let mut x = 0f32;
        let mut y = 0f32;
        let mut connects = Vec::new();
        self.skip_ws();
        while self.peek() != b'}' {
            let key = self.parse_string();
            self.skip_ws();
            self.expect(b':');
            match key.as_str() {
                "x" => x = self.parse_f32(),
                "y" => y = self.parse_f32(),
                "connects" => connects = self.parse_connects(),
                _ => self.skip_value(),
            }
            self.skip_ws();
            if self.peek() == b',' {
                self.eat();
                self.skip_ws();
            }
        }
        self.eat();
        GraphNode { id, x, y, connects }
    }

    fn parse_portal_node(&mut self, id: String) -> ParkingPortal {
        self.expect(b'{');
        let mut x = 0f32;
        let mut y = 0f32;
        let mut center = [0f32; 2];
        let mut connects = Vec::new();
        self.skip_ws();
        while self.peek() != b'}' {
            let key = self.parse_string();
            self.skip_ws();
            self.expect(b':');
            match key.as_str() {
                "x" => x = self.parse_f32(),
                "y" => y = self.parse_f32(),
                "center" => center = self.parse_pt2(),
                "connects" => connects = self.parse_connects(),
                _ => self.skip_value(),
            }
            self.skip_ws();
            if self.peek() == b',' {
                self.eat();
                self.skip_ws();
            }
        }
        self.eat();
        ParkingPortal {
            node: GraphNode { id, x, y, connects },
            center,
        }
    }

    fn parse_node_map(&mut self) -> Vec<GraphNode> {
        self.expect(b'{');
        let mut out = Vec::new();
        self.skip_ws();
        while self.peek() != b'}' {
            let id = self.parse_string();
            self.skip_ws();
            self.expect(b':');
            out.push(self.parse_plain_node(id));
            self.skip_ws();
            if self.peek() == b',' {
                self.eat();
                self.skip_ws();
            }
        }
        self.eat();
        out
    }

    fn parse_portal_map(&mut self) -> Vec<ParkingPortal> {
        self.expect(b'{');
        let mut out = Vec::new();
        self.skip_ws();
        while self.peek() != b'}' {
            let id = self.parse_string();
            self.skip_ws();
            self.expect(b':');
            out.push(self.parse_portal_node(id));
            self.skip_ws();
            if self.peek() == b',' {
                self.eat();
                self.skip_ws();
            }
        }
        self.eat();
        out
    }

    fn parse_city(&mut self) -> CityData {
        self.expect(b'{');
        let mut nodes: HashMap<String, GraphNode> = HashMap::new();
        let mut portals: Vec<ParkingPortal> = Vec::new();
        self.skip_ws();
        while self.peek() != b'}' {
            let key = self.parse_string();
            self.skip_ws();
            self.expect(b':');
            match key.as_str() {
                "intersections" | "endpoints" => {
                    for n in self.parse_node_map() {
                        nodes.insert(n.id.clone(), n);
                    }
                }
                "parking_portals" => {
                    for p in self.parse_portal_map() {
                        nodes.insert(p.node.id.clone(), p.node.clone());
                        portals.push(p);
                    }
                }
                _ => self.skip_value(),
            }
            self.skip_ws();
            if self.peek() == b',' {
                self.eat();
                self.skip_ws();
            }
        }
        self.eat();
        CityData { nodes, portals }
    }

    fn skip_value(&mut self) {
        self.skip_ws();
        match self.peek() {
            b'"' => {
                self.parse_string();
            }
            b'[' => {
                self.eat();
                self.skip_ws();
                while self.peek() != b']' {
                    self.skip_value();
                    self.skip_ws();
                    if self.peek() == b',' {
                        self.eat();
                        self.skip_ws();
                    }
                }
                self.eat();
            }
            b'{' => {
                self.eat();
                self.skip_ws();
                while self.peek() != b'}' {
                    self.parse_string();
                    self.skip_ws();
                    self.expect(b':');
                    self.skip_value();
                    self.skip_ws();
                    if self.peek() == b',' {
                        self.eat();
                        self.skip_ws();
                    }
                }
                self.eat();
            }
            _ => {
                while self.i < self.s.len()
                    && !matches!(
                        self.peek(),
                        b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t'
                    )
                {
                    self.eat();
                }
            }
        }
    }
}
