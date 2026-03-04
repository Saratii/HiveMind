/*
prologue
Name of program: HiveMind City Map Loader
Description: Loads the city layout from JSON and builds a graph used for pathfinding in the HiveMind simulation.
Author: Maren Proplesch
Date Created: 3/1/2026
Date Revised: 3/2/2026
Revision History: Included in the numerous sprint artifacts.
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
*/
// Loads city map from JSON and builds graph for pathfinding.

use serde::Deserialize;
use std::fs;

const NODE_EPSILON: f64 = 1.0;

// Defines the Segment struct, which contains the points of the segment
#[derive(Debug, Deserialize, Clone)]
pub struct Segment {
    pub pts: Vec<[f64; 2]>,
}

// Defines the ParkingLotConfig struct, which contains the spawn and exit points of the parking lot
#[derive(Debug, Deserialize, Clone)]
pub struct ParkingLotConfig {
    #[serde(alias = "center")]
    pub spawn: [f64; 2],
    pub exit: [f64; 2],
}

// Defines the ParkingLotSpawns type, which is a hashmap of parking lot ids and their configurations
pub type ParkingLotSpawns = std::collections::HashMap<String, ParkingLotConfig>;

// Defines the GraphNode struct, which contains the x and y coordinates of the node
#[derive(Debug, Clone)]
pub struct GraphNode {
    pub x: f64,
    pub y: f64,
}

// Defines the GraphEdge struct, which contains the from and to nodes and the length of the edge
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub length: f64,
}

// Defines the CityGraph struct, which contains the nodes, edges, and adjacency list
#[derive(Debug, Clone)]
pub struct CityGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub adjacency: Vec<Vec<usize>>,
}

// Defines the CityMap struct, which contains the segments and parking spawns
#[derive(Debug, Clone)]
pub struct CityMap {
    pub segments: Vec<Segment>,
    pub parking_spawns: ParkingLotSpawns,
}

// Implements the CityMap struct, which contains the segments and parking spawns
impl CityMap {
    // Loads the city map from the given path
    pub fn load(path: &str) -> Result<Self, String> {
        // Reads the city map from the given path
        let data = fs::read_to_string(path)
            // If the read fails, return an error
            .map_err(|e| format!("Failed to read city map: {}", e))?;
        // Converts the data to a JSON value
        let v: serde_json::Value = serde_json::from_str(&data)
            // If the conversion fails, return an error
            .map_err(|e| format!("Invalid JSON: {}", e))?;
        // Converts the JSON value to a vector of segments
        let segments: Vec<Segment> = serde_json::from_value(
            v.get("segments").cloned().unwrap_or(serde_json::Value::Array(vec![])),
        )
        .map_err(|e| format!("Invalid segments: {}", e))?;
        // Initializes the parking spawns hashmap
        let mut parking_spawns = ParkingLotSpawns::new();
        if let Some(pl) = v.get("parking_lots").and_then(|x| x.as_object()) {
            // For each parking lot, get the id and object
            for (id, obj) in pl {
                // If the center and exit are some, get the center and exit points
                if let (Some(center), Some(exit)) = (
                    obj.get("center").and_then(|c| c.as_array()).filter(|a| a.len() >= 2),
                    obj.get("exit").and_then(|e| e.as_array()).filter(|a| a.len() >= 2),
                ) {
                    // Converts the center and exit points to arrays of f64
                    let spawn = [
                        center[0].as_f64().unwrap_or(0.0),
                        center[1].as_f64().unwrap_or(0.0),
                    ];
                    // Converts the exit points to arrays of f64
                    let exit_pt = [
                        exit[0].as_f64().unwrap_or(0.0),
                        exit[1].as_f64().unwrap_or(0.0),
                    ];
                    // Inserts the parking lot config into the parking spawns hashmap
                    parking_spawns.insert(id.clone(), ParkingLotConfig { spawn, exit: exit_pt });
                }
            }
        }
        // Returns the city map
        Ok(CityMap {
            segments,
            parking_spawns,
        })
    }

    // Builds the graph for the city map
    pub fn build_graph(&self) -> CityGraph {
        // Initializes the nodes and edges vectors
        let mut nodes: Vec<GraphNode> = Vec::new();
        let mut edges: Vec<GraphEdge> = Vec::new();
        // Defines the find_or_add function, which finds the nearest node to the given x and y coordinates
        let find_or_add = |nodes: &mut Vec<GraphNode>, x: f64, y: f64| -> usize {
            // For each node, check if the distance between the node and the given x and y coordinates is less than the node epsilon
            for (i, n) in nodes.iter().enumerate() {
                // If the distance is less than the node epsilon, return the index of the node
                if (n.x - x).hypot(n.y - y) < NODE_EPSILON {
                    return i;
                }
            }
            // If the distance is greater than the node epsilon, add the node to the nodes vector and return the index of the node
            nodes.push(GraphNode { x, y });
            nodes.len() - 1
        };
        // For each segment, get the points and add the edges to the edges vector
        for seg in &self.segments {
            // For each window in the segment, get the points and add the edges to the edges vector
            for window in seg.pts.windows(2) {
                // Gets the points from the window
                let [x0, y0] = window[0];
                let [x1, y1] = window[1];
                // Finds the nearest node to the given x and y coordinates
                let from = find_or_add(&mut nodes, x0, y0);
                // Finds the nearest node to the given x and y coordinates
                let to = find_or_add(&mut nodes, x1, y1);
                // Calculates the length of the edge
                let length = (x1 - x0).hypot(y1 - y0);
                // Adds the edge to the edges vector
                edges.push(GraphEdge { from, to, length });
                // Adds the reverse edge to the edges vector
                edges.push(GraphEdge {
                    from: to,
                    to: from,
                    length,
                });
            }
        }
        // Initializes the adjacency vector
        let mut adjacency = vec![Vec::new(); nodes.len()];
        // For each edge, add the edge to the adjacency vector
        for (i, edge) in edges.iter().enumerate() {
            // Adds the edge to the adjacency vector
            adjacency[edge.from].push(i);
        }
        // Returns the city graph
        CityGraph {
            nodes,
            edges,
            adjacency,
        }
    }
}

// Implements the CityGraph struct, which contains the nodes, edges, and adjacency list
impl CityGraph {
    // Finds the nearest node to the given x and y coordinates
    pub fn nearest_node(&self, x: f64, y: f64) -> usize {
        // Finds the nearest node to the given x and y coordinates
        self.nodes
            .iter()
            .enumerate()
            // Finds the node with the smallest distance to the given x and y coordinates
            .min_by(|(_, a), (_, b)| {
                // Calculates the distance between the node and the given x and y coordinates
                (a.x - x)
                    .hypot(a.y - y)
                    // Compares the distance between the node and the given x and y coordinates
                    .partial_cmp(&(b.x - x).hypot(b.y - y))
                    .unwrap()
            })
            // Returns the index of the node with the smallest distance to the given x and y coordinates
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
}
