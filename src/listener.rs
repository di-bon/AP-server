mod storer;

use crate::listener::storer::Storer;
use crossbeam_channel::{select, Receiver, Sender};
use std::collections::HashMap;
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Packet, PacketType};

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
    command_channel: Receiver<bool>,
    storers: HashMap<u64, Storer>,
}

impl Listener {
    fn new(
        node_id: NodeId,
        tx_sender: Sender<Packet>,
        server_logic_channel: Sender<Packet>,
        drone_channel: Receiver<Packet>,
        command_channel: Receiver<bool>,
        tx_receiver: Receiver<Packet>,
    ) -> Self {
        Self {
            node_id,
            tx_sender,
            tx_receiver,
            server_logic_channel,
            drone_channel,
            command_channel,
            storers: HashMap::default(),
        }
    }

    fn run(&mut self) {
        loop {
            select! {
                recv(self.drone_channel) -> packet => {
                    match packet {
                        Ok(packet) => {
                            log::info!("Received packet {packet}");
                            self.process_drone_packet(packet);
                        },
                        Err(err) => {
                            panic!("Listener cannot receive packets from drones channel");
                        }
                    }
                },
                recv(self.tx_receiver) -> packet => {
                    match packet {
                        Ok(packet) => {
                            log::info!("Received packet {packet}");
                            if matches!(packet.pack_type, PacketType::MsgFragment(_)) {
                                log::warn!("Received a message fragment from self.tx_receiver. This should not happen. Ignoring fragment");
                                continue;
                            }
                            // this kind of packets (ACKs, NACKs, FloodRequest, FloodResponse) should be directly
                            // forwarded to transmitter to be processed
                            self.forward_packet_to_transmitter(packet);
                        },
                        Err(err) => {
                            panic!("Listener cannot receive packets from internal transmitter channel");
                        }
                    }
                },
                recv(self.command_channel) -> exit => {
                    if let Ok(exit) = exit {
                        if exit {
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Checks the readiness for the `Storer` associated to the `session_id`. Returns `None` if there is no `Storer` associated to the given `session_id`
    fn check_storer(&self, session_id: u64) -> Option<bool> {
        let storer = self.storers.get(&session_id)?;
        Some(storer.is_ready())
    }

    /// Stores a `Fragment` into the `Storer` for the given `session_id`
    fn store_fragment(&mut self, session_id: u64, fragment: Fragment) {
        let storer = self.storers.get_mut(&session_id);
        match storer {
            Some(storer) => {
                log::info!("Storing fragment {fragment} into storer");
                storer.insert_fragment(fragment);
            }
            None => {
                log::info!("Creating a new storer for fragment {fragment}");
                let storer = Storer::new_from_fragment(fragment);
                self.storers.insert(session_id, storer);
            }
        }
    }

    /// Processes a `Packet` received from the connected drones based on the `PacketType`
    fn process_drone_packet(&mut self, packet: Packet) {
        match packet.pack_type {
            PacketType::MsgFragment(ref fragment) => {
                log::info!("Processing a message fragment");
                let session_id = packet.session_id;

                // this communication starts the ACK generation for the received fragment.
                // the logic is handled by the transmitter
                match self.tx_sender.send(packet.clone()) {
                    Ok(()) => {
                        log::info!("Fragment sent to transmitter to generate its ACK packet");
                    }
                    Err(err) => {
                        log::warn!("Cannot communicate with transmitter to generate an ACK packet");
                        panic!("Listener cannot communicate with transmitter using the internal channel");
                    }
                }

                self.store_fragment(session_id, fragment.clone());
                let storer = self.storers.get(&session_id);
                match storer {
                    Some(storer) => {
                        if storer.is_ready() {
                            log::info!("Storer for session {session_id} is ready for message reassemble");
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
            }
            PacketType::Nack(_)
            | PacketType::Ack(_)
            | PacketType::FloodRequest(_)
            | PacketType::FloodResponse(_) => {
                log::info!("Forwarding a not message fragment to transmitter");
                self.forward_packet_to_transmitter(packet);
            }
        }
    }

    /// Forwards a `Packet` to `Transmitter`
    /// If a `PacketType::MsgFragment` is forwarded, the relative `ACK` will be generated and sent
    /// If another `PacketType` is forwarded, the `Transmitter` will update the network graph accordingly
    fn forward_packet_to_transmitter(&self, packet: Packet) {
        match self.tx_sender.send(packet) {
            Ok(()) => {
                log::info!("Packet successfully forwarded to transmitter");
            }
            Err(err) => {
                log::warn!("Couldn't forward packet to transmitter");
                panic!("Listener cannot send internal message to transmitter");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::thread::sleep;
    use std::time::Duration;
    use super::*;
    use crossbeam_channel::unbounded;
    use futures::task::SpawnExt;
    use ntest::timeout;
    use wg_2024::network::SourceRoutingHeader;
    use wg_2024::packet::{Ack, Packet, PacketType};

    fn create_listener_and_channels(node_id: NodeId) -> (Listener, Sender<Packet>, Receiver<Packet>, Receiver<Packet>, Sender<bool>) {
        let (tx_sender, tx_receiver) = unbounded::<Packet>();
        let (drones_sender, drones_receiver) = unbounded::<Packet>();
        let (server_logic_sender, server_logic_receiver) = unbounded::<Packet>();
        let (command_tx, command_rx) = unbounded::<bool>();

        let listener = Listener::new(
            node_id,
            tx_sender,
            server_logic_sender,
            drones_receiver,
            command_rx,
            tx_receiver.clone(),
        );

        (listener, drones_sender, server_logic_receiver, tx_receiver, command_tx)
    }

    #[test]
    fn initialize() {
        let (listener, _, _, tx_receiver, command_tx) = create_listener_and_channels(1);

        let (tx_sender, tx_receiver) = unbounded::<Packet>();
        let (_drones_sender, drones_receiver) = unbounded::<Packet>();
        let (server_logic_sender, _server_logic_receiver) = unbounded::<Packet>();
        let (command_tx, command_rx) = unbounded::<bool>();
        let expected = Listener {
            node_id: 1,
            tx_sender,
            server_logic_channel: server_logic_sender,
            drone_channel: drones_receiver,
            tx_receiver,
            command_channel: command_rx,
            storers: Default::default(),
        };

        assert_eq!(listener.node_id, expected.node_id);
        assert_eq!(listener.storers.len(), expected.storers.len());
    }

    #[test]
    fn forward_packet_to_transmitter_ok() {
        let (listener, _, _, tx_receiver, command_tx) = create_listener_and_channels(1);

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


    #[test]
    #[timeout(1000)]
    fn store_fragment_successful() {
        let (listener, drone_tx, _, tx_receiver, command_tx) = create_listener_and_channels(1);
        let listener = Arc::new(Mutex::new(listener));
        let listener_clone = Arc::clone(&listener);

        let _ = thread::spawn(move || {
            let mut listener = listener_clone.lock().unwrap();
            listener.run()
        });

        assert_eq!(listener.lock().unwrap().storers.len(), 0);

        let fragment_packet = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![],
            },
            session_id: 10,
            pack_type: PacketType::MsgFragment(Fragment{
                fragment_index: 0,
                total_n_fragments: 2,
                length: 80,
                data: [0; 128],
            }),
        };
        let _ = drone_tx.send(fragment_packet.clone());

        sleep(Duration::from_millis(200));
        let _ = command_tx.send(true);

        let storers = listener.lock().unwrap();

        assert_eq!(storers.storers.len(), 1);
        let storer = storers.storers.get(&10).unwrap();
        assert!(!storer.is_ready());
    }

    /*
     TODO: test that need to be done:
     - receiving different types of packets from drone channel
     - receiving different types of packets from tx channel
    */
}
