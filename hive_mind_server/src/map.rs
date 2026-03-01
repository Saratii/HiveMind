/*
prologue
Name of program: map.rs
Description: Loads a city map from JSON and constructs a graph representation for pathfinding.
Author: Maren Proplesch
Date Created: 3/1/2026
Date Revised: 3/1/2026
Revision History: Included in the numerous sprint artifacts.
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
*/

use serde::Deserialize;
use std::fs;

pub const ENDPOINT_EPSILON: f64 = 1.0;

//struct for a line segment in the city map, defined by a list of points
#[derive(Debug, Deserialize, Clone)]
pub struct Segment {
    pub pts: Vec<[f64; 2]>,
}

//struct for the city map, containing a list of segments
#[derive(Debug, Clone)]
pub struct CityMap {
    pub segments: Vec<Segment>,
}

//struct for a graph node with coordinates
#[derive(Debug, Clone)]
pub struct GraphNode {
    pub x: f64,
    pub y: f64,
}

//struct for a graph edge connecting two nodes with a length
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub length: f64,
}

//struct for the city graph, containing nodes, edges, and an adjacency list
#[derive(Debug, Clone)]
pub struct CityGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub adjacency: Vec<Vec<usize>>,
}

impl CityMap {
    // Load a city map from a JSON file at the given path
    //inputs: file path to JSON map
    //returns Ok(CityMap) or Err(error message)
    pub fn load(path: &str) -> Result<Self, String> {
        let data = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read city map from '{}': {}", path, e))?;
        #[derive(Deserialize)]
        struct RawMap {
            segments: Vec<Segment>,
        }
        let raw: RawMap = serde_json::from_str(&data)
            .map_err(|e| format!("Failed to parse city map JSON: {}", e))?;
        Ok(CityMap {
            segments: raw.segments,
        })
    }

    // Get the total number of segments in the city map
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    // Build a graph representation of the city map for pathfinding
    //inputs: self (CityMap)
    //returns CityGraph with nodes, edges, and adjacency list
    pub fn build_graph(&self) -> CityGraph {
        let mut nodes: Vec<GraphNode> = Vec::new();
        let mut edges: Vec<GraphEdge> = Vec::new();
        let find_or_add = |nodes: &mut Vec<GraphNode>, x: f64, y: f64| -> usize {
            for (i, n) in nodes.iter().enumerate() {
                if (n.x - x).hypot(n.y - y) < ENDPOINT_EPSILON {
                    return i;
                }
            }
            nodes.push(GraphNode { x, y });
            nodes.len() - 1
        };
        for seg in &self.segments {
            if seg.pts.len() < 2 {
                continue;
            }
            for window in seg.pts.windows(2) {
                let [x0, y0] = window[0];
                let [x1, y1] = window[1];
                let from = find_or_add(&mut nodes, x0, y0);
                let to = find_or_add(&mut nodes, x1, y1);
                let length = (x1 - x0).hypot(y1 - y0);
                edges.push(GraphEdge { from, to, length });
                edges.push(GraphEdge {
                    from: to,
                    to: from,
                    length,
                });
            }
        }
        let mut adjacency = vec![Vec::new(); nodes.len()];
        for (i, edge) in edges.iter().enumerate() {
            adjacency[edge.from].push(i);
        }
        CityGraph {
            nodes,
            edges,
            adjacency,
        }
    }
}

impl CityGraph {
    // Find the index of the nearest graph node to the given coordinates
    //inputs: x and y coordinates
    //returns index of nearest node in the graph
    pub fn nearest_node(&self, x: f64, y: f64) -> usize {
        self.nodes
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (a.x - x)
                    .hypot(a.y - y)
                    .partial_cmp(&(b.x - x).hypot(b.y - y))
                    .unwrap()
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
}
