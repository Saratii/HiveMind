/*
prologue
Name of program: pathfinding.rs
Description: Implements Dijkstra’s algorithm to compute a path of waypoints from a start point to a destination point on the city graph.
Author: Maren Proplesch
Date Created: 3/1/2026
Date Revised: 3/1/2026
Revision History: None
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
*/

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::Point;
use crate::map::CityGraph;

//struct holding a waypoint in the path, with direction vector and distance to next waypoint
#[derive(Debug, Clone)]
pub struct Waypoint {
    pub x: f64,
    pub y: f64,
    pub dir_x: f64,
    pub dir_y: f64,
    pub dist_to_next: f64,
}

//struct for Dijkstra's algorithm state in the priority queue
#[derive(Copy, Clone)]
struct State {
    cost: f64,
    node: usize,
}

// Implement ordering for State so that the BinaryHeap becomes a min-heap based on cost
impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
}

// Implement PartialOrd, PartialEq, and Eq for State to satisfy BinaryHeap requirements
impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Two states are equal if they refer to the same node (cost is not considered for equality)
impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

// We only care about node equality for the priority queue, so we can ignore cost in Eq
impl Eq for State {}

// Dijkstra's algorithm to find the shortest path from start node to goal node in the graph
//inputs: graph, start node index, goal node index
//returns Some(vec of node indices in path) or None if no path exists
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

// Compute a path of waypoints from start to dest using Dijkstra's algorithm on the city graph
//inputs: graph, start point, destination point
//returns Some(vec of waypoints) or None if no path exists
pub fn compute_path(graph: &CityGraph, start: &Point, dest: &Point) -> Option<Vec<Waypoint>> {
    let start_node = graph.nearest_node(start.x, start.y);
    let goal_node = graph.nearest_node(dest.x, dest.y);
    let node_path = dijkstra(graph, start_node, goal_node)?;
    if node_path.len() < 2 {
        return None;
    }
    let mut waypoints = Vec::new();
    for i in 0..node_path.len() - 1 {
        let a = &graph.nodes[node_path[i]];
        let b = &graph.nodes[node_path[i + 1]];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dist = dx.hypot(dy);
        let dir_x = dx / dist;
        let dir_y = dy / dist;
        waypoints.push(Waypoint {
            x: a.x,
            y: a.y,
            dir_x,
            dir_y,
            dist_to_next: dist,
        });
    }
    let last = &graph.nodes[*node_path.last().unwrap()];
    waypoints.push(Waypoint {
        x: last.x,
        y: last.y,
        dir_x: 0.0,
        dir_y: 0.0,
        dist_to_next: 0.0,
    });
    Some(waypoints)
}

//unit tests
#[cfg(test)]
mod tests {
    use crate::map::{GraphEdge, GraphNode};

    use super::*;

    fn build_test_graph() -> CityGraph {
        let nodes = vec![
            GraphNode { x: 0.0, y: 0.0 },
            GraphNode { x: 100.0, y: 0.0 },
            GraphNode { x: 200.0, y: 0.0 },
            GraphNode {
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
        let start = Point { x: 0.0, y: 0.0 };
        let dest = Point { x: 200.0, y: 0.0 };
        let path = compute_path(&graph, &start, &dest).expect("should find path");
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].x, 0.0);
        assert_eq!(path[1].x, 100.0);
        assert_eq!(path[2].x, 200.0);
    }

    #[test]
    fn test_path_with_turn() {
        let graph = build_test_graph();
        let start = Point { x: 0.0, y: 0.0 };
        let dest = Point {
            x: 100.0,
            y: -100.0,
        };
        let path = compute_path(&graph, &start, &dest).expect("should find path");
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].x, 0.0);
        assert_eq!(path[1].x, 100.0);
        assert_eq!(path[2].y, -100.0);
    }

    #[test]
    fn test_direction_vectors_are_normalised() {
        let graph = build_test_graph();
        let start = Point { x: 0.0, y: 0.0 };
        let dest = Point { x: 200.0, y: 0.0 };
        let path = compute_path(&graph, &start, &dest).unwrap();
        for wp in &path[..path.len() - 1] {
            let mag = wp.dir_x.hypot(wp.dir_y);
            assert!(
                (mag - 1.0).abs() < 1e-9,
                "direction not normalised: mag={}",
                mag
            );
        }
    }

    #[test]
    fn test_final_waypoint_has_zero_direction() {
        let graph = build_test_graph();
        let start = Point { x: 0.0, y: 0.0 };
        let dest = Point { x: 200.0, y: 0.0 };
        let path = compute_path(&graph, &start, &dest).unwrap();
        let last = path.last().unwrap();
        assert_eq!(last.dir_x, 0.0);
        assert_eq!(last.dir_y, 0.0);
        assert_eq!(last.dist_to_next, 0.0);
    }

    #[test]
    fn test_unreachable_returns_none() {
        let graph = CityGraph {
            nodes: vec![GraphNode { x: 0.0, y: 0.0 }, GraphNode { x: 100.0, y: 0.0 }],
            edges: vec![],
            adjacency: vec![vec![], vec![]],
        };
        let start = Point { x: 0.0, y: 0.0 };
        let dest = Point { x: 100.0, y: 0.0 };
        assert!(compute_path(&graph, &start, &dest).is_none());
    }

    #[test]
    fn test_dist_to_next_is_correct() {
        let graph = build_test_graph();
        let start = Point { x: 0.0, y: 0.0 };
        let dest = Point { x: 200.0, y: 0.0 };
        let path = compute_path(&graph, &start, &dest).unwrap();
        assert!((path[0].dist_to_next - 100.0).abs() < 1e-9);
        assert!((path[1].dist_to_next - 100.0).abs() < 1e-9);
    }

    #[test]
    fn test_nearest_node_snapping() {
        let graph = build_test_graph();
        let start = Point { x: 1.0, y: 0.0 };
        let dest = Point { x: 199.0, y: 0.0 };
        let path = compute_path(&graph, &start, &dest).expect("should snap and find path");
        assert_eq!(path.last().unwrap().x, 200.0);
    }
}
