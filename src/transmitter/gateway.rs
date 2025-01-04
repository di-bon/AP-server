use crossbeam_channel::{SendError, Sender};
use std::collections::HashMap;
use std::sync::Arc;
use messages::node_event::NodeEvent;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodResponse, Nack, NackType, Packet, PacketType};
use crate::simulation_controller_notifier::SimulationControllerNotifier;

#[derive(Debug)]
pub struct Gateway {
    node_id: NodeId,
    neighbors: HashMap<NodeId, Sender<Packet>>,
    listener_channel: Sender<Packet>,
    simulation_controller_notifier: Arc<SimulationControllerNotifier>,
}

impl PartialEq<Self> for Gateway {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id && self.neighbors.keys().eq(other.neighbors.keys())
    }
}

impl Eq for Gateway {}

impl Gateway {
    pub fn new(
        node_id: NodeId,
        neighbors: HashMap<NodeId, Sender<Packet>>,
        listener_channel: Sender<Packet>,
        simulation_controller_notifier: Arc<SimulationControllerNotifier>,
    ) -> Self {
        Self {
            node_id,
            neighbors,
            listener_channel,
            simulation_controller_notifier
        }
    }

    /// Sends a Packet to every connected neighboring node
    pub fn send_flood(&self, packet: Packet) {
        if !matches!(packet.pack_type, PacketType::FloodRequest(_)) {
            log::error!(
                "Cannot flood the network with a packet of type {:?}",
                packet.pack_type
            );
            panic!(
                "Cannot flood the network with a packet of type {:?}",
                packet.pack_type
            );
        }

        for (node_id, channel) in &self.neighbors {
            self.send_on_channel_checked(channel, packet.clone(), *node_id);
        }
    }

    /// Sends a packet on the given channel. If channel.send fails, it sends an ErrorInRouting back to listener
    fn send_on_channel_checked(&self, channel: &Sender<Packet>, packet: Packet, next_hop: NodeId) {
        match channel.send(packet.clone()) {
            Ok(()) => {
                log::info!("Packet {packet} successfully sent to {next_hop}");
                let event = NodeEvent::PacketSent(packet);
                self.simulation_controller_notifier.send_event(event);
            }
            Err(SendError(packet)) => {
                let nack_type = NackType::ErrorInRouting(next_hop);
                log::warn!("Error while sending packet {packet} to node {next_hop}: sending nack packet {nack_type:?}");
                self.send_nack_packet_to_listener(packet, nack_type);
            }
        }
    }

    /// Sends a FloodResponse packet
    pub fn send_flood_response(&self, flood_response: FloodResponse) {
        let forward_to = match flood_response.path_trace.iter().nth(1) {
            Some((node_id, _node_type)) => *node_id,
            None => {
                log::error!("Tried to send a FloodResponse with no next hop");
                panic!("Tried to send a FloodResponse with no next hop");
            }
        };
        let wrapper_packet = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![],
            },
            session_id: 0, // TODO: default value
            pack_type: PacketType::FloodResponse(flood_response),
        };
        let channel = match self.neighbors.get(&forward_to) {
            Some(channel) => channel,
            None => {
                log::error!("No channel found to forward the flood response back to who sent it");
                panic!("No channel found to forward the flood response back to who sent it");
            }
        };
        self.send_on_channel_checked(channel, wrapper_packet, forward_to);
    }

    /// Forwards a Packet based on its SourceRoutingHeader.
    /// It expects to receive Packets with hop_index set to 0
    pub fn forward(&self, mut packet: Packet) {
        let next_hop = match packet.routing_header.next_hop() {
            Some(next_hop) => next_hop,
            None => {
                log::error!("No next hop for packet {packet}");
                panic!("No next hop for current packet");
            }
        };

        packet.routing_header.hop_index += 1;

        if let Some(channel) = self.neighbors.get(&next_hop) {
            self.send_on_channel_checked(channel, packet, next_hop);
        } else {
            self.send_nack_packet_to_listener(packet, NackType::ErrorInRouting(next_hop));
        }
    }

    /// Sends a NACK to listener. Note that the only nack that this will send are just
    /// ErrorInRouting and (hopefully never) UnexpectedRecipient. There is no way that
    /// a Dropped or DestinationIsDrone gets sent, so there is no need to reverse the header
    /// or sending a nack for a specific fragment index
    fn send_nack_packet_to_listener(&self, packet: Packet, nack_type: NackType) {
        let fragment_index = match &packet.pack_type {
            PacketType::MsgFragment(fragment) => fragment.fragment_index,
            _ => 0,
        };

        let nack = Nack {
            fragment_index,
            nack_type,
        };

        // routing header needs to have a single node in hops, which is 'self.node_id' to properly handle NACKs
        let packet = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![self.node_id],
            },
            // session_id is not checked by listener if the packet is received by an internal channel, i.e. from transmitter (gateway) to listener
            session_id: 0,
            pack_type: PacketType::Nack(nack),
        };

        self.send_to_listener(packet);
    }

    /// Sends a Packet to Listener
    pub fn send_to_listener(&self, packet: Packet) {
        match self.listener_channel.send(packet.clone()) {
            Ok(()) => {
                // log::info!("Packet {packet} successfully sent to listener");
            }
            Err(_err) => {
                log::error!("Gateway cannot communicate with listener");
                panic!("Gateway cannot communicate with listener");
            }
        }
    }

    /// Adds a channel to the connected neighbors
    fn add_neighbor(&mut self, node_id: NodeId, channel: Sender<Packet>) {
        match self.neighbors.insert(node_id, channel) {
            None => log::info!("Added neighbor with NodeId {node_id}"),
            Some(_) => log::info!("Updated neighbor's channel associated to NodeId {node_id}"),
        }
    }

    /// Removes a channel from the connected neighbors
    fn remove_neighbor(&mut self, node_id: &NodeId) {
        match self.neighbors.remove(node_id) {
            Some(_) => log::info!("Remove neighbor's channel associated to NodeId {node_id}"),
            None => log::warn!("Cannot remove neighbor's channel associated to NodeId {node_id}: there is no channel to remove"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crossbeam_channel::unbounded;
    use ntest::timeout;
    use std::collections::HashMap;
    use wg_2024::packet::{Ack, FloodRequest, NodeType, Packet};

    #[test]
    fn initialize() {
        let node_id = 10;
        let (tx, _rx) = unbounded::<Packet>();
        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);
        let gateway = Gateway::new(node_id, HashMap::new(), tx, simulation_controller_notifier.clone());

        assert_eq!(gateway.node_id, node_id);
        assert!(gateway.neighbors.is_empty());

        let (tx, _rx) = unbounded::<Packet>();
        let expected = Gateway {
            node_id,
            neighbors: HashMap::new(),
            listener_channel: tx,
            simulation_controller_notifier,
        };

        assert_eq!(gateway, expected);
    }

    #[test]
    fn check_forward_failure_error_in_routing() {
        let (tx, rx) = unbounded::<Packet>();
        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);
        let gateway = Gateway::new(10, HashMap::new(), tx, simulation_controller_notifier);
        let packet = Packet {
            pack_type: PacketType::Ack(Ack { fragment_index: 0 }),
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![10, 1, 2],
            },
            session_id: 0,
        };
        gateway.forward(packet);

        let received = rx.recv().unwrap();
        let expected = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![10],
            },
            session_id: 0,
            pack_type: PacketType::Nack(Nack {
                fragment_index: 0,
                nack_type: NackType::ErrorInRouting(1),
            }),
        };
        assert_eq!(received, expected);
    }

    #[test]
    fn check_forward_successful() {
        let (tx, _rx) = unbounded::<Packet>();

        let (tx_drone, rx_drone) = unbounded::<Packet>();
        let mut neighbors = HashMap::new();
        neighbors.insert(1, tx_drone);

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let gateway = Gateway::new(10, neighbors, tx, simulation_controller_notifier);

        let packet = Packet {
            pack_type: PacketType::Ack(Ack { fragment_index: 0 }),
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![10, 1, 2],
            },
            session_id: 0,
        };

        gateway.forward(packet);

        let received = rx_drone.recv().unwrap();

        let expected = Packet {
            pack_type: PacketType::Ack(Ack { fragment_index: 0 }),
            routing_header: SourceRoutingHeader {
                hop_index: 1,
                hops: vec![10, 1, 2],
            },
            session_id: 0,
        };
        assert_eq!(received, expected);
    }

    #[test]
    #[should_panic(expected = "No next hop for current packet")]
    fn check_forward_no_next_hop() {
        let (tx, _rx) = unbounded::<Packet>();

        let (tx_drone, rx_drone) = unbounded::<Packet>();
        let mut neighbors = HashMap::new();
        neighbors.insert(1, tx_drone);

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let gateway = Gateway::new(10, neighbors, tx, simulation_controller_notifier);

        let packet = Packet {
            pack_type: PacketType::Ack(Ack { fragment_index: 0 }),
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![],
            },
            session_id: 0,
        };

        gateway.forward(packet);
    }

    #[test]
    fn send_on_channel_checked_test_successful() {
        let (tx, _rx) = unbounded::<Packet>();

        let (tx_drone, rx_drone) = unbounded::<Packet>();
        let mut neighbors = HashMap::new();
        neighbors.insert(1, tx_drone);

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let gateway = Gateway::new(10, neighbors.clone(), tx, simulation_controller_notifier);

        let packet = Packet {
            pack_type: PacketType::Ack(Ack { fragment_index: 0 }),
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![10, 1, 2],
            },
            session_id: 0,
        };

        gateway.send_on_channel_checked(
            neighbors.get(&1).unwrap(),
            packet.clone(),
            packet.routing_header.next_hop().unwrap(),
        );

        let received = rx_drone.recv().unwrap();

        assert_eq!(received, packet);
    }

    #[test]
    #[timeout(2000)]
    fn send_on_channel_checked_test_fail() {
        let (listener_tx, listener_rx) = unbounded::<Packet>();

        let (tx_drone, rx_drone) = unbounded::<Packet>();
        let mut neighbors = HashMap::new();
        neighbors.insert(1, tx_drone);

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let gateway = Gateway::new(10, neighbors.clone(), listener_tx, simulation_controller_notifier);

        let packet = Packet {
            pack_type: PacketType::Ack(Ack { fragment_index: 0 }),
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![10, 1, 2],
            },
            session_id: 0,
        };

        drop(rx_drone);
        gateway.send_on_channel_checked(
            neighbors.get(&1).unwrap(),
            packet.clone(),
            packet.routing_header.next_hop().unwrap(),
        );

        let received_from_listener = listener_rx.recv().unwrap();

        let expected = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![10],
            },
            session_id: 0,
            pack_type: PacketType::Nack(Nack {
                fragment_index: 0,
                nack_type: NackType::ErrorInRouting(1),
            }),
        };

        assert_eq!(received_from_listener, expected);
    }

    #[test]
    #[timeout(2000)]
    fn send_flood_request() {
        let gateway_node_id = 9;

        let (listener_tx, _listener_rx) = unbounded::<Packet>();

        let mut drones_rx = Vec::new();
        let mut neighbors = HashMap::new();
        let (tx_drone_1, rx_drone_1) = unbounded::<Packet>();
        neighbors.insert(1, tx_drone_1);
        drones_rx.push(rx_drone_1);
        let (tx_drone_3, rx_drone_3) = unbounded::<Packet>();
        neighbors.insert(3, tx_drone_3);
        drones_rx.push(rx_drone_3);
        let (tx_drone_10, rx_drone_10) = unbounded::<Packet>();
        neighbors.insert(10, tx_drone_10);
        drones_rx.push(rx_drone_10);

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let gateway = Gateway::new(gateway_node_id, neighbors.clone(), listener_tx, simulation_controller_notifier);

        let flood_request = FloodRequest {
            flood_id: 0,
            initiator_id: gateway_node_id,
            path_trace: vec![(gateway_node_id, NodeType::Server)],
        };
        let packet = Packet {
            pack_type: PacketType::FloodRequest(flood_request),
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![],
            },
            session_id: 0,
        };

        gateway.send_flood(packet.clone());

        for channel in &drones_rx {
            let received = channel.recv().unwrap();
            assert_eq!(received, packet);
        }
    }

    #[test]
    fn send_flood_response_successful() {
        let gateway_node_id = 9;

        let (listener_tx, _listener_rx) = unbounded::<Packet>();

        let mut neighbors = HashMap::new();
        let (tx_drone_1, rx_drone_1) = unbounded::<Packet>();
        neighbors.insert(1, tx_drone_1);

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let gateway = Gateway::new(gateway_node_id, neighbors.clone(), listener_tx, simulation_controller_notifier);

        let session_id = 0;
        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![(gateway_node_id, NodeType::Server), (1, NodeType::Drone)],
        };

        gateway.send_flood_response(flood_response.clone());

        let received = rx_drone_1.recv().unwrap();

        let expected = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![],
            },
            session_id: 0,
            pack_type: PacketType::FloodResponse(flood_response),
        };
        assert_eq!(received, expected);
    }

    #[test]
    #[should_panic(expected = "Tried to send a FloodResponse with no next hop")]
    fn send_flood_response_with_no_next_hop() {
        let gateway_node_id = 9;

        let (listener_tx, _listener_rx) = unbounded::<Packet>();

        let mut neighbors = HashMap::new();
        let (tx_drone_1, _rx_drone_1) = unbounded::<Packet>();
        neighbors.insert(1, tx_drone_1);

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let gateway = Gateway::new(gateway_node_id, neighbors.clone(), listener_tx, simulation_controller_notifier);

        let session_id = 0;
        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![(gateway_node_id, NodeType::Server)],
        };

        gateway.send_flood_response(flood_response);
    }

    #[test]
    #[should_panic(expected = "No channel found")]
    fn send_flood_response_to_non_existent_node() {
        let gateway_node_id = 9;

        let (listener_tx, _listener_rx) = unbounded::<Packet>();

        let mut neighbors = HashMap::new();
        let (tx_drone_1, _rx_drone_1) = unbounded::<Packet>();
        neighbors.insert(1, tx_drone_1);

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let gateway = Gateway::new(gateway_node_id, neighbors.clone(), listener_tx, simulation_controller_notifier);

        let session_id = 0;
        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![(gateway_node_id, NodeType::Server), (100, NodeType::Drone)],
        };

        gateway.send_flood_response(flood_response);
    }

    #[test]
    fn check_add_neighbor() {
        let (tx, _rx) = unbounded::<Packet>();

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let mut gateway = Gateway::new(10, HashMap::new(), tx, simulation_controller_notifier);

        assert_eq!(gateway.neighbors.len(), 0);
        let (tx_drone_5, _rx_drone_5) = unbounded::<Packet>();
        gateway.add_neighbor(5, tx_drone_5);
        assert_eq!(gateway.neighbors.len(), 1);
        let (tx_drone_5, _rx_drone_5) = unbounded::<Packet>();
        gateway.add_neighbor(5, tx_drone_5);
        assert_eq!(gateway.neighbors.len(), 1);
        let (tx_drone_8, _rx_drone_8) = unbounded::<Packet>();
        gateway.add_neighbor(8, tx_drone_8);
        assert_eq!(gateway.neighbors.len(), 2);
    }

    #[test]
    fn check_remove_neighbor() {
        let (tx, _rx) = unbounded::<Packet>();

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let mut gateway = Gateway::new(10, HashMap::new(), tx, simulation_controller_notifier);

        assert_eq!(gateway.neighbors.len(), 0);
        let (tx_drone_5, _rx_drone_5) = unbounded::<Packet>();
        gateway.add_neighbor(5, tx_drone_5);
        assert_eq!(gateway.neighbors.len(), 1);
        let (tx_drone_5, _rx_drone_5) = unbounded::<Packet>();
        gateway.add_neighbor(5, tx_drone_5);
        assert_eq!(gateway.neighbors.len(), 1);
        let (tx_drone_8, _rx_drone_8) = unbounded::<Packet>();
        gateway.add_neighbor(8, tx_drone_8);
        assert_eq!(gateway.neighbors.len(), 2);
        gateway.remove_neighbor(&8);
        assert_eq!(gateway.neighbors.len(), 1);
        gateway.remove_neighbor(&5);
        assert_eq!(gateway.neighbors.len(), 0);
    }
}
