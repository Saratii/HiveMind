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
const INTERSECT_EPSILON: f64 = 1e-6;

// Defines the Segment struct, which contains the points of the segment
#[derive(Debug, Deserialize, Clone)]
pub struct Segment {
    pub pts: Vec<[f64; 2]>,
}

// Parking lot: center (spawn), rectangular size, and road entrance (point on road segment).
#[derive(Debug, Deserialize, Clone)]
pub struct ParkingLotConfig {
    #[serde(alias = "center")]
    pub spawn: [f64; 2],
    #[serde(default = "default_lot_size")]
    pub size: [f64; 2],
    /// Point on the road where the lot meets the roadway (car enters/exits here).
    #[serde(alias = "exit")]
    pub entrance: [f64; 2],
}

fn default_lot_size() -> [f64; 2] {
    [40.0, 40.0]
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
            for (id, obj) in pl {
                let entrance_arr = obj.get("entrance").or_else(|| obj.get("exit")).and_then(|e| e.as_array()).filter(|a| a.len() >= 2);
                if let (Some(center), Some(entrance)) = (
                    obj.get("center").and_then(|c| c.as_array()).filter(|a| a.len() >= 2),
                    entrance_arr,
                ) {
                    let spawn = [
                        center[0].as_f64().unwrap_or(0.0),
                        center[1].as_f64().unwrap_or(0.0),
                    ];
                    let entrance_pt = [
                        entrance[0].as_f64().unwrap_or(0.0),
                        entrance[1].as_f64().unwrap_or(0.0),
                    ];
                    let size = obj.get("size").and_then(|s| s.as_array()).filter(|a| a.len() >= 2)
                        .map(|a| [a[0].as_f64().unwrap_or(40.0), a[1].as_f64().unwrap_or(40.0)])
                        .unwrap_or([40.0, 40.0]);
                    parking_spawns.insert(id.clone(), ParkingLotConfig { spawn, size, entrance: entrance_pt });
                }
            }
        }
        // Returns the city map
        Ok(CityMap {
            segments,
            parking_spawns,
        })
    }

    // Builds the graph for the city map, splitting segments at intersections so the graph is connected.
    pub fn build_graph(&self) -> CityGraph {
        // Expand each segment with intersection points from other segments
        let expanded: Vec<Vec<[f64; 2]>> = self
            .segments
            .iter()
            .map(|seg| Self::expand_segment_with_intersections(seg, &self.segments))
            .collect();

        let mut nodes: Vec<GraphNode> = Vec::new();
        let mut edges: Vec<GraphEdge> = Vec::new();
        let find_or_add = |nodes: &mut Vec<GraphNode>, x: f64, y: f64| -> usize {
            for (i, n) in nodes.iter().enumerate() {
                if (n.x - x).hypot(n.y - y) < NODE_EPSILON {
                    return i;
                }
            }
            nodes.push(GraphNode { x, y });
            nodes.len() - 1
        };
        for pts in &expanded {
            for window in pts.windows(2) {
                let [x0, y0] = window[0];
                let [x1, y1] = window[1];
                let from = find_or_add(&mut nodes, x0, y0);
                let to = find_or_add(&mut nodes, x1, y1);
                // Skip zero-length edges (e.g. duplicate points in L-shaped segments like F's spur)
                if from == to {
                    continue;
                }
                let length = (x1 - x0).hypot(y1 - y0);
                if length < 1e-9 {
                    continue;
                }
                edges.push(GraphEdge { from, to, length });
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

    /// Intersection of two line segments (a0,a1) and (b0,b1). Returns the point if they cross
    /// (including at endpoints); parallel or non-crossing returns None.
    fn segment_intersect(
        a0: [f64; 2],
        a1: [f64; 2],
        b0: [f64; 2],
        b1: [f64; 2],
    ) -> Option<[f64; 2]> {
        let dxa = a1[0] - a0[0];
        let dya = a1[1] - a0[1];
        let dxb = b1[0] - b0[0];
        let dyb = b1[1] - b0[1];
        let denom = dxa * dyb - dya * dxb;
        if denom.abs() < INTERSECT_EPSILON {
            return None;
        }
        let t = ((b0[0] - a0[0]) * dyb - (b0[1] - a0[1]) * dxb) / denom;
        let s = ((b0[0] - a0[0]) * dya - (b0[1] - a0[1]) * dxa) / denom;
        if t >= -INTERSECT_EPSILON && t <= 1.0 + INTERSECT_EPSILON
            && s >= -INTERSECT_EPSILON && s <= 1.0 + INTERSECT_EPSILON
        {
            Some([a0[0] + t * dxa, a0[1] + t * dya])
        } else {
            None
        }
    }

    /// Expand one segment's point list by inserting intersection points with all other segments,
    /// so that crossing segments share a node and the graph is connected.
    fn expand_segment_with_intersections(seg: &Segment, all_segments: &[Segment]) -> Vec<[f64; 2]> {
        let mut result = Vec::new();
        for (edge_idx, window) in seg.pts.windows(2).enumerate() {
            let a0 = window[0];
            let a1 = window[1];
            let mut splits: Vec<(f64, [f64; 2])> = Vec::new();
            for other in all_segments {
                if std::ptr::eq(other, seg) {
                    continue;
                }
                for ow in other.pts.windows(2) {
                    if let Some(pt) = Self::segment_intersect(a0, a1, ow[0], ow[1]) {
                        let d = (pt[0] - a0[0]).hypot(pt[1] - a0[1]);
                        splits.push((d, pt));
                    }
                }
            }
            splits.sort_by(|u, v| u.0.partial_cmp(&v.0).unwrap_or(std::cmp::Ordering::Equal));
            if edge_idx == 0 {
                result.push(a0);
            }
            for (_d, pt) in splits {
                result.push(pt);
            }
            result.push(a1);
        }
        result
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
