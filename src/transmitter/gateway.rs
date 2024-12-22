use std::collections::HashMap;
use crossbeam_channel::{SendError, Sender, TrySendError};
use wg_2024::network::{NodeId, SourceRoutingHeader};
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

    pub fn send_flood(&self, packet: Packet) {
        for (node_id, channel) in &self.neighbors {
            self.send_on_channel_checked(channel, packet.clone(), *node_id);
        }
    }

    /// Sends a packet on the given channel. If channel.send fails, it sends an ErrorInRouting back to listener
    fn send_on_channel_checked(&self, channel: &Sender<Packet>, packet: Packet, next_hop: NodeId) {
        match channel.send(packet) {
            Ok(()) => {},
            Err(SendError(packet)) => {
                self.send_nack_packet_to_receiver(&packet, NackType::ErrorInRouting(next_hop));
            }
        }
    }

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

    // TODO: maybe this method is kinda useless, it is the same thing of calling channel.send directly
    /*
    fn send_on_channel(&self, channel: &Sender<Packet>, packet: Packet) -> Result<(), SendError<Packet>> {
        channel.send(packet)
        /*
        match channel.send(packet) {
            Ok(()) => { },
            Err(err) => {
                self.send_nack_packet_to_receiver(&packet, NackType::ErrorInRouting(next_hop)); // next hop cannot be retrieved from packet header since this function may also forward flood requests
                // TODO: send nack to remove crashed drone
            }
        }
         */
    }
     */

    // TODO: update forward method to just forward packets without updating hop_index (so, assume it is
    // always 1 when receiving a packet. Also, don't make so many checks, just forward the packet
    // Also, use the send_on_channel method to further modularize the code
    // TODO: should this return a Result<(), ()> or can it just panic?
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
            self.send_nack_packet_to_receiver(&packet, NackType::ErrorInRouting(next_hop));
        }

        /*
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

         */
    }

    /// Sends a nack to listener. Note that the only nack that this will send are just
    /// ErrorInRouting and (hopefully never) UnexpectedRecipient. There is no way that
    /// a Dropped or DestinationIsDrone gets sent, so there is no need to reverse the header
    /// or sending a nack for a specific fragment index
    fn send_nack_packet_to_receiver(&self, packet: &Packet, nack_type: NackType) {
        /*
        // Useless
        let fragment_index = match &packet.pack_type {
            PacketType::MsgFragment(fragment) => {
                fragment.fragment_index
            }
            _ => 0
        };
         */
        let fragment_index = 0;
        let nack = Nack {
            fragment_index,
            nack_type,
        };

        // TODO: properly initialize routing_header? Decide this base on how transmitter handles the NACKs
        // these NACKs in server can only be generated by the server itself, so it is sufficient
        // to set hops to just 'vec![self.node_id]' to properly handle NACKs. Maybe the routing header isn't even needed?
        let packet = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![],
            },
            session_id: 0,
            pack_type: PacketType::Nack(nack),
        };

        self.send_to_listener(packet);

        /*
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
            // TODO: properly initialize routing_header
            // these NACKs in server can only be generated by the server itself, to it is sufficient
            // to set hops to just 'vec![self.node_id]' to properly handle NACKs
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![self.node_id] },
            session_id: packet.session_id,
        };
        self.receiver_channel.try_send(packet)
         */
    }

    pub fn send_to_listener(&self, packet: Packet) {
        match self.listener_channel.send(packet) {
            Ok(()) => {},
            Err(_err) => {
                panic!("Gateway cannot communicate with listener");
            }
        }
    }

    fn add_neighbor(&mut self, node_id: NodeId, channel: Sender<Packet>) {
        self.neighbors.insert(node_id, channel);
    }

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