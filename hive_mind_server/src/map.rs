/*
prologue
Name of program: map.rs
Description: Loads a city map from the node-based JSON format and constructs a graph
             representation for pathfinding.
Author: Maren Proplesch
Date Created: 3/1/2026
Date Revised: 3/13/2026
*/

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
struct RawNode {
    pub x: f64,
    pub y: f64,
    pub connects: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawPortal {
    pub x: f64,
    pub y: f64,
    pub connects: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawMap {
    pub intersections: HashMap<String, RawNode>,
    pub endpoints: HashMap<String, RawNode>,
    pub parking_portals: HashMap<String, RawPortal>,
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub x: f64,
    pub y: f64,
    pub id: String,
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
    nodes: Vec<GraphNode>,
    connections: Vec<(usize, usize)>,
}

impl CityMap {
    pub fn load(path: &str) -> Result<Self, String> {
        let data = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read city map '{}': {}", path, e))?;
        let raw: RawMap = serde_json::from_str(&data)
            .map_err(|e| format!("Failed to parse city map JSON: {}", e))?;
        let mut id_to_index: HashMap<String, usize> = HashMap::new();
        let mut nodes: Vec<GraphNode> = Vec::new();
        let mut push = |id: String, x: f64, y: f64| {
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(GraphNode { x, y, id });
        };
        let mut sorted_keys: Vec<String> = raw.intersections.keys().cloned().collect();
        sorted_keys.sort();
        for k in &sorted_keys {
            let n = &raw.intersections[k];
            push(k.clone(), n.x, n.y);
        }
        let mut sorted_keys: Vec<String> = raw.endpoints.keys().cloned().collect();
        sorted_keys.sort();
        for k in &sorted_keys {
            let n = &raw.endpoints[k];
            push(k.clone(), n.x, n.y);
        }
        let mut sorted_keys: Vec<String> = raw.parking_portals.keys().cloned().collect();
        sorted_keys.sort();
        for k in &sorted_keys {
            let p = &raw.parking_portals[k];
            push(k.clone(), p.x, p.y);
        }
        let mut connection_set: std::collections::HashSet<(usize, usize)> =
            std::collections::HashSet::new();
        let collect_connects =
            |id: &str, connects: &[String], id_to_index: &HashMap<String, usize>| {
                let from = match id_to_index.get(id) {
                    Some(&i) => i,
                    None => return Vec::new(),
                };
                connects
                    .iter()
                    .filter_map(|nb_id| {
                        id_to_index.get(nb_id).map(
                            |&to| {
                                if from < to { (from, to) } else { (to, from) }
                            },
                        )
                    })
                    .collect::<Vec<_>>()
            };
        for (id, node) in &raw.intersections {
            for pair in collect_connects(id, &node.connects, &id_to_index) {
                connection_set.insert(pair);
            }
        }
        for (id, node) in &raw.endpoints {
            for pair in collect_connects(id, &node.connects, &id_to_index) {
                connection_set.insert(pair);
            }
        }
        for (id, portal) in &raw.parking_portals {
            for pair in collect_connects(id, &portal.connects, &id_to_index) {
                connection_set.insert(pair);
            }
        }
        let connections: Vec<(usize, usize)> = {
            let mut v: Vec<_> = connection_set.into_iter().collect();
            v.sort();
            v
        };

        Ok(CityMap { nodes, connections })
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn build_graph(&self) -> CityGraph {
        let n = self.nodes.len();
        let mut edges: Vec<GraphEdge> = Vec::new();
        for &(from, to) in &self.connections {
            let a = &self.nodes[from];
            let b = &self.nodes[to];
            let length = (b.x - a.x).hypot(b.y - a.y);
            edges.push(GraphEdge { from, to, length });
            edges.push(GraphEdge {
                from: to,
                to: from,
                length,
            });
        }
        let mut adjacency = vec![Vec::new(); n];
        for (i, edge) in edges.iter().enumerate() {
            adjacency[edge.from].push(i);
        }
        CityGraph {
            nodes: self.nodes.clone(),
            edges,
            adjacency,
        }
    }
}

impl CityGraph {
    pub fn node_index(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pathfinding::compute_path;

    fn load() -> CityGraph {
        CityMap::load("../city.json")
            .expect("city.json must be present")
            .build_graph()
    }

    #[test]
    fn test_graph_loads_and_has_nodes() {
        let g = load();
        assert!(g.nodes.len() >= 2, "graph must have at least 2 nodes");
        assert!(!g.edges.is_empty(), "graph must have edges");
    }

    #[test]
    fn test_parking_lot_exits_are_routable() {
        let g = load();
        let path =
            compute_path(&g, "PA", "PB").expect("should find path between parking lot exits");
        assert!(path.len() >= 2);
        assert_eq!(path.first().unwrap().node_id, "PA");
        assert_eq!(path.last().unwrap().node_id, "PB");
    }

    #[test]
    fn test_graph_is_connected() {
        use std::collections::{HashSet, VecDeque};
        let g = load();
        let mut seen = HashSet::new();
        let mut q = VecDeque::new();
        q.push_back(0usize);
        seen.insert(0usize);
        while let Some(u) = q.pop_front() {
            for &ei in &g.adjacency[u] {
                let v = g.edges[ei].to;
                if seen.insert(v) {
                    q.push_back(v);
                }
            }
        }
        assert_eq!(
            seen.len(),
            g.nodes.len(),
            "graph must be fully connected; only {}/{} nodes reachable from node 0",
            seen.len(),
            g.nodes.len()
        );
    }
}
