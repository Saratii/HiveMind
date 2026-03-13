/*
prologue
Name of program: pathfinding.rs
Description: Implements Dijkstra's algorithm to compute a path of waypoints from a start point to a destination point on the city graph.
Author: Maren Proplesch
Date Created: 3/1/2026
Date Revised: 3/13/2026
Revision History: None
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
*/

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::map::CityGraph;

#[derive(Debug, Clone)]
pub struct Waypoint {
    pub node_id: String,
}

#[derive(Copy, Clone)]
struct State {
    cost: f64,
    node: usize,
}

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

    #[test]
    fn test_direct_path() {
        let graph = build_test_graph();
        let path = compute_path(&graph, "N00", "N02").expect("should find path");
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].node_id, "N00");
        assert_eq!(path[1].node_id, "N01");
        assert_eq!(path[2].node_id, "N02");
    }

    #[test]
    fn test_path_with_turn() {
        let graph = build_test_graph();
        let path = compute_path(&graph, "N00", "N03").expect("should find path");
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].node_id, "N00");
        assert_eq!(path[1].node_id, "N01");
        assert_eq!(path[2].node_id, "N03");
    }

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

    #[test]
    fn test_unknown_id_returns_none() {
        let graph = build_test_graph();
        assert!(compute_path(&graph, "N00", "ZZZZ").is_none());
    }
}
