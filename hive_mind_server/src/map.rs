// Loads city map from JSON and builds graph for pathfinding.

use serde::Deserialize;
use std::fs;

const NODE_EPSILON: f64 = 1.0;

#[derive(Debug, Deserialize, Clone)]
pub struct Segment {
    pub pts: Vec<[f64; 2]>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ParkingLotConfig {
    #[serde(alias = "center")]
    pub spawn: [f64; 2],
    pub exit: [f64; 2],
}

pub type ParkingLotSpawns = std::collections::HashMap<String, ParkingLotConfig>;

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub length: f64,
}

#[derive(Debug, Clone)]
pub struct CityGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub adjacency: Vec<Vec<usize>>,
}

#[derive(Debug, Clone)]
pub struct CityMap {
    pub segments: Vec<Segment>,
    pub parking_spawns: ParkingLotSpawns,
}

impl CityMap {
    pub fn load(path: &str) -> Result<Self, String> {
        let data = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read city map: {}", e))?;
        let v: serde_json::Value = serde_json::from_str(&data)
            .map_err(|e| format!("Invalid JSON: {}", e))?;

        let segments: Vec<Segment> = serde_json::from_value(
            v.get("segments").cloned().unwrap_or(serde_json::Value::Array(vec![])),
        )
        .map_err(|e| format!("Invalid segments: {}", e))?;

        let mut parking_spawns = ParkingLotSpawns::new();
        if let Some(pl) = v.get("parking_lots").and_then(|x| x.as_object()) {
            for (id, obj) in pl {
                if let (Some(center), Some(exit)) = (
                    obj.get("center").and_then(|c| c.as_array()).filter(|a| a.len() >= 2),
                    obj.get("exit").and_then(|e| e.as_array()).filter(|a| a.len() >= 2),
                ) {
                    let spawn = [
                        center[0].as_f64().unwrap_or(0.0),
                        center[1].as_f64().unwrap_or(0.0),
                    ];
                    let exit_pt = [
                        exit[0].as_f64().unwrap_or(0.0),
                        exit[1].as_f64().unwrap_or(0.0),
                    ];
                    parking_spawns.insert(id.clone(), ParkingLotConfig { spawn, exit: exit_pt });
                }
            }
        }

        Ok(CityMap {
            segments,
            parking_spawns,
        })
    }

    pub fn build_graph(&self) -> CityGraph {
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

        for seg in &self.segments {
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
