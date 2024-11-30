use std::collections::HashMap;
use crossbeam_channel::{SendError, Sender, TrySendError};
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
                        let _ = self.send_error_packet_to_receiver(&packet, nack_type.clone());
                        Err(nack_type)
                    }
                }
            },
            None => {
                // if the match expression returns None, it means that the current drone
                // is the designed destination
                let nack_type = NackType::UnexpectedRecipient(self.node_id);
                let _ = self.send_error_packet_to_receiver(&packet, nack_type.clone());
                Err(nack_type)
            }
        }
    }

    fn send_error_packet_to_receiver(&self, packet: &Packet, nack_type: NackType) -> Result<(), TrySendError<Packet>> {
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
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![] },
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