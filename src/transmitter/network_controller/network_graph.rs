mod network_node;

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::rc::Rc;
use std::u64::MAX;
use rand::distr::uniform::SampleBorrow;
use wg_2024::network::NodeId;
use wg_2024::packet::NodeType;
use crate::transmitter::network_controller::network_graph::network_node::NetworkNode;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct NetworkGraph {
    node_id: NodeId,
    node_type: NodeType,
    nodes: RefCell<Vec<Rc<RefCell<NetworkNode>>>>
}

impl NetworkGraph {
    pub(super) fn new(
        node_id: NodeId, // owner's node_id
        node_type: NodeType // owner's node_type
    ) -> Self {
        let result = Self {
            node_id,
            node_type,
            nodes: RefCell::new(vec![])
        };
        result.insert_node(node_id, node_type);
        result
    }

    fn insert_node(&self, node_id: NodeId, node_type: NodeType) {
        let node = NetworkNode::new(node_id, node_type);
        let node = RefCell::new(node);
        let node = Rc::new(node);
        self.nodes.borrow_mut().push(node);
    }

    fn insert_node_if_not_present(&self, node_id: NodeId, node_type: NodeType) {
        let insert_node = {
            let nodes = self.nodes.borrow();
            nodes.iter().find(|node| node.borrow().node_id == node_id).is_none()
        };

        if insert_node {
            self.insert_node(node_id, node_type);
        }
    }

    fn delete_node(&self, node_id: NodeId) {
        let index = self.nodes.borrow_mut().iter().position(|x| x.borrow().node_id == node_id);
        if let Some(index) = index {
            self.nodes.borrow_mut().remove(index);
        }
    }

    pub(super) fn reset_graph(&mut self) {
        self.nodes.borrow_mut().retain(|node| node.borrow().node_id == self.node_id);

        // It may be faster to just do
        // self.nodes = RefCell::new(vec![]);
        // self.insert_node(self.node_id, self.node_type);
    }

    /// Inserts a bidirectional edge between a and b
    fn insert_bidirectional_edge(&self, a: NodeId, b: NodeId) {
        let nodes = self.nodes.borrow();
        let node_a = match nodes.iter().find(|node| node.borrow().node_id == a) {
            Some(node) => node,
            None => panic!("Node 'a' with node_id {a} does not exist"),
        };
        let node_b = match nodes.iter().find(|node| node.borrow().node_id == b) {
            Some(node) => node,
            None => panic!("Node 'b' with node_id {b} does not exist"),
        };
        node_a.borrow_mut().insert_edge(b);
        node_b.borrow_mut().insert_edge(a);
    }

    pub fn insert_edges_from_path_trace(&self, path_trace: &[(NodeId, NodeType)]) {
        for (node_id, node_type) in path_trace.iter() {
            self.insert_node_if_not_present(*node_id, *node_type);
        }

        for ((first_id, _first_type), (second_id, _second_type)) in path_trace.iter().zip(path_trace.iter().skip(1)) {
            self.insert_bidirectional_edge(*first_id, *second_id);
        }
    }

    pub(super) fn delete_bidirectional_edge(&self, from: NodeId, to: NodeId) {
        let nodes = self.nodes.borrow();
        let node_from = nodes.iter().find(|node| node.borrow().node_id == from);
        let node_to = nodes.iter().find(|node| node.borrow().node_id == to);
        match (node_from, node_to) {
            (Some(node_from), Some(node_to)) => {
                node_from.borrow_mut().remove_edge(to);
                node_to.borrow_mut().remove_edge(from);
            },
            _ => {
                // TODO: don't do anything?
            }
        }
    }

    pub(super) fn increment_num_of_dropped_packets(&self, node_id: NodeId) {
        let borrow_mut = self
            .nodes
            .borrow_mut();
        let mut faulty_node = borrow_mut
            .iter()
            .find(|node| node.borrow().node_id == node_id);
        match faulty_node {
            Some(node) => {
                node.borrow_mut().increment_dropped_packets();
            }
            None => {
                // just ignore this case?
                // It may arise when an old Nack::Dropped is received after resetting the
                // graph and flooding it again
            }
        }
    }

    /// Returns an HashMap associating every node to its predecessor
    fn get_paths(&self) -> HashMap<NodeId, NodeId> {
        let mut come_from: HashMap<NodeId, NodeId> = HashMap::new();
        // come_from.insert(self.node_id, self.node_id); // ??

        let mut to_be_examined: Vec<NodeId> = Vec::new();

        let mut costs: HashMap<NodeId, u64> = HashMap::new();
        costs.insert(self.node_id, 0);

        to_be_examined.push(self.node_id);

        while !to_be_examined.is_empty() {
            let current_node_id = to_be_examined[0];
            to_be_examined.remove(0);

            let borrow = self.nodes.borrow();
            let current_node = match borrow.iter().find(|node| node.borrow().node_id == current_node_id) {
                Some(node) => node,
                None => panic!("Error with nodes while getting paths"),
            };

            if current_node.borrow().node_type != NodeType::Drone && current_node.borrow().node_id != self.node_id {
                continue;
            }

            let current_cost = match costs.get(&current_node_id) {
                Some(cost) => *cost,
                None => panic!("Error with costs while getting paths"),
            };

            for neighbor_node_id in current_node.borrow().neighbors.borrow().iter() {
                let neighbor_current_cost = match costs.get(neighbor_node_id) {
                    Some(cost) => *cost,
                    None => u64::MAX,
                };

                let neighbor = {
                    let borrow = self.nodes.borrow();
                    match borrow.iter().find(|node| node.borrow().node_id == *neighbor_node_id) {
                        Some(node) => node.clone(),
                        None => panic!("Node not found"),
                    }
                };

                let neighbor_proposed_cost = current_cost + neighbor.borrow().num_of_dropped_packets;

                if neighbor_proposed_cost < neighbor_current_cost {
                    come_from.insert(*neighbor_node_id, current_node_id);
                    costs.insert(*neighbor_node_id, neighbor_proposed_cost);

                    if !to_be_examined.contains(&neighbor_node_id) {
                        to_be_examined.push(*neighbor_node_id);
                    }
                }
            }

            to_be_examined.sort_by_key(|node| costs.get(node).unwrap());
        }

        come_from
    }

    pub fn get_path_to(&self, to: NodeId) -> Option<Vec<NodeId>> {
        let distances = self.get_paths();
        let mut result: Vec<NodeId> = Vec::new();

        let mut current = distances.get(&to)?;
        result.push(to);
        while *current != self.node_id {
            result.push(*current);
            current = distances.get(current)?;
        }
        result.push(self.node_id);

        result.reverse();
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use wg_2024::packet::FloodResponse;
    use super::*;

    #[test]
    fn initialize() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let graph = NetworkGraph::new(node_id, node_type);

        let node = NetworkNode::new(node_id, node_type);
        let node = Rc::new(RefCell::new(node));
        let expected = NetworkGraph {
            node_id,
            node_type,
            nodes: RefCell::new(vec![node]),
        };

        assert_eq!(graph, expected);
    }

    #[test]
    fn insert_two_equal_nodes() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let mut graph = NetworkGraph::new(node_id, node_type);

        let owner_node = NetworkNode::new(node_id, node_type);
        let owner_node = Rc::new(RefCell::new(owner_node));

        let new_node_id = 1;
        let new_node_type = NodeType::Drone;
        let node = NetworkNode::new(new_node_id, new_node_type);
        let node = Rc::new(RefCell::new(node));

        graph.insert_node(new_node_id, new_node_type);
        graph.insert_node(new_node_id, new_node_type);

        let expected = NetworkGraph {
            node_id,
            node_type,
            nodes: RefCell::new(vec![owner_node.clone(), node.clone(), node.clone()]),
        };

        assert_eq!(graph, expected);
    }

    #[test]
    fn insert_node_if_not_present_twice() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let mut graph = NetworkGraph::new(node_id, node_type);

        let owner_node = NetworkNode::new(node_id, node_type);
        let owner_node = Rc::new(RefCell::new(owner_node));

        let new_node_id = 1;
        let new_node_type = NodeType::Drone;
        let node = NetworkNode::new(new_node_id, new_node_type);
        let node = Rc::new(RefCell::new(node));

        graph.insert_node_if_not_present(new_node_id, new_node_type);
        graph.insert_node_if_not_present(new_node_id, new_node_type);

        let expected = NetworkGraph {
            node_id,
            node_type,
            nodes: RefCell::new(vec![owner_node.clone(), node.clone()]),
        };

        assert_eq!(graph, expected);
    }

    #[test]
    fn reset_graph_to_initial_state() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let mut graph = NetworkGraph::new(node_id, node_type);

        let owner_node = NetworkNode::new(node_id, node_type);
        let owner_node = Rc::new(RefCell::new(owner_node));

        let new_node_id = 1;
        let new_node_type = NodeType::Drone;
        let node_1 = NetworkNode::new(new_node_id, new_node_type);
        let node_1 = Rc::new(RefCell::new(node_1));

        graph.insert_node(new_node_id, new_node_type);
        graph.insert_node(new_node_id, new_node_type);

        let expected = NetworkGraph {
            node_id,
            node_type,
            nodes: RefCell::new(vec![owner_node.clone(), node_1.clone(), node_1.clone()]),
        };

        assert_eq!(graph, expected);

        let new_node_id = 2;
        let new_node_type = NodeType::Drone;
        let node_2 = NetworkNode::new(new_node_id, new_node_type);
        let node_2 = Rc::new(RefCell::new(node_2));

        graph.insert_node_if_not_present(new_node_id, new_node_type);
        graph.insert_node_if_not_present(new_node_id, new_node_type);

        let expected = NetworkGraph {
            node_id,
            node_type,
            nodes: RefCell::new(vec![owner_node.clone(), node_1.clone(), node_1.clone(), node_2.clone()]),
        };

        assert_eq!(graph, expected);

        graph.reset_graph();

        let expected = NetworkGraph {
            node_id,
            node_type,
            nodes: RefCell::new(vec![owner_node.clone()]),
        };

        assert_eq!(graph, expected);
    }

    #[test]
    fn add_edges_from_path_trace() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let graph = NetworkGraph::new(node_id, node_type);

        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![
                (node_id, node_type),
                (1, NodeType::Drone),
                (2, NodeType::Drone),
                (3, NodeType::Drone),
                (4, NodeType::Client),
            ],
        };

        graph.insert_edges_from_path_trace(&flood_response.path_trace);

        let owner_node = create_rc_refcell_node(node_id, node_type);
        let node_1 = create_rc_refcell_node(1, NodeType::Drone);
        let node_2 = create_rc_refcell_node(2, NodeType::Drone);
        let node_3 = create_rc_refcell_node(3, NodeType::Drone);
        let node_4 = create_rc_refcell_node(4, NodeType::Client);

        owner_node.borrow_mut().neighbors.borrow_mut().push(1);
        node_1.borrow_mut().neighbors.borrow_mut().push(0);
        node_1.borrow_mut().neighbors.borrow_mut().push(2);
        node_2.borrow_mut().neighbors.borrow_mut().push(1);
        node_2.borrow_mut().neighbors.borrow_mut().push(3);
        node_3.borrow_mut().neighbors.borrow_mut().push(2);
        node_3.borrow_mut().neighbors.borrow_mut().push(4);
        node_4.borrow_mut().neighbors.borrow_mut().push(3);

        let expected = NetworkGraph {
            node_id,
            node_type,
            nodes: RefCell::new(vec![
                owner_node.clone(),
                node_1.clone(),
                node_2.clone(),
                node_3.clone(),
                node_4.clone(),
            ]),
        };

        assert_eq!(graph, expected);
    }

    #[test]
    fn add_bidirectional_graph() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let mut graph = NetworkGraph::new(node_id, node_type);

        graph.insert_node_if_not_present(1, NodeType::Drone);
        graph.insert_node_if_not_present(2, NodeType::Drone);

        let expected = NetworkGraph {
            node_id,
            node_type,
            nodes: RefCell::new(vec![
                create_rc_refcell_node(node_id, node_type),
                create_rc_refcell_node(1, NodeType::Drone),
                create_rc_refcell_node(2, NodeType::Drone),
            ]),
        };

        assert_eq!(graph, expected);

        graph.insert_bidirectional_edge(1, 2);

        let node_1 = graph.nodes.borrow()[1].clone();
        let node_2 = graph.nodes.borrow()[2].clone();

        let mut expected_1 = create_rc_refcell_node(1, NodeType::Drone);
        expected_1.borrow_mut().neighbors.borrow_mut().push(2);

        let mut expected_2 = create_rc_refcell_node(2, NodeType::Drone);
        expected_2.borrow_mut().neighbors.borrow_mut().push(1);

        assert_eq!(node_1, expected_1);
        assert_eq!(node_2, expected_2);
    }

    #[test]
    fn delete_edge() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let mut graph = NetworkGraph::new(node_id, node_type);

        graph.insert_node_if_not_present(1, NodeType::Drone);
        graph.insert_node_if_not_present(2, NodeType::Drone);

        let expected = NetworkGraph {
            node_id,
            node_type,
            nodes: RefCell::new(vec![
                create_rc_refcell_node(node_id, node_type),
                create_rc_refcell_node(1, NodeType::Drone),
                create_rc_refcell_node(2, NodeType::Drone),
            ]),
        };

        assert_eq!(graph, expected);

        graph.insert_bidirectional_edge(1, 2);

        let node_1 = graph.nodes.borrow()[1].clone();
        let node_2 = graph.nodes.borrow()[2].clone();

        let mut expected_1 = create_rc_refcell_node(1, NodeType::Drone);
        expected_1.borrow_mut().neighbors.borrow_mut().push(2);

        let mut expected_2 = create_rc_refcell_node(2, NodeType::Drone);
        expected_2.borrow_mut().neighbors.borrow_mut().push(1);

        assert_eq!(node_1, expected_1);
        assert_eq!(node_2, expected_2);

        graph.delete_bidirectional_edge(1, 2);
        let expected_1 = create_rc_refcell_node(1, NodeType::Drone);
        let expected_2 = create_rc_refcell_node(2, NodeType::Drone);

        assert_eq!(node_1, expected_1);
        assert_eq!(node_2, expected_2);
    }

    #[test]
    fn get_path_to_node() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let graph = NetworkGraph::new(node_id, node_type);

        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![
                (node_id, node_type),
                (1, NodeType::Drone),
                (2, NodeType::Drone),
                (3, NodeType::Drone),
                (4, NodeType::Client),
            ],
        };

        graph.insert_edges_from_path_trace(&flood_response.path_trace);

        let hops = graph.get_path_to(100);
        assert_eq!(hops, None);

        let hops = graph.get_path_to(4);
        let expected = Some(vec![0, 1, 2, 3, 4]);
        assert_eq!(hops, expected);

        graph.insert_bidirectional_edge(1, 4);
        let hops = graph.get_path_to(4);
        let expected = Some(vec![0, 1, 4]);
        assert_eq!(hops, expected);
    }

    #[test]
    fn get_path_to_should_return_none() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let graph = NetworkGraph::new(node_id, node_type);

        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![
                (node_id, node_type),
                (1, NodeType::Drone),
                (2, NodeType::Client),
                (3, NodeType::Drone),
                (4, NodeType::Client),
            ],
        };

        graph.insert_edges_from_path_trace(&flood_response.path_trace);

        let hops = graph.get_path_to(4);
        assert_eq!(hops, None);
    }

    fn create_rc_refcell_node(node_id: NodeId, node_type: NodeType) -> Rc<RefCell<NetworkNode>> {
        Rc::new(RefCell::new(NetworkNode::new(node_id, node_type)))
    }
}