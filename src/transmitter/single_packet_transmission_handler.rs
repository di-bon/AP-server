use std::sync::Arc;
use std::thread;
use std::time::Duration;
use assembler::Assembler;
use messages::{MessageUtilities};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Packet, PacketType};
use crate::transmitter::gateway::Gateway;
use crate::transmitter::network_controller::NetworkController;

/// A `TransmissionHandler` struct that will handle the fragmentation and packet creation, sending
/// said packets to the gateway. All created packets will share the same `SourceRoutingHeader`,
/// unless it gets updated using the `update_source_routing_header` method
pub(super) struct SinglePacketTransmissionHandler {
    packet_type: PacketType,
    source_id: NodeId,
    session_id: u64,
    gateway: Arc<Gateway>,
    network_controller: Arc<NetworkController>,
    destination_node_id: NodeId,
    backoff_time: Duration,
}

impl SinglePacketTransmissionHandler {
    pub fn new(packet_type: PacketType, source_id: NodeId, session_id: u64, gateway: Arc<Gateway>, network_controller: Arc<NetworkController>, destination_node_id: NodeId, backoff_time: Duration) -> Self {
        Self { packet_type, source_id, session_id, gateway, network_controller, destination_node_id, backoff_time }
    }

    pub fn send_packet(&self) {
        let mut source_routing_header = self.find_new_routing_header();

       let packet = Packet {
           routing_header: source_routing_header,
           session_id: self.session_id,
           pack_type: self.packet_type.clone(),
       };

        self.gateway.forward(packet);
    }

    fn find_new_routing_header(&self) -> SourceRoutingHeader {
        loop {
            let hops = self.network_controller.get_path(self.destination_node_id);
            if let Some(hops) = hops {
                let source_routing_header = SourceRoutingHeader {
                    hop_index: 0,
                    hops,
                };
                return source_routing_header;
            } else {
                thread::sleep(self.backoff_time);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // TODO: add tests
}