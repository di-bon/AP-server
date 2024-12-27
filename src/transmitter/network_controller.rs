mod network_graph;

use std::sync::Arc;
use rand::Rng;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, FloodResponse, NackType, NodeType, Packet, PacketType};
use crate::transmitter::gateway::Gateway;
use crate::transmitter::network_controller::network_graph::NetworkGraph;

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
                        self.network_graph.reset_graph();
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