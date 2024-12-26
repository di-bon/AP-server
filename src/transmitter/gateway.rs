use std::collections::HashMap;
use crossbeam_channel::{SendError, Sender};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet;
use wg_2024::packet::{FloodResponse, Nack, NackType, Packet, PacketType};

#[derive(Debug)]
pub struct Gateway {
    node_id: NodeId,
    neighbors: HashMap<NodeId, Sender<Packet>>,
    listener_channel: Sender<Packet>,
}

impl Gateway {
    pub fn new(node_id: NodeId, neighbors: HashMap<NodeId, Sender<Packet>>, listener_channel: Sender<Packet>) -> Self {
        Self {
            node_id,
            neighbors,
            listener_channel,
        }
    }

    /// Sends a Packet to every connected neighboring node
    pub fn send_flood(&self, packet: Packet) {
        if !matches!(packet.pack_type, PacketType::FloodRequest(_)) {
            log::warn!("Cannot flood the network with a packet of type {:?}. Ignoring packet", packet.pack_type);
            return;
        }

        for (node_id, channel) in &self.neighbors {
            self.send_on_channel_checked(channel, packet.clone(), *node_id);
        }
    }

    /// Sends a packet on the given channel. If channel.send fails, it sends an ErrorInRouting back to listener
    fn send_on_channel_checked(&self, channel: &Sender<Packet>, packet: Packet, next_hop: NodeId) {
        match channel.send(packet) {
            Ok(()) => {},
            Err(SendError(packet)) => {
                self.send_nack_packet_to_receiver(packet, NackType::ErrorInRouting(next_hop));
            }
        }
    }

    /// Sends a FloodResponse packet
    pub fn send_flood_response(&self, flood_response: FloodResponse, session_id: u64) {
        let forward_to = match flood_response.path_trace.iter().skip(1).next() {
            Some((node_id, _node_type)) => *node_id,
            None => {
                // TODO: what to do?
                panic!("received a flood request with no path trace");
            }
        };
        let wrapper_packet = Packet {
            routing_header: Default::default(),
            session_id,
            pack_type: PacketType::FloodResponse(flood_response),
        };
        let channel = match self.neighbors.get(&forward_to) {
            Some(channel) => {
                channel
            },
            None => {
                // TODO: update panic message
                panic!("No channel found");
            },
        };
        self.send_on_channel_checked(channel, wrapper_packet, forward_to);
    }

    // TODO: should this return a Result<(), ()> or can it just panic?
    /// Forwards a Packet based on its SourceRoutingHeader.
    /// It expects to receive Packets with hop_index set to 1
    pub fn forward(&self, packet: Packet) {
        let next_hop = match packet.routing_header.next_hop() {
            Some(next_hop) => { next_hop },
            None => {
                // TODO: maybe panic is too much, maybe sending a nack to listener is enough? Maybe UnexpectedRecipient?
                panic!("No next hop")
            }
        };

        if let Some(channel) = self.neighbors.get(&next_hop) {
            self.send_on_channel_checked(channel, packet, next_hop);
        } else {
            self.send_nack_packet_to_receiver(packet, NackType::ErrorInRouting(next_hop));
        }
    }

    /// Sends a NACK to listener. Note that the only nack that this will send are just
    /// ErrorInRouting and (hopefully never) UnexpectedRecipient. There is no way that
    /// a Dropped or DestinationIsDrone gets sent, so there is no need to reverse the header
    /// or sending a nack for a specific fragment index
    fn send_nack_packet_to_receiver(&self, packet: Packet, nack_type: NackType) {
        let fragment_index = match &packet.pack_type {
            PacketType::MsgFragment(fragment) => {
                fragment.fragment_index
            }
            _ => 0
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
        match self.listener_channel.send(packet) {
            Ok(()) => {},
            Err(_err) => {
                panic!("Gateway cannot communicate with listener");
            }
        }
    }

    /// Adds a channel to the connected neighbors
    fn add_neighbor(&mut self, node_id: NodeId, channel: Sender<Packet>) {
        self.neighbors.insert(node_id, channel);
    }

    /// Removes a channel from the connected neighbors
    fn remove_neighbor(&mut self, node_id: &NodeId) {
        self.neighbors.remove(node_id);
    }
}

// TODO: update tests
/*
#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashMap;
    use crossbeam_channel::unbounded;
    use wg_2024::packet::{Ack, Packet};
    use wg_2024::packet::NackType::ErrorInRouting;

    #[test]
    fn create() {
        let (tx, rx) = unbounded::<Packet>();
        let gateway = Gateway::new(10, HashMap::new(), tx);

        assert_eq!(gateway.node_id, 10);
        assert_eq!(gateway.neighbors.len(), 0);
    }

    #[test]
    fn check_send_message_failure_error_in_routing() {
        let (tx, rx) = unbounded::<Packet>();
        let gateway = Gateway::new(10, HashMap::new(), tx);
        let packet = Packet {
            pack_type: PacketType::Ack(Ack{ fragment_index: 0 }),
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![10, 1, 2] },
            session_id: 0,
        };
        let result = gateway.forward(packet.clone());
        assert_eq!(result, Err(ErrorInRouting(1)));
    }

    #[test]
    fn check_send_message_successful() {
        let (tx, rx) = unbounded::<Packet>();
        let (tx_drone, rx_drone) = unbounded::<Packet>();
        let mut neighbors = HashMap::new();
        neighbors.insert(1, tx_drone);
        let gateway = Gateway::new(10, neighbors, tx);
        let packet = Packet {
            pack_type: PacketType::Ack(Ack{ fragment_index: 0 }),
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![10, 1, 2] },
            session_id: 0,
        };
        let result = gateway.forward(packet.clone());

        assert_eq!(result, Ok(()));

        let received = rx_drone.recv();

        let received_packet = received.unwrap();
        assert_eq!(packet.session_id, received_packet.session_id);
        match (packet.pack_type, received_packet.pack_type) {
            (PacketType::Ack(_), PacketType::Ack(_)) => { assert!(true) },
            _ => { assert!(false) }
        }
        assert_eq!(packet.routing_header.hop_index + 1, received_packet.routing_header.hop_index);
    }

    #[test]
    fn check_send_message_forward_to_receiver() {
        let (tx, rx) = unbounded::<Packet>();
        let gateway = Gateway::new(10, HashMap::new(), tx);
        let packet = Packet {
            pack_type: PacketType::Ack(Ack{ fragment_index: 0 }),
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![10] },
            session_id: 0,
        };
        let result = gateway.forward(packet.clone());

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn check_send_error_packet_to_receiver() {
        let (tx, rx) = unbounded::<Packet>();
        let gateway = Gateway::new(10, HashMap::new(), tx);
        let packet = Packet {
            pack_type: PacketType::Ack(Ack{ fragment_index: 0 }),
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![1, 2, 3, 4] },
            session_id: 0,
        };
        let result = gateway.send_nack_packet_to_receiver(&packet, NackType::Dropped);
        match result {
            Ok(()) => assert!(true),
            Err(_error) => assert!(false),
        }

        let received_nack = rx.recv();
        match &received_nack {
            Ok(_nack) => assert!(true),
            Err(_error) => assert!(false),
        }
        let received_nack_packet = received_nack.unwrap();
        match received_nack_packet.pack_type {
            PacketType::Nack(nack) => {
                assert_eq!(nack.nack_type, NackType::Dropped);
            }
            _ => {
                assert!(false)
            },
        }
    }

    #[test]
    fn check_add_neighbor() {
        let (tx, rx) = unbounded::<Packet>();
        let mut gateway = Gateway::new(10, HashMap::new(), tx);
        assert_eq!(gateway.neighbors.len(), 0);
        let (tx_drone_5, rx_drone_5) = unbounded::<Packet>();
        gateway.add_neighbor(5, tx_drone_5);
        assert_eq!(gateway.neighbors.len(), 1);
        let (tx_drone_5, rx_drone_5) = unbounded::<Packet>();
        gateway.add_neighbor(5, tx_drone_5);
        assert_eq!(gateway.neighbors.len(), 1);
        let (tx_drone_8, rx_drone_8) = unbounded::<Packet>();
        gateway.add_neighbor(8, tx_drone_8);
        assert_eq!(gateway.neighbors.len(), 2);
    }

    #[test]
    fn check_remove_neighbor() {
        let (tx, rx) = unbounded::<Packet>();
        let mut gateway = Gateway::new(10, HashMap::new(), tx);
        assert_eq!(gateway.neighbors.len(), 0);
        let (tx_drone_5, rx_drone_5) = unbounded::<Packet>();
        gateway.add_neighbor(5, tx_drone_5);
        assert_eq!(gateway.neighbors.len(), 1);
        let (tx_drone_5, rx_drone_5) = unbounded::<Packet>();
        gateway.add_neighbor(5, tx_drone_5);
        assert_eq!(gateway.neighbors.len(), 1);
        let (tx_drone_8, rx_drone_8) = unbounded::<Packet>();
        gateway.add_neighbor(8, tx_drone_8);
        assert_eq!(gateway.neighbors.len(), 2);
        gateway.remove_neighbor(&8);
        assert_eq!(gateway.neighbors.len(), 1);
        gateway.remove_neighbor(&5);
        assert_eq!(gateway.neighbors.len(), 0);
    }
}
 */