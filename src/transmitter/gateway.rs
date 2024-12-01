use std::collections::HashMap;
use crossbeam_channel::{Sender, TrySendError};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Nack, NackType, Packet, PacketType};

#[derive(Debug)]
pub struct Gateway {
    node_id: NodeId,
    neighbors: HashMap<NodeId, Sender<Packet>>,
    receiver_channel: Sender<Packet>,
}

impl Gateway {
    pub fn new(node_id: NodeId, neighbors: HashMap<NodeId, Sender<Packet>>, receiver_channel: Sender<Packet>) -> Self {
        Self {
            node_id,
            neighbors,
            receiver_channel,
        }
    }

    pub fn forward(&self, mut packet: Packet) -> Result<(), NackType> {
        packet.routing_header.hop_index += 1;
        let hop_index = packet.routing_header.hop_index;
        let next_hop = packet.routing_header.hops.get(hop_index);
        match next_hop {
            Some(next_node) => {
                match self.neighbors.get(next_node) {
                    Some(next_node_channel) => {
                        let _ = next_node_channel.try_send(packet);
                        Ok(())
                    },
                    None => {
                        let nack_type = NackType::ErrorInRouting(*next_node);
                        // send error in routing to receiver to handle a possible network crash or wrong routing header
                        match self.send_nack_packet_to_receiver(&packet, nack_type.clone()) {
                            Ok(()) => {},
                            Err(error) => panic!("Gateway to receiver internal channel is disconnected: {error:?}"), // TODO: document this panic
                        }
                        Err(nack_type)
                    }
                }
            },
            None => {
                // if the match expression returns None, it means that the current node (i.e. server)
                // is the designed destination
                // so, forward the packet to Receiver service to handle that logic
                match self.receiver_channel.try_send(packet) { // TODO: consider add panic!() if the receiver channel is disconnected, which is a state that cannot be recovered
                    Ok(()) => Ok(()),
                    Err(error) => panic!("Gateway to receiver internal channel is disconnected: {error:?}"), // TODO: document this panic
                }

                // this logic should be handled by receiver
                // let nack_type = NackType::UnexpectedRecipient(self.node_id);
                // let _ = self.send_error_packet_to_receiver(&packet, nack_type.clone());
                // Err(nack_type)
            }
        }
    }

    fn send_nack_packet_to_receiver(&self, packet: &Packet, nack_type: NackType) -> Result<(), TrySendError<Packet>> {
        let nack = Nack {
            fragment_index: match &packet.pack_type {
                PacketType::MsgFragment(fragment) => {
                    fragment.fragment_index
                }
                _ => 0
            },
            nack_type: nack_type.clone(),
        };
        let packet = Packet {
            pack_type: PacketType::Nack(nack),
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![] }, // TODO: properly initialize routing_header
            session_id: packet.session_id,
        };
        self.receiver_channel.try_send(packet)
    }

    fn add_neighbor(&mut self, node_id: NodeId, channel: Sender<Packet>) {
        self.neighbors.insert(node_id, channel);
    }

    fn remove_neighbor(&mut self, node_id: &NodeId) {
        self.neighbors.remove(node_id);
    }
}

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