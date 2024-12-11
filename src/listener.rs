mod storer;

use std::collections::HashMap;
use crossbeam_channel::{Sender, Receiver, select};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Packet, PacketType};
use crate::listener::storer::Storer;

/*
    TODO:
    - reassemble fragments
    - forward reassembled message to server logic
    - maybe add commands?
 */

struct Listener {
    node_id: NodeId,
    tx_sender: Sender<Packet>, // this should only transmit packets of all types but PacketType::MsgFragment(Fragment)
    server_logic_channel: Sender<Packet>, // this should only transmit reassembled messages -> its type is high level message, not packet!
    drone_channel: Receiver<Packet>,
    tx_receiver: Receiver<Packet>,
    storers: HashMap<u64, Storer>
}

impl Listener {
    fn new(
        node_id: NodeId,
        tx_sender: Sender<Packet>,
        server_logic_channel: Sender<Packet>,
        drone_channel: Receiver<Packet>,
        tx_receiver: Receiver<Packet>,
    ) -> Self {
        Self {
            node_id,
            tx_sender,
            server_logic_channel,
            drone_channel,
            tx_receiver,
            storers: Default::default(),
        }
    }

    fn run(&mut self) {
        loop {
            select! {
                recv(self.drone_channel) -> packet => {
                    match packet {
                        Ok(packet) => {
                            self.process_drone_packet(packet);
                        },
                        Err(err) => {
                            panic!("Cannot receive from drones channel");
                        }
                    }
                },
                recv(self.tx_receiver) -> packet => {
                    match packet {
                        Ok(packet) => {
                            self.forward_packet_to_transmitter(packet);
                        },
                        Err(err) => {
                            panic!("Cannot receive from transmitter channel");
                        }
                    }
                },
            }
        }
    }

    fn check_storer(&self, session_id: u64) -> Option<bool> {
        let storer = self.storers.get(&session_id)?;
        Some(storer.is_ready())
    }

    fn store_fragment(&mut self, session_id: u64, fragment: Fragment) {
        let mut storer = self.storers.get_mut(&session_id);
        match storer {
            Some(storer) => {
                storer.insert_fragment(fragment);
            },
            None => {
                let storer = Storer::new_from_fragment(fragment);
                self.storers.insert(session_id, storer);
            }
        }
    }

    fn process_drone_packet(&mut self, packet: Packet) {
        match packet.pack_type {
            PacketType::MsgFragment(fragment) => {
                let session_id = packet.session_id;
                self.store_fragment(packet.session_id, fragment);
                // refactor this
                match self.check_storer(session_id) {
                    Some(true) => {
                        let fragments = self
                            .storers
                            .get(&session_id)
                            .unwrap()
                            .get_fragments();
                    }
                    _ => {}
                }
            },
            PacketType::Nack(_)
            | PacketType::Ack(_)
            | PacketType::FloodRequest(_)
            | PacketType::FloodResponse(_) => {
                self.forward_packet_to_transmitter(packet);
            }
        }
    }

    fn forward_packet_to_transmitter(&self, packet: Packet) {
        match self.tx_sender.send(packet) {
            Ok(()) => {},
            Err(err) => {
                panic!("Listener cannot send internal message to transmitter");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::unbounded;
    use wg_2024::packet::Packet;
    use crate::listener::Listener;

    #[test]
    fn initialize() {
        let (tx_sender, tx_receiver) = unbounded::<Packet>();
        let (_drones_sender, drones_receiver) = unbounded::<Packet>();
        let (server_logic_sender, _server_logic_receiver) = unbounded::<Packet>();

        let listener = Listener::new(
            1,
            tx_sender,
            server_logic_sender,
            drones_receiver,
            tx_receiver
        );

        let (tx_sender, tx_receiver) = unbounded::<Packet>();
        let (_drones_sender, drones_receiver) = unbounded::<Packet>();
        let (server_logic_sender, _server_logic_receiver) = unbounded::<Packet>();
        let expected = Listener {
            node_id: 1,
            tx_sender,
            server_logic_channel: server_logic_sender,
            drone_channel: drones_receiver,
            tx_receiver,
            storers: Default::default(),
        };

        assert_eq!(listener.node_id, expected.node_id);
    }
}