mod network_graph;

use std::sync::Arc;
use rand::Rng;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, FloodResponse, NackType, NodeType, Packet, PacketType};
use crate::transmitter::gateway::Gateway;
use crate::transmitter::network_controller::network_graph::NetworkGraph;

#[derive(Debug, Eq, PartialEq)]
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
        // TODO: send KnownNetworkGraph
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
                        self.network_graph.delete_bidirectional_edge(from, next_hop);
                        // TODO: send KnownNetworkGraph
                    }
                    NackType::DestinationIsDrone | NackType::UnexpectedRecipient(_) => {
                        // Something went wrong, reset the network graph and flood the network again
                        self.network_graph.reset_graph();
                        // TODO: send KnownNetworkGraph
                        self.flood_network();
                    }
                    NackType::Dropped => {
                        // Update num_of_dropped_packets
                        let faulty_node_id = match nack_packet.routing_header.source() {
                            Some(node) => node,
                            None => panic!("Received a packet with no routing header")
                        };
                        self.network_graph.increment_num_of_dropped_packets(faulty_node_id);
                        // TODO: send KnownNetworkGraph - overkill?
                    }
                }
            },
            _ => {
                panic!("Expected nack packet!")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use crossbeam_channel::unbounded;
    use wg_2024::packet::Nack;
    use super::*;

    #[test]
    fn initialize() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let (listener_tx, listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(node_id, HashMap::new(), listener_tx);
        let gateway = Arc::new(gateway);

        let network_controller = NetworkController::new(node_id, node_type, gateway.clone());

        let expected = NetworkController {
            node_id,
            network_graph: NetworkGraph::new(node_id, node_type),
            gateway: gateway.clone(),
        };

        assert_eq!(network_controller, expected);
    }

    #[test]
    fn update_from_error_in_routing() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let (listener_tx, listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(node_id, HashMap::new(), listener_tx);
        let gateway = Arc::new(gateway);

        let mut network_controller = NetworkController::new(node_id, node_type, gateway);

        /*
        | --------|
        0 -- 1 -- 2 -- 3
                  |
                  -- 5 -- 8
         */

        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![
                (node_id, node_type),
                (1, NodeType::Drone),
                (2, NodeType::Drone),
                (3, NodeType::Client),
            ],
        };
        network_controller.update_from_flood_response(flood_response);
        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![
                (node_id, node_type),
                (2, NodeType::Drone),
                (5, NodeType::Drone),
                (8, NodeType::Server),
            ],
        };
        network_controller.update_from_flood_response(flood_response);
        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![
                (node_id, node_type),
                (1, NodeType::Drone),
                (8, NodeType::Server),
            ],
        };
        network_controller.update_from_flood_response(flood_response);

        let hops = network_controller.get_path(8);
        let expected = Some(vec![0, 1, 8]);
        assert_eq!(hops, expected);

        let nack = Nack {
            fragment_index: 0,
            nack_type: NackType::ErrorInRouting(8),
        };
        let nack = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 1,
                hops: vec![1, 0],
            },
            session_id: 0,
            pack_type: PacketType::Nack(nack),
        };

        network_controller.update_from_nack(nack);

        let hops = network_controller.get_path(8);
        let expected = Some(vec![0, 2, 5, 8]);
        assert_eq!(hops, expected);
    }

    #[test]
    fn update_from_dropped() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let (listener_tx, listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(node_id, HashMap::new(), listener_tx);
        let gateway = Arc::new(gateway);

        let mut network_controller = NetworkController::new(node_id, node_type, gateway);

        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![
                (node_id, node_type),
                (1, NodeType::Drone),
                (2, NodeType::Drone),
                (3, NodeType::Client),
            ],
        };
        network_controller.update_from_flood_response(flood_response);

        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![
                (node_id, node_type),
                (1, NodeType::Drone),
                (4, NodeType::Drone),
                (3, NodeType::Client),
            ],
        };
        network_controller.update_from_flood_response(flood_response);

        let nack = Nack {
            fragment_index: 0,
            nack_type: NackType::Dropped,
        };
        let nack = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 2,
                hops: vec![2, 1, 0],
            },
            session_id: 0,
            pack_type: PacketType::Nack(nack),
        };
        network_controller.update_from_nack(nack);

        let hops = network_controller.get_path(3);
        let expected = Some(vec![0, 1, 4, 3]);
        assert_eq!(hops, expected);
    }
}