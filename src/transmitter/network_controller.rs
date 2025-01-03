mod network_graph;

use std::sync::{Arc, RwLock};
use messages::node_event::{EventNetworkGraph, EventNetworkNode, NodeEvent};
use rand::Rng;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, FloodResponse, NackType, NodeType, Packet, PacketType};
use crate::transmitter::gateway::Gateway;
use crate::transmitter::network_controller::network_graph::NetworkGraph;

#[derive(Debug)]
pub struct NetworkController {
    node_id: NodeId,
    node_type: NodeType,
    network_graph: RwLock<NetworkGraph>,
    gateway: Arc<Gateway> // gateway reference used to send all the FloodRequests
}

impl PartialEq for NetworkController {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id
            && self.network_graph.read().unwrap().eq(&other.network_graph.read().unwrap())
            && self.gateway.eq(&other.gateway)
    }
}

impl NetworkController {
    pub fn new(
        node_id: NodeId,
        node_type: NodeType,
        gateway: Arc<Gateway>,
    )-> Self {
        Self {
            node_id,
            node_type,
            network_graph: RwLock::new(NetworkGraph::new(node_id, node_type)),
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
            FloodRequest::initialize(flood_id, self.node_id, self.node_type),
        );
        self.gateway.send_flood(flood_request);
    }

    pub fn get_path(&self, to: NodeId) -> Option<Vec<NodeId>> {
        self.network_graph.read().unwrap().get_path_to(to)
    }

    pub fn update_from_flood_response(&self, flood_response: FloodResponse) {
        self.network_graph.read().unwrap().insert_edges_from_path_trace(&flood_response.path_trace)
        // TODO: send KnownNetworkGraph
    }

    pub fn update_from_nack(&self, nack_packet: &Packet) {
        match &nack_packet.pack_type {
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
                        self.network_graph.read().unwrap().delete_bidirectional_edge(from, next_hop);
                        // TODO: send KnownNetworkGraph
                    }
                    NackType::DestinationIsDrone | NackType::UnexpectedRecipient(_) => {
                        // Something went wrong, reset the network graph and flood the network again
                        self.network_graph.write().unwrap().reset_graph();
                        // TODO: send KnownNetworkGraph
                        let event_network_graph = self.get_event_graph();
                        let event = NodeEvent::KnownNetworkGraph(event_network_graph);


                        self.flood_network();
                    }
                    NackType::Dropped => {
                        // Update num_of_dropped_packets
                        let faulty_node_id = match nack_packet.routing_header.source() {
                            Some(node) => node,
                            None => panic!("Received a packet with no hops in routing header")
                        };
                        self.network_graph.read().unwrap().increment_num_of_dropped_packets(faulty_node_id);
                        // TODO: send KnownNetworkGraph - overkill? -> create a new event 'UpdateNumOfDroppedPackets(NodeId, num_of_dropped_packets)'
                    }
                }
            },
            _ => {
                panic!("Expected nack packet!")
            }
        }
    }

    fn get_event_graph(&self) -> EventNetworkGraph {
        let mut nodes = vec![];

        let network_graph = self.network_graph.read().unwrap();
        for network_node in network_graph.nodes.read().unwrap().iter() {
            let network_node = network_node.read().unwrap();
            let neighbors = {
                let mut neighbors: Vec<NodeId> = vec![];

                for neighbor in network_node.neighbors.read().unwrap().iter() {
                    neighbors.push(*neighbor)
                }

                neighbors
            };
            let result_node = EventNetworkNode {
                node_id: network_node.node_id,
                node_type: network_node.node_type,
                num_of_dropped_packets: network_node.num_of_dropped_packets,
                neighbors,
            };
            nodes.push(result_node);
        }

        EventNetworkGraph {
            nodes
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use crossbeam_channel::{unbounded, Sender};
    use ntest::timeout;
    use wg_2024::packet::{Ack, Nack};
    use super::*;

    #[test]
    fn initialize() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let (listener_tx, _listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(node_id, HashMap::new(), listener_tx);
        let gateway = Arc::new(gateway);

        let network_controller = NetworkController::new(node_id, node_type, gateway.clone());

        let expected = NetworkController {
            node_id,
            node_type,
            network_graph: RwLock::new(NetworkGraph::new(node_id, node_type)),
            gateway: gateway.clone(),
        };

        assert_eq!(network_controller, expected);
    }

    #[test]
    fn update_from_error_in_routing() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let (listener_tx, _listener_rx) = unbounded::<Packet>();
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

        network_controller.update_from_nack(&nack);

        let hops = network_controller.get_path(8);
        let expected = Some(vec![0, 2, 5, 8]);
        assert_eq!(hops, expected);
    }

    #[test]
    fn update_from_dropped() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let (listener_tx, _listener_rx) = unbounded::<Packet>();
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
        network_controller.update_from_nack(&nack);

        let hops = network_controller.get_path(3);
        let expected = Some(vec![0, 1, 4, 3]);
        assert_eq!(hops, expected);
    }

    #[test]
    #[timeout(2000)]
    fn check_flood_network() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let (listener_tx, _listener_rx) = unbounded::<Packet>();

        let mut connected_drones:HashMap<NodeId, Sender<Packet>> = HashMap::new();

        let drone_1_node_id = 1;
        let (drone_1_tx, drone_1_rx) = unbounded::<Packet>();
        connected_drones.insert(drone_1_node_id, drone_1_tx);

        let drone_2_node_id = 2;
        let (drone_2_tx, drone_2_rx) = unbounded::<Packet>();
        connected_drones.insert(drone_2_node_id, drone_2_tx);

        let gateway = Gateway::new(node_id, connected_drones, listener_tx);
        let gateway = Arc::new(gateway);

        let mut network_controller = NetworkController::new(node_id, node_type, gateway);

        network_controller.flood_network();

        let expected_source_routing_header = SourceRoutingHeader {
            hop_index: 0,
            hops: vec![],
        };
        let expected_initiator_id = node_id;
        let expected_path_trace = vec![(node_id, node_type)];

        let received = drone_1_rx.recv().unwrap();

        assert_eq!(received.routing_header, expected_source_routing_header);
        let received_flood_request = match &received.pack_type {
            PacketType::FloodRequest(flood_request) => flood_request,
            _ => panic!("Received unexpected packet type"),
        };
        assert_eq!(received_flood_request.initiator_id, expected_initiator_id);
        assert_eq!(received_flood_request.path_trace, expected_path_trace);

        let received = drone_2_rx.recv().unwrap();

        assert_eq!(received.routing_header, expected_source_routing_header);
        let received_flood_request = match &received.pack_type {
            PacketType::FloodRequest(flood_request) => flood_request,
            _ => panic!("Received unexpected packet type"),
        };
        assert_eq!(received_flood_request.initiator_id, expected_initiator_id);
        assert_eq!(received_flood_request.path_trace, expected_path_trace);
    }

    #[test]
    fn reset_graph_from_flood() {
        let node_id = 0;
        let node_type = NodeType::Server;

        let connected_drones = HashMap::new();
        let (gateway_to_listener_tx, gateway_to_listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(node_id, connected_drones, gateway_to_listener_tx);
        let gateway = Arc::new(gateway);

        let network_controller = NetworkController::new(node_id, node_type, gateway);

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

        let nack = Nack {
            fragment_index: 0,
            nack_type: NackType::DestinationIsDrone,
        };
        let nack = Packet {
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![] },
            session_id: 0,
            pack_type: PacketType::Nack(nack),
        };
        network_controller.update_from_nack(&nack);

        assert_eq!(network_controller.network_graph.read().unwrap().nodes.read().unwrap().len(), 1);
    }

    #[test]
    #[should_panic(expected = "Received a NACK packet with no source in header")]
    fn error_in_routing_with_no_header() {
        let node_id = 0;
        let node_type = NodeType::Server;

        let connected_drones = HashMap::new();
        let (gateway_to_listener_tx, gateway_to_listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(node_id, connected_drones, gateway_to_listener_tx);
        let gateway = Arc::new(gateway);

        let network_controller = NetworkController::new(node_id, node_type, gateway);

        let nack = Nack {
            fragment_index: 0,
            nack_type: NackType::ErrorInRouting(10),
        };
        let nack = Packet {
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![] },
            session_id: 0,
            pack_type: PacketType::Nack(nack),
        };

        network_controller.update_from_nack(&nack);
    }

    #[test]
    #[should_panic(expected = "Received a packet with no hops in routing header")]
    fn dropped_with_no_header() {
        let node_id = 0;
        let node_type = NodeType::Server;

        let connected_drones = HashMap::new();
        let (gateway_to_listener_tx, gateway_to_listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(node_id, connected_drones, gateway_to_listener_tx);
        let gateway = Arc::new(gateway);

        let network_controller = NetworkController::new(node_id, node_type, gateway);

        let nack = Nack {
            fragment_index: 0,
            nack_type: NackType::Dropped,
        };
        let nack = Packet {
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![] },
            session_id: 0,
            pack_type: PacketType::Nack(nack),
        };

        network_controller.update_from_nack(&nack);
    }

    #[test]
    #[should_panic(expected = "Expected nack packet!")]
    fn update_from_nack_wrong_packet_type() {
        let node_id = 0;
        let node_type = NodeType::Server;

        let connected_drones = HashMap::new();
        let (gateway_to_listener_tx, gateway_to_listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(node_id, connected_drones, gateway_to_listener_tx);
        let gateway = Arc::new(gateway);

        let network_controller = NetworkController::new(node_id, node_type, gateway);

        let ack = Ack {
            fragment_index: 0,
        };
        let ack = Packet {
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![] },
            session_id: 0,
            pack_type: PacketType::Ack(ack),
        };

        network_controller.update_from_nack(&ack);
    }
}