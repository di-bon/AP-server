mod storer;

use crate::listener::storer::Storer;
use crossbeam_channel::{select, Receiver, SendError, Sender};
use std::collections::HashMap;
use wg_2024::network::NodeId;
use wg_2024::packet::{Ack, Fragment, Packet, PacketType};

/*
   TODO:
   - reassemble fragments: X
   - forward reassembled message to server logic: X
   - maybe add commands?
   - write tests
*/

struct Listener {
    node_id: NodeId,
    // listener -> transmitter
    tx_sender: Sender<Packet>, // this should only transmit packets of all types but PacketType::MsgFragment(Fragment)
    // transmitter -> listener
    tx_receiver: Receiver<Packet>, // internal channel for error propagation
    // listener -> server_logic
    server_logic_channel: Sender<Packet>, // this should only transmit reassembled messages -> its type is high level message, not packet!
    // drone(s) -> listener
    drone_channel: Receiver<Packet>,
    storers: HashMap<u64, Storer>,
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
            tx_receiver,
            server_logic_channel,
            drone_channel,
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
                            if matches!(packet.pack_type, PacketType::MsgFragment(_)) {
                                log::warn!("Received a message fragment from self.tx_receiver. This should not happen. Ignoring fragment");
                                continue;
                            }
                            // this kind of packets (ACKs, NACKs, FloodRequest, FloodResponse) should be directly
                            // forwarded to transmitter to be processed
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
        let storer = self.storers.get_mut(&session_id);
        match storer {
            Some(storer) => {
                storer.insert_fragment(fragment);
            }
            None => {
                let storer = Storer::new_from_fragment(fragment);
                self.storers.insert(session_id, storer);
            }
        }
    }

    fn process_drone_packet(&mut self, packet: Packet) {
        match packet.pack_type {
            PacketType::MsgFragment(ref fragment) => {
                let session_id = packet.session_id;
                match self.tx_sender.send(packet.clone()) {
                    Ok(()) => {}
                    Err(err) => {
                        panic!("Listener cannot communicate to transmitter");
                    }
                }

                self.store_fragment(session_id, fragment.clone());
                let storer = self.storers.get(&session_id);
                match storer {
                    Some(storer) => {
                        if storer.is_ready() {
                            let fragments = storer.get_fragments();
                            // TODO: call assembler to get a HL message
                            /*
                            TODO: fix this placeholder code with appropriate server_logic_channel message types
                            match self.server_logic_channel.send() {
                                Ok(()) => {
                                    self.storers.remove(&session_id);
                                },
                                Err(err) => {
                                    panic!("Listener cannot forward messages to server logic");
                                }
                            }
                             */
                        }
                    }
                    None => {
                        // TODO: maybe panic?
                        log::warn!("Storer for session {session_id} not found. At this point however it should exist");
                    }
                }

                // refactor this
                /*
                match self.check_storer(session_id) {
                    Some(true) => {
                        let fragments = self.storers.get(&session_id).unwrap().get_fragments();
                    }
                    _ => {}
                }
                */
            }
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
            Ok(()) => {}
            Err(err) => {
                panic!("Listener cannot send internal message to transmitter");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use wg_2024::network::SourceRoutingHeader;
    use wg_2024::packet::{Ack, Packet, PacketType};

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
            tx_receiver,
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

    #[test]
    fn forward_packet_to_transmitter_ok() {
        let (tx_sender, tx_receiver) = unbounded::<Packet>();
        let (_drones_sender, drones_receiver) = unbounded::<Packet>();
        let (server_logic_sender, _server_logic_receiver) = unbounded::<Packet>();

        let listener = Listener::new(
            1,
            tx_sender,
            server_logic_sender,
            drones_receiver,
            tx_receiver.clone(),
        );

        let packet = Packet {
            pack_type: PacketType::Ack(Ack { fragment_index: 0 }),
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![],
            },
            session_id: 0,
        };

        listener.forward_packet_to_transmitter(packet);

        let expected = Packet {
            pack_type: PacketType::Ack(Ack { fragment_index: 0 }),
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![],
            },
            session_id: 0,
        };

        select! {
            recv(tx_receiver) -> packet => {
                if let Ok(packet) = packet {
                    assert_eq!(packet, expected);
                    return;
                } else {
                    assert!(false);
                }
            }
        }
        assert!(false);
    }
}
