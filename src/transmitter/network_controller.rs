use std::sync::Arc;
use rand::Rng;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, FloodResponse, NackType, NodeType, Packet, PacketType};
use crate::transmitter::gateway::Gateway;
use crate::transmitter::network_controller::graph::NetworkGraph;

pub struct NetworkController {
    node_id: NodeId,
    network_graph: NetworkGraph,
    // network controller -> gateway
    gateway: Arc<Gateway> // gateway reference used to send all the FloodRequests
}

impl NetworkController {
    pub fn new(
        node_id: NodeId,
        node_type: NodeType,
        gateway: Arc<Gateway>,
    )-> Self {
        Self {
            node_id,
            network_graph: NetworkGraph::new(node_id, node_type),
            gateway,
        }
    }

    fn flood_network(&self) {
        let mut rng = rand::rng();
        let session_id: u64 = rng.random();
        let flood_id: u64 = rng.random();
        // TODO: consider using a different session_id maybe? Or just reserve 0 for this kind of messages?
        let flood_request = Packet::new_flood_request(
            SourceRoutingHeader::new(vec![], 0),
            session_id,
            FloodRequest::new(flood_id, self.node_id),
        );
        self.gateway.send_flood(flood_request);
    }

    pub fn get_path(&self, to: NodeId) -> Option<Vec<NodeId>> {
        self.network_graph.get_path_to(to)
    }

    pub fn update_from_flood_response(&self, flood_response: FloodResponse) {
        self.network_graph.insert_edges_from_path_trace(&flood_response.path_trace)
    }

    pub fn update_from_nack(&mut self, nack_packet: Packet) {
        match nack_packet.pack_type {
            PacketType::Nack(nack) => {
                match nack.nack_type {
                    NackType::ErrorInRouting(next_hop) => {
                        // remove edge between next_hop and nack_packet.header.0
                        let from = match nack_packet.routing_header.source() {
                            None => {
                                panic!("Received a NACK packet with no source in header")
                            }
                            Some(source) => source
                        };
                        self.network_graph.delete_edge(from, next_hop);
                    }
                    NackType::DestinationIsDrone | NackType::UnexpectedRecipient(_) => {
                        // Something went wrong, reset the network graph and flood the network again
                        self.network_graph.reset_graph(self.node_id);
                        self.flood_network();
                    }
                    NackType::Dropped => {
                        // Update num_of_dropped_packets
                        let faulty_node_id = match nack_packet.routing_header.source() {
                            Some(node) => node,
                            None => panic!("Received a packet with no routing header")
                        };
                        self.network_graph.increment_num_of_dropped_packets(faulty_node_id);
                    }
                }
            },
            _ => {
                panic!("Expected nack packet!")
            }
        }
    }
}

mod graph {
    use std::cell::RefCell;
    use std::rc::Rc;
    use wg_2024::network::NodeId;
    use wg_2024::packet::NodeType;
    use crate::transmitter::network_controller::graph::node::NetworkNode;

    pub(super) struct NetworkGraph {
        nodes: RefCell<Vec<Rc<NetworkNode>>>
    }

    impl NetworkGraph {
        pub(super) fn new(
            node_id: NodeId, // owner's node_id
            node_type: NodeType // owner's node_type
        ) -> Self {
            let result = Self {
                nodes: RefCell::new(vec![])
            };
            result.insert_node(node_id, node_type); // this ensures that owner is always at index 0
            result
        }

        pub(super) fn reset_graph(&self, server_node_id: NodeId) {
            self.nodes.borrow_mut().retain(|node| node.node_id == server_node_id);
        }

        fn insert_node_checked(&self, node_id: NodeId, node_type: NodeType) {
            let nodes = self.nodes.borrow();
            let node_entry = nodes.iter().find(|node| node.node_id == node_id);
            if node_entry.is_none() {
                self.insert_node(node_id, node_type);
            }
        }

        pub fn insert_edges_from_path_trace(&self, path_trace: &[(NodeId, NodeType)]) {
            for ((first_id, first_type), (second_id, second_type)) in path_trace.iter().zip(path_trace.iter().skip(1)) {
                self.insert_node_checked(*first_id, *first_type);
                self.insert_node_checked(*second_id, *second_type);
                self.insert_edge(*first_id, *second_id)
            }
        }

        fn insert_edge(&self, from: NodeId, to: NodeId) {
            let nodes = self.nodes.borrow();
            let node_from = match nodes.iter().find(|node| node.node_id == from) {
                Some(node) => node,
                None => panic!("Node from does not exist"),
            };
            let node_to = match nodes.iter().find(|node| node.node_id == to) {
                Some(node) => node,
                None => panic!("Node to does not exist"),
            };
            node_from.insert_edge(node_to.clone());
            node_to.insert_edge(node_from.clone());
        }

        pub(super) fn delete_edge(&self, from: NodeId, to: NodeId) {
            let nodes = self.nodes.borrow();
            let node_from = nodes.iter().find(|node| node.node_id == from);
            let node_to = nodes.iter().find(|node| node.node_id == to);
            match (node_from, node_to) {
                (Some(from), Some(to)) => {
                    from.remove_edge(to.clone());
                    to.remove_edge(from.clone());
                },
                _ => {
                    // TODO: don't do anything?
                }
            }
        }

        fn insert_node(&self, node_id: NodeId, node_type: NodeType) {
            let node = NetworkNode::new(node_id, node_type);
            let node = Rc::new(node);
            self.nodes.borrow_mut().push(node);
        }

        fn delete_node(&self, node_id: NodeId) {
            let index = self.nodes.borrow_mut().iter().position(|x| x.node_id == node_id);
            if let Some(index) = index {
                self.nodes.borrow_mut().remove(index);
            }
        }

        pub(super) fn increment_num_of_dropped_packets(&self, node_id: NodeId) {
            let faulty_node = self
                .nodes
                .borrow_mut()
                .iter()
                .find(|node| node.node_id == node_id);
            match faulty_node {
                Some(mut node) => {
                    node.increment_dropped_packets();
                }
                None => {
                    // just ignore this case?
                    // It may arise when an old Nack::Dropped is received after resetting the
                    // graph and flooding it again
                }
            }
        }

        fn dijkstra(&self) {
            todo!()
        }

        pub fn get_path_to(&self, to: NodeId) -> Option<Vec<NodeId>> {
            // TODO: call Dijkstra's (or any other) algorithm to find the best route.
            // TODO: use also estimated pdr to get best route

            todo!()
        }
    }

    mod node {
        use std::cell::RefCell;
        use std::rc::Rc;
        use wg_2024::network::NodeId;
        use wg_2024::packet::NodeType;

        pub(super) struct NetworkNode {
            pub(super) node_id: NodeId,
            node_type: NodeType,
            num_of_dropped_packets: usize, // TODO: maybe it is useful to add some timestamps or whatever
            // to delete old dropped packets, so that if an unreliable drone gets its pdr changed it get
            // selected during the path finding part, or vice versa, if a reliable drone gets its pdr raised
            neighbors: RefCell<Vec<Rc<NetworkNode>>>
        }

        impl NetworkNode {
            pub(super) fn new(
                node_id: NodeId,
                node_type: NodeType,
            ) -> Self {
                Self {
                    node_id,
                    node_type,
                    num_of_dropped_packets: 0,
                    neighbors: RefCell::new(vec![]),
                }
            }

            pub(super) fn insert_edge(&self, to: Rc<NetworkNode>) {
                self.neighbors.borrow_mut().push(to)
            }

            pub(super) fn remove_edge(&self, to: Rc<NetworkNode>) {
                let index = self.neighbors.borrow().iter().position(|node| node.node_id == to.node_id);
                if let Some(index) = index {
                    self.neighbors.borrow_mut().remove(index);
                }
            }

            pub(super) fn increment_dropped_packets(&mut self) {
                self.num_of_dropped_packets += 1
            }

            pub(super) fn reset_num_of_dropped_packets(&mut self) {
                self.num_of_dropped_packets = 0;
            }
        }
    }
}