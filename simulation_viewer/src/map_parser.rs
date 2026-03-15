/*
prologue
Name of program: map_parser.rs
Description: Defines structs for holding the graph and map. Parses the map json into render elements.
Author: Maren Proplesch
Date Created: 3/13/2026
Date Revised: 3/13/2026
Revision History: None
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
Citation: Used AI copilot for limited code generation - claude.ai
*/

use bevy::prelude::*;
use std::collections::HashMap;

// Represents a parking lot portal, pairing its road entry node with the portal's visual center position
// node: the GraphNode at the road entry point that cars use when entering or exiting this portal
// center: the [x, z] world position of the parking lot's center, used as the car's spawn and despawn location
#[derive(Debug, Clone)]
pub struct ParkingPortal {
    pub node: GraphNode,
    pub center: [f32; 2],
}

// The fully parsed city data used throughout the renderer and car systems
// nodes: flat map of all graph nodes keyed by their string ID, including intersections, endpoints, and portal entry nodes
// portals: ordered list of all parking portals, preserving the order they appear in the city JSON
#[derive(Debug)]
pub struct CityData {
    pub nodes: HashMap<String, GraphNode>,
    pub portals: Vec<ParkingPortal>,
}

impl CityData {
    // Looks up a node's world position by its string ID, returning the origin if the ID is not found
    // Input: id: &str the string identifier of the node to look up
    // Returns: (f32, f32) tuple of the node's x and y world coordinates, or (0.0, 0.0) if the ID is missing
    pub fn node_pos(&self, id: &str) -> (f32, f32) {
        let node = self
            .nodes
            .get(id)
            .unwrap_or_else(|| panic!("node '{}' not found", id));
        (node.x, node.y)
    }
}

// A resolved waypoint along a car's assigned route, carrying both its node identifier and its world coordinates
// node_id: the string identifier of the graph node at this waypoint
// x: world X coordinate of this waypoint, looked up from CityData at parse time
// z: world Z coordinate of this waypoint, looked up from CityData at parse time
#[derive(Debug, Clone)]
pub struct Waypoint {
    pub node_id: String,
    pub x: f32,
    pub z: f32,
}

// ECS component attached to portal pad entities so click events can identify which portal was clicked
// portal_index: index into CityData's portals vector for the portal this entity represents
#[derive(Component)]
pub struct PortalMarker {
    pub portal_index: usize,
}

// A node in the city road graph as used by the renderer, holding position, ID, and adjacency information
// id: unique string identifier for this node, matching the keys used in the city JSON
// x: world X coordinate of this node
// y: world Z coordinate of this node, named y to match the JSON field but used as the Z axis in 3D space
// connects: list of neighboring node IDs that this node has a direct road connection to
#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub connects: Vec<String>,
}

// Reads a waypoint list from a parsed form parameter map and resolves each node ID to its world coordinates using the city data
// Input: params: &HashMap<String, String> containing wp_count and wp0..wpN entries from a server response; city: &CityData for coordinate lookup
// Returns: Vec<Waypoint> with one entry per waypoint in order, skipping any whose ID cannot be found in the params map
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

// Entry point for parsing a city JSON string into a CityData value by constructing a Parser and running the top-level parse
// Input: src: &str containing the full city JSON text
// Returns: CityData populated with all nodes and portals from the JSON
pub fn parse_city(src: &str) -> CityData {
    let mut p = Parser {
        s: src.as_bytes(),
        i: 0,
    };
    p.parse_city()
}

// A zero-copy byte-level JSON parser that walks the city JSON without allocating intermediate representations
// s: the source JSON as a byte slice
// i: the current read position within the byte slice
struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    // Returns the byte at the current position without advancing the cursor
    // Input: none
    // Returns: u8 the byte at position i
    fn peek(&self) -> u8 {
        self.s[self.i]
    }

    // Advances the cursor by one byte
    // Input: none
    // Returns: none
    fn eat(&mut self) {
        self.i += 1;
    }

    // Advances the cursor past any whitespace characters including spaces, tabs, and newlines
    // Input: none
    // Returns: none
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && matches!(self.peek(), b' ' | b'\t' | b'\r' | b'\n') {
            self.eat();
        }
    }

    // Skips whitespace then asserts that the current byte matches the expected value and advances past it, panicking if it does not
    // Input: b: u8 the byte value that must appear at the current position
    // Returns: none
    fn expect(&mut self, b: u8) {
        self.skip_ws();
        assert_eq!(self.peek(), b, "expected '{}' at {}", b as char, self.i);
        self.eat();
    }

    // Parses a JSON string value including escape sequence handling and returns it as an owned String
    // Input: none
    // Returns: String containing the unquoted and unescaped contents of the JSON string
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

    // Parses a JSON number value, including optional leading minus and decimal or exponent notation, into an f32
    // Input: none
    // Returns: f32 parsed from the bytes at the current position
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

    // Parses a JSON array of quoted strings and returns them as an owned Vec
    // Input: none
    // Returns: Vec<String> containing each string element from the JSON array in order
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

    // Parses a JSON two-element numeric array into a fixed-size [f32; 2] array
    // Input: none
    // Returns: [f32; 2] with the first element at index 0 and the second at index 1
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

    // Parses a JSON object representing an intersection or endpoint node, reading x, y, and connects fields and ignoring any unrecognized keys
    // Input: id: String the already-parsed node ID to assign to this node
    // Returns: GraphNode populated with the parsed coordinate and adjacency data
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

    // Parses a JSON object representing a parking portal, reading x, y, center, and connects fields and ignoring any unrecognized keys
    // Input: id: String the already-parsed portal ID to assign to this portal's node
    // Returns: ParkingPortal containing the road entry GraphNode and the parking lot center coordinates
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

    // Parses a JSON object whose values are plain nodes, returning all of them as an ordered Vec
    // Input: none
    // Returns: Vec<GraphNode> with one entry per key-value pair in the JSON object
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

    // Parses a JSON object whose values are parking portal objects, returning all of them as an ordered Vec
    // Input: none
    // Returns: Vec<ParkingPortal> with one entry per key-value pair in the JSON object
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

    // Parses the top-level city JSON object, routing intersections and endpoints into the nodes map and parking portals into both the nodes map and the portals list
    // Input: none
    // Returns: CityData containing all parsed nodes and portals
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
                "intersections" => {
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

    // Skips over an arbitrary JSON value of any type, including strings, arrays, objects, and bare primitives, without retaining any of the data
    // Input: none
    // Returns: none
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
