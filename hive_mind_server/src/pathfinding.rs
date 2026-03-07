/*
prologue
Name of program: HiveMind Pathfinding Module
Description: Implements Dijkstra-based pathfinding and converts node paths into drivable waypoints for the HiveMind simulation.
Author: Muhammad Ibrahim
Date Created: 3/1/2026
Date Revised: 3/2/2026
Revision History: Included in the numerous sprint artifacts.
Preconditions: Not applicable/Redundant
Postconditions: Not applicable/Redundant
*/
// Dijkstra pathfinding from start to dest on the city graph.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::map::CityGraph;
// Defines the Waypoint struct, which contains the x, y coordinates of the waypoint, the direction of the waypoint, and the distance to the next waypoint
#[derive(Debug, Clone)]
pub struct Waypoint {
    pub x: f64,
    pub y: f64,
    pub dir_x: f64,
    pub dir_y: f64,
    pub dist_to_next: f64,
}
// Defines the State struct, which contains the cost of the state and the node of the state
#[derive(Copy, Clone)]
struct State {
    cost: f64,
    node: usize,
}

// Implements the Ord trait for the State struct, which allows for comparison of states
impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.partial_cmp(&self.cost).unwrap_or(Ordering::Equal)
    }
}

// Implements the PartialOrd trait for the State struct, which allows for partial comparison of states
impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Implements the PartialEq trait for the State struct, which allows for partial equality of states
impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

// Implements the Eq trait for the State struct, which allows for equality of states
impl Eq for State {}
// Defines the dijkstra function, which implements the Dijkstra algorithm to find the shortest path between two nodes
fn dijkstra(graph: &CityGraph, start: usize, goal: usize) -> Option<Vec<usize>> {
    let n = graph.nodes.len();
    // Initializes the distance vector with infinity for all nodes
    let mut dist = vec![f64::INFINITY; n];
    // Initializes the previous vector with the maximum value for all nodes
    let mut prev = vec![usize::MAX; n];
    // Initializes the heap with the start node
    let mut heap = BinaryHeap::new();
    // Sets the distance to the start node to 0
    dist[start] = 0.0;
    heap.push(State { cost: 0.0, node: start });
    // While the heap is not empty, pop the node with the smallest cost
    while let Some(State { cost, node }) = heap.pop() {
        if node == goal {
            break;
        }
        if cost > dist[node] {
            continue;
        }
        // For each edge connected to the current node, calculate the cost to the next node
        for &edge_idx in &graph.adjacency[node] {
            let edge = &graph.edges[edge_idx];
            let next_cost = cost + edge.length;
            // If the cost to the next node is less than the current distance to the next node, update the distance and previous node
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
    // If the distance to the goal node is infinity, return None
    if dist[goal] == f64::INFINITY {
        return None;
    }
    // Initializes the path vector with the goal node
    let mut path = Vec::new();
    let mut cur = goal;
    // While the current node is not the start node, add the current node to the path and set the current node to the previous node
    while cur != usize::MAX {
        path.push(cur);
        cur = prev[cur];
    }
    path.reverse();
    Some(path)
}

/// Computes a drivable path from (start_x, start_y) to (dest_x, dest_y) using Dijkstra's algorithm
/// on the city graph. Returns a list of waypoints along the shortest path; the server uses these
/// to send direction updates so the car follows the path and turns at each segment.
pub fn compute_path(
    graph: &CityGraph,
    start_x: f64,
    start_y: f64,
    dest_x: f64,
    dest_y: f64,
) -> Option<Vec<Waypoint>> {
    let start_node = graph.nearest_node(start_x, start_y);
    let goal_node = graph.nearest_node(dest_x, dest_y);
    let node_path = dijkstra(graph, start_node, goal_node)?;
    // If the path is less than 2 nodes, return None
    if node_path.len() < 2 {
        return None;
    }
    // Smaller step = more waypoints = tighter to the road and smoother turns
    const STEP_M: f64 = 5.0;
    let mut waypoints = Vec::new();
    // For each node in the path, calculate the waypoints (skip zero-length edges so we never emit dir 0,0)
    for i in 0..node_path.len() - 1 {
        let a = &graph.nodes[node_path[i]];
        let b = &graph.nodes[node_path[i + 1]];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dist = dx.hypot(dy);
        if dist < 1e-9 {
            continue;
        }
        let dir_x = dx / dist;
        let dir_y = dy / dist;
        let mut d = 0.0;
        // While the distance is less than the distance to the next node, calculate the waypoints
        while d < dist - 1e-6 {
            let seg = (dist - d).min(STEP_M);
            // Add the waypoint to the waypoints vector
            waypoints.push(Waypoint {
                x: a.x + dir_x * d,
                y: a.y + dir_y * d,
                dir_x,
                dir_y,
                dist_to_next: seg,
            });
            d += STEP_M;
        }
    }
    // Adds the last node to the waypoints vector
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
