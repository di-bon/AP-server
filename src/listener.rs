use std::collections::HashMap;
use crossbeam_channel::{Sender, Receiver};
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;

struct Listener {
    tx_channel: Sender<Packet>, // this should only transmit packets of all types but PacketType::MsgFragment(Fragment)
    server_logic_channel: Sender<Packet>, // this should only transmit reassembled messages
    connected_drones: HashMap<NodeId, Receiver<Packet>>
}

impl Listener {
    // TODO: add parameters
    fn new() -> Self {
        todo!()
    }

    fn run(&self) {
        todo!()
    }
}