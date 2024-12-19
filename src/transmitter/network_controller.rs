use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use crossbeam_channel::Sender;
use rand::Rng;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, NodeType, Packet};
use crate::transmitter::gateway::Gateway;
use crate::transmitter::transmission_handler::TransmissionHandler;

pub struct NetworkController {
    node_id: NodeId,
    network_graph: NetworkGraph,
    // network controller -> gateway
    gateway: Rc<Gateway> // gateway reference used to send all the FloodRequests. Consider sending them back to transmitter and the forwarding them to gateway maybe?
}

impl NetworkController {
    fn new() -> Self {
        todo!()
    }

    fn flood_network(&self) {
        let mut rng = rand::rng();
        let flood_id: u64 = rng.random();
        // TODO: consider using a different session_id maybe? Or just reserve 0 for this kind of messages?
        let flood_request = Packet::new_flood_request(
            SourceRoutingHeader::new(vec![], 0),
            0,
            FloodRequest::new(flood_id, self.node_id),
        );
        self.gateway.send_flood(flood_request);
    }

    pub fn get_path(&self, to: NodeId) -> Option<Vec<NodeId>> {
        // TODO: call Dijkstra's (or any other) algorithm to find the beset route.
        // Use also estimated pdr to get best route
        todo!()
    }
}

struct NetworkGraph {
    nodes: RefCell<Vec<Rc<NetworkNode>>>
}

impl NetworkGraph {
    fn new() -> Self {
        todo!()
    }

    fn insert_edges_from_path_trace(&self, path_trace: &[(NodeId, NodeType)]) {
        todo!()
    }

    fn insert_edge(&self, from: NodeId, to: NodeId) {
        todo!()
    }

    fn delete_edge(&self, from: NodeId, to: NodeId) {
        todo!()
    }

    fn delete_node(&self, id: NodeId) {
        todo!()
    }

    fn dijkstra(&self) {
        todo!()
    }

    fn get_path_to(&self, to: NodeId) -> Vec<NodeId> {
        todo!()
    }
}

struct NetworkNode {
    node_id: NodeId,
    node_type: NodeType,
    num_of_dropped_packets: usize, // TODO: maybe it is useful to add some timestamps or whatever
    // to delete old dropped packets, so that if an unreliable drone gets its pdr changed it get
    // selected during the path finding part, or vice versa, if a reliable drone gets its pdr raised
    neighbors: RefCell<Vec<Rc<NetworkNode>>>
}

impl NetworkNode {
    fn new() -> Self {
        todo!()
    }

    fn insert_edge(&self, to: NodeId) {
        todo!()
    }

    fn remove_edge(&self, to: NodeId) {
        todo!()
    }
}