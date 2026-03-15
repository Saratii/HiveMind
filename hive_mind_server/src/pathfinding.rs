/*
prologue
Name of program: pathfinding.rs
Description: Handles logic for path finding and traversing the city graph.
Author: Maren Proplesch
Date Created: 2/11/2026
Date Revised: 3/13/2026
Revision History: Included in the numerous sprint artifacts.
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
*/

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::map::CityGraph;

// A single stop along a computed route, identified by the node's string ID
// node_id: the string identifier of the graph node at this position in the path
#[derive(Debug, Clone)]
pub struct Waypoint {
    pub node_id: String,
}

// A priority queue entry used internally by Dijkstra's algorithm to track the cheapest known cost to reach a node
// cost: the total accumulated edge length to reach this node from the start
// node: the index of the node in the graph's nodes vector
#[derive(Copy, Clone)]
struct State {
    cost: f64,
    node: usize,
}

// Orders State entries in reverse so that the BinaryHeap behaves as a min-heap on cost
impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
}
impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}
impl Eq for State {}

// Runs Dijkstra's shortest path algorithm on the city graph from a start node to a goal node, returning the sequence of node indices that form the optimal route
// Input: graph: &CityGraph containing the nodes, edges, and adjacency list; start: usize index of the origin node; goal: usize index of the destination node
// Returns: Option<Vec<usize>> with the ordered list of node indices along the shortest path, or None if the goal is unreachable from the start
fn dijkstra(graph: &CityGraph, start: usize, goal: usize) -> Option<Vec<usize>> {
    let n = graph.nodes.len();
    let mut dist = vec![f64::INFINITY; n];
    let mut prev = vec![usize::MAX; n];
    let mut heap = BinaryHeap::new();
    dist[start] = 0.0;
    heap.push(State {
        cost: 0.0,
        node: start,
    });
    while let Some(State { cost, node }) = heap.pop() {
        if node == goal {
            break;
        }
        if cost > dist[node] {
            continue;
        }
        for &edge_idx in &graph.adjacency[node] {
            let edge = &graph.edges[edge_idx];
            let next_cost = cost + edge.length;
            if next_cost < dist[edge.to] {
                dist[edge.to] = next_cost;
                prev[edge.to] = node;
                heap.push(State {
                    cost: next_cost,
                    node: edge.to,
                });
            }
        }
    }
    if dist[goal] == f64::INFINITY {
        return None;
    }
    let mut path = Vec::new();
    let mut cur = goal;
    while cur != usize::MAX {
        path.push(cur);
        cur = prev[cur];
    }
    path.reverse();
    Some(path)
}

// Resolves string node IDs to graph indices, runs Dijkstra's algorithm, and converts the resulting index path into an ordered list of named Waypoints
// Input: graph: &CityGraph to search; start_id: &str string identifier of the origin node; dest_id: &str string identifier of the destination node
// Returns: Option<Vec<Waypoint>> with the ordered waypoints from start to destination, or None if either ID is not found or no path exists between them
pub fn compute_path(graph: &CityGraph, start_id: &str, dest_id: &str) -> Option<Vec<Waypoint>> {
    let start_node = graph.node_index(start_id)?;
    let goal_node = graph.node_index(dest_id)?;
    let node_path = dijkstra(graph, start_node, goal_node)?;
    if node_path.len() < 2 {
        return None;
    }
    Some(
        node_path
            .into_iter()
            .map(|idx| Waypoint {
                node_id: graph.nodes[idx].id.clone(),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{CityGraph, GraphEdge, GraphNode};

    // Constructs a small four-node test graph with bidirectional edges for use across the pathfinding unit tests
    // Input: none
    // Returns: CityGraph with nodes N00 through N03 arranged in a simple branching layout
    fn build_test_graph() -> CityGraph {
        let nodes = vec![
            GraphNode {
                id: "N00".to_string(),
                x: 0.0,
                y: 0.0,
            },
            GraphNode {
                id: "N01".to_string(),
                x: 100.0,
                y: 0.0,
            },
            GraphNode {
                id: "N02".to_string(),
                x: 200.0,
                y: 0.0,
            },
            GraphNode {
                id: "N03".to_string(),
                x: 100.0,
                y: -100.0,
            },
        ];
        let edges = vec![
            GraphEdge {
                from: 0,
                to: 1,
                length: 100.0,
            },
            GraphEdge {
                from: 1,
                to: 0,
                length: 100.0,
            },
            GraphEdge {
                from: 1,
                to: 2,
                length: 100.0,
            },
            GraphEdge {
                from: 2,
                to: 1,
                length: 100.0,
            },
            GraphEdge {
                from: 1,
                to: 3,
                length: 100.0,
            },
            GraphEdge {
                from: 3,
                to: 1,
                length: 100.0,
            },
        ];
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

    // Verifies that a path through two intermediate hops is computed correctly and that all three waypoints appear in the right order
    // Input: none
    // Returns: none, panics if the path length or node IDs do not match expectations
    #[test]
    fn test_direct_path() {
        let graph = build_test_graph();
        let path = compute_path(&graph, "N00", "N02").expect("should find path");
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].node_id, "N00");
        assert_eq!(path[1].node_id, "N01");
        assert_eq!(path[2].node_id, "N02");
    }

    // Verifies that the pathfinder correctly routes through a shared intermediate node when the destination branches off the main corridor
    // Input: none
    // Returns: none, panics if the path length or node IDs do not match expectations
    #[test]
    fn test_path_with_turn() {
        let graph = build_test_graph();
        let path = compute_path(&graph, "N00", "N03").expect("should find path");
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].node_id, "N00");
        assert_eq!(path[1].node_id, "N01");
        assert_eq!(path[2].node_id, "N03");
    }

    // Confirms that compute_path returns None when the destination node exists in the graph but has no edges connecting it to the start
    // Input: none
    // Returns: none, panics if a path is unexpectedly returned
    #[test]
    fn test_unreachable_returns_none() {
        let graph = CityGraph {
            nodes: vec![
                GraphNode {
                    id: "N00".to_string(),
                    x: 0.0,
                    y: 0.0,
                },
                GraphNode {
                    id: "N01".to_string(),
                    x: 100.0,
                    y: 0.0,
                },
            ],
            edges: vec![],
            adjacency: vec![vec![], vec![]],
        };
        assert!(compute_path(&graph, "N00", "N01").is_none());
    }

    // Confirms that compute_path returns None when the destination ID does not exist anywhere in the graph
    // Input: none
    // Returns: none, panics if a path is unexpectedly returned
    #[test]
    fn test_unknown_id_returns_none() {
        let graph = build_test_graph();
        assert!(compute_path(&graph, "N00", "ZZZZ").is_none());
    }
}
