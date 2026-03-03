// Dijkstra pathfinding from start to dest on the city graph.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::map::CityGraph;

#[derive(Debug, Clone)]
pub struct Waypoint {
    pub x: f64,
    pub y: f64,
    pub dir_x: f64,
    pub dir_y: f64,
    pub dist_to_next: f64,
}

#[derive(Copy, Clone)]
struct State {
    cost: f64,
    node: usize,
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.partial_cmp(&self.cost).unwrap_or(Ordering::Equal)
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
    heap.push(State { cost: 0.0, node: start });

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

    if node_path.len() < 2 {
        return None;
    }

    const STEP_M: f64 = 40.0;
    let mut waypoints = Vec::new();
    for i in 0..node_path.len() - 1 {
        let a = &graph.nodes[node_path[i]];
        let b = &graph.nodes[node_path[i + 1]];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dist = dx.hypot(dy);
        let dir_x = if dist > 1e-9 { dx / dist } else { 0.0 };
        let dir_y = if dist > 1e-9 { dy / dist } else { 0.0 };
        let mut d = 0.0;
        while d < dist - 1e-6 {
            let seg = (dist - d).min(STEP_M);
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
