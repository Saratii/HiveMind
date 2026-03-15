/*
prologue
Name of program: map.rs
Description: Handles logic involving the map and interactions with the graph.
Author: Maren Proplesch
Date Created: 2/11/2026
Date Revised: 3/13/2026
Revision History: Included in the numerous sprint artifacts.
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
Citation: Used AI copilot for limited code generation - claude.ai
*/

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;

// Intermediate deserialization struct for a node as it appears in the raw city JSON
// x: horizontal position of the node in the city coordinate space
// y: vertical position of the node in the city coordinate space
// connects: list of node IDs that this node has a direct road connection to
#[derive(Debug, Deserialize, Clone)]
struct RawNode {
    pub x: f64,
    pub y: f64,
    pub connects: Vec<String>,
}

// Intermediate deserialization struct for a parking portal as it appears in the raw city JSON
// x: horizontal position of the portal in the city coordinate space
// y: vertical position of the portal in the city coordinate space
// connects: list of node IDs that this portal connects to, typically roadway entry and exit points
#[derive(Debug, Deserialize, Clone)]
struct RawPortal {
    pub x: f64,
    pub y: f64,
    pub connects: Vec<String>,
}

// Top-level deserialization struct that maps directly to the structure of the city JSON file
// intersections: all interior road intersection nodes keyed by their string ID
// endpoints: terminal nodes representing entry and exit points of the road network, keyed by their string ID
// parking_portals: special nodes representing parking lot connections to the road network, keyed by their string ID
#[derive(Debug, Deserialize)]
struct RawMap {
    pub intersections: HashMap<String, RawNode>,
    pub parking_portals: HashMap<String, RawPortal>,
}

// A single node in the built city graph, carrying both its position and its string identifier
// x: horizontal position of the node in the city coordinate space
// y: vertical position of the node in the city coordinate space
// id: the unique string identifier used to look this node up by name
#[derive(Debug, Clone)]
pub struct GraphNode {
    pub x: f64,
    pub y: f64,
    pub id: String,
}

// A directed edge in the city graph connecting two nodes with a precomputed Euclidean distance
// from: index into the nodes vector for the origin of this edge
// to: index into the nodes vector for the destination of this edge
// length: Euclidean distance between the two nodes, used as the edge weight during pathfinding
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub length: f64,
}

// The fully built directed graph of the city road network, ready for pathfinding
// nodes: ordered list of all nodes in the graph
// edges: list of all directed edges, with each undirected connection represented as two opposing edges
// adjacency: per-node list of edge indices into the edges vector, used to efficiently traverse neighbors
#[derive(Debug, Clone)]
pub struct CityGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub adjacency: Vec<Vec<usize>>,
}

// Intermediate representation of the city map after parsing but before graph construction
// nodes: ordered list of all nodes collected from all three categories in the raw map
// connections: deduplicated list of undirected node index pairs representing road connections
#[derive(Debug, Clone)]
pub struct CityMap {
    nodes: Vec<GraphNode>,
    connections: Vec<(usize, usize)>,
}

impl CityMap {
    // Reads and parses the city JSON file at the given path, assigns stable index-based IDs to all nodes, and collects deduplicated undirected connections across all node categories
    // Input: path: &str pointing to the city JSON file on disk
    // Returns: Result<CityMap, String> where the error string describes what went wrong during file reading or JSON parsing
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
        let mut sorted_keys: Vec<String> = raw.parking_portals.keys().cloned().collect();
        sorted_keys.sort();
        for k in &sorted_keys {
            let p = &raw.parking_portals[k];
            push(k.clone(), p.x, p.y);
        }
        let mut connection_set: HashSet<(usize, usize)> = HashSet::new();
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

    // Returns the total number of nodes loaded into this city map
    // Input: none
    // Returns: usize representing the count of all nodes across intersections, endpoints, and parking portals
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    // Converts the flat node and connection lists into a directed CityGraph by computing edge lengths and building a per-node adjacency index
    // Input: none
    // Returns: CityGraph with bidirectional edges and an adjacency list ready for pathfinding
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
    // Looks up a node's numeric index by its string ID, used to translate human-readable IDs into graph indices before pathfinding
    // Input: id: &str representing the node's string identifier
    // Returns: Option<usize> with the node's index in the nodes vector, or None if the ID is not found
    pub fn node_index(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pathfinding::compute_path;

    // Loads the city graph from the standard test fixture path
    // Input: none
    // Returns: CityGraph built from the city.json file, panicking if the file is missing or invalid
    fn load() -> CityGraph {
        CityMap::load("../city.json")
            .expect("city.json must be present")
            .build_graph()
    }

    // Verifies that the city graph loads successfully and contains a minimum viable number of nodes and edges
    // Input: none
    // Returns: none, panics if assertions fail
    #[test]
    fn test_graph_loads_and_has_nodes() {
        let g = load();
        assert!(g.nodes.len() >= 2, "graph must have at least 2 nodes");
        assert!(!g.edges.is_empty(), "graph must have edges");
    }

    // Confirms that a path can be computed between two parking portal exits, validating that portals are correctly integrated into the graph
    // Input: none
    // Returns: none, panics if no path is found or if the path endpoints do not match the expected node IDs
    #[test]
    fn test_parking_lot_exits_are_routable() {
        let g = load();
        let path =
            compute_path(&g, "PA", "PB").expect("should find path between parking lot exits");
        assert!(path.len() >= 2);
        assert_eq!(path.first().unwrap().node_id, "PA");
        assert_eq!(path.last().unwrap().node_id, "PB");
    }

    // Performs a breadth-first traversal from node 0 to confirm that every node in the graph is reachable, ensuring the road network has no isolated components
    // Input: none
    // Returns: none, panics if any nodes are unreachable from node 0
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
