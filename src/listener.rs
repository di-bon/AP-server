mod storer;

use crate::listener::storer::Storer;
use assembler::naive_assembler::NaiveAssembler;
use assembler::Assembler;
use crossbeam_channel::{select, Receiver, SendError, Sender};
use messages::node_event::NodeEvent;
use std::collections::HashMap;
use std::sync::Arc;
use messages::{Message, MessageType};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, NackType, Packet, PacketType};
use messages::MessageUtilities;
use crate::simulation_controller_notifier::SimulationControllerNotifier;
/*
   TODO:
   - write tests
*/

pub enum ListenerCommand {
    Quit,
}

#[derive(Debug, Clone)]
pub struct Listener {
    node_id: NodeId,
    // listener -> transmitter
    transmitter_tx: Sender<Packet>, // this should only transmit packets of all types but PacketType::MsgFragment(Fragment)
    // transmitter -> listener
    transmitter_rx: Receiver<Packet>, // internal channel for error propagation
    // se -> server_logic
    server_logic_tx: Sender<Message>, // this should only transmit reassembled messages -> its type is high level message, not packet!
    // drone(s) -> listener
    drones_rx: Receiver<Packet>,
    command_rx: Receiver<ListenerCommand>,
    simulation_controller_notifier: Arc<SimulationControllerNotifier>,
    // HashMap containing all the pairs (session_id, Storer)
    storers: HashMap<u64, Storer>,
}

impl PartialEq for Listener {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id && self.storers == other.storers
    }
}

impl Listener {
    pub fn new(
        node_id: NodeId,
        transmitter_tx: Sender<Packet>,
        transmitter_rx: Receiver<Packet>,
        server_logic_tx: Sender<Message>,
        drones_rx: Receiver<Packet>,
        command_rx: Receiver<ListenerCommand>,
        simulation_controller_notifier: Arc<SimulationControllerNotifier>,
    ) -> Self {
        Self {
            node_id,
            transmitter_tx,
            transmitter_rx,
            server_logic_tx,
            drones_rx,
            command_rx,
            simulation_controller_notifier,
            storers: HashMap::default(),
        }
    }

    pub fn run(&mut self) {
        loop {
            select! {
                recv(self.drones_rx) -> packet => {
                    match packet {
                        Ok(packet) => {
                            log::info!("Received packet {packet}");
                            // TODO: send PacketReceived
                            self.process_drone_packet(packet);
                        },
                        Err(err) => {
                            panic!("Listener cannot receive packets from drones channel");
                        }
                    }
                },
                recv(self.transmitter_rx) -> packet => {
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
                recv(self.command_rx) -> command => {
                    if let Ok(command) = command {
                        match command {
                            ListenerCommand::Quit => break,
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

                let current_hop_id = match packet.routing_header.current_hop() {
                    Some(id) => id,
                    None => {
                        // TODO: don't panic, send nack back and continue?
                        panic!("Malformed routing header")
                    }
                };

                let wrong_destination = current_hop_id != self.node_id;

                if !packet.routing_header.is_last_hop() || wrong_destination {
                    let nack = Nack {
                        fragment_index: fragment.fragment_index,
                        nack_type: NackType::UnexpectedRecipient(self.node_id),
                    };
                    let nack = Packet {
                        routing_header: Default::default(),
                        session_id,
                        pack_type: PacketType::Nack(nack),
                    };
                    match self.transmitter_tx.send(nack) {
                        Ok(()) => {
                            log::info!("Send nack packet (UnexpectedRecipient) for fragment {} with session_id {session_id}", fragment.fragment_index);
                        }
                        Err(err) => {
                            log::warn!("Cannot communicate with transmitter to send NACK packet");
                            panic!("Listener cannot communicate with transmitter using the internal channel");
                        }
                    }
                    return;
                }

                // this communication starts the ACK generation for the received fragment.
                // the logic is handled by the transmitter
                match self.transmitter_tx.send(packet.clone()) {
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
                            log::info!(
                                "Storer for session {session_id} is ready for message reassemble"
                            );
                            let fragments = storer.get_fragments();
                            let message = NaiveAssembler::reassemble(&fragments);
                            let message = String::from_utf8(message).unwrap();
                            let message: Message = MessageUtilities::from_string(message).unwrap();
                            log::info!("Reassembled message in bytes: {message:?}");
                            self.storers.remove(&session_id);

                            match self.server_logic_tx.send(message.clone()) {
                                Ok(()) => {
                                    log::info!("Listener successfully forwarded message {message:?} to server logic");
                                },
                                Err(err) => {
                                    panic!("Listener cannot forward messages to server logic");
                                }
                            }
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
        match self.transmitter_tx.send(packet) {
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
    use super::*;
    use crossbeam_channel::unbounded;
    use ntest::timeout;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::thread::sleep;
    use std::time::Duration;
    use messages::{RequestType, TextRequest};
    use wg_2024::network::SourceRoutingHeader;
    use wg_2024::packet::{
        Ack, FloodRequest, FloodResponse, Nack, NackType, NodeType, Packet, PacketType,
    };

    fn create_listener_and_channels(
        node_id: NodeId,
    ) -> (
        Listener,
        Sender<Packet>,
        Receiver<Packet>,
        Receiver<Message>,
        Sender<ListenerCommand>,
        Sender<Packet>,
        Receiver<NodeEvent>,
    ) {
        let (internal_transmitter_to_listener_tx, internal_transmitter_to_listener_rx) =
            unbounded::<Packet>();
        let (internal_listener_to_transmitter_tx, internal_listener_to_transmitter_rx) =
            unbounded::<Packet>();
        let (internal_listener_to_server_logic_tx, internal_listener_to_server_logic_rx) =
            unbounded::<Message>();
        let (listener_commands_tx, listener_commands_rx) = unbounded::<ListenerCommand>();
        let (listener_public_tx, listener_public_rx) = unbounded::<Packet>();

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let listener = Listener::new(
            node_id,
            internal_listener_to_transmitter_tx,
            internal_transmitter_to_listener_rx,
            internal_listener_to_server_logic_tx,
            listener_public_rx,
            listener_commands_rx,
            simulation_controller_notifier,
        );

        (
            listener,
            internal_transmitter_to_listener_tx,
            internal_listener_to_transmitter_rx,
            internal_listener_to_server_logic_rx,
            listener_commands_tx,
            listener_public_tx,
            simulation_controller_rx,
        )
    }

    #[test]
    fn initialize() {
        let (
            listener,
            internal_transmitter_to_listener_tx,
            internal_listener_to_transmitter_rx,
            internal_listener_to_server_logic_rx,
            listener_commands_tx,
            listener_public_tx,
            simulation_controller_rx,
        ) = create_listener_and_channels(1);

        let (transmitter_tx, transmitter_rx) = unbounded::<Packet>();
        let (drones_tx, drones_rx) = unbounded::<Packet>();
        let (server_logic_tx, _server_logic_rx) = unbounded::<Message>();
        let (command_tx, command_rx) = unbounded::<ListenerCommand>();
        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let expected = Listener {
            node_id: 1,
            transmitter_tx,
            server_logic_tx,
            drones_rx,
            transmitter_rx,
            command_rx,
            simulation_controller_notifier,
            storers: Default::default(),
        };

        assert_eq!(listener.node_id, expected.node_id);
        assert_eq!(listener.storers.len(), expected.storers.len());
    }

    #[test]
    fn check_storer() {
        let (
            mut listener,
            internal_transmitter_to_listener_tx,
            internal_listener_to_transmitter_rx,
            internal_listener_to_server_logic_rx,
            listener_commands_tx,
            listener_public_tx,
            simulation_controller_rx,
        ) = create_listener_and_channels(1);
        let (transmitter_tx, transmitter_rx) = unbounded::<Packet>();
        let (drones_tx, drones_rx) = unbounded::<Packet>();
        let (server_logic_tx, _server_logic_rx) = unbounded::<Message>();
        let (command_tx, command_rx) = unbounded::<ListenerCommand>();
        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let mut expected = Listener {
            node_id: 1,
            transmitter_tx,
            server_logic_tx,
            drones_rx,
            transmitter_rx,
            command_rx,
            simulation_controller_notifier,
            storers: Default::default(),
        };

        assert_eq!(listener, expected);

        let session_id = 0;
        let fragments = vec![
            Fragment {
                fragment_index: 0,
                total_n_fragments: 3,
                length: 128,
                data: [0; 128],
            },
            Fragment {
                fragment_index: 1,
                total_n_fragments: 3,
                length: 128,
                data: [0; 128],
            },
            Fragment {
                fragment_index: 2,
                total_n_fragments: 3,
                length: 128,
                data: [0; 128],
            },
        ];

        assert_eq!(listener.check_storer(session_id), None);

        listener.store_fragment(session_id, fragments[0].clone());

        let mut expected_storers = HashMap::new();
        let expected_storer = Storer::new_from_fragment(fragments[0].clone());
        expected_storers.insert(session_id, expected_storer);
        expected.storers = expected_storers;

        assert_eq!(listener, expected);
        assert_eq!(listener.check_storer(session_id), Some(false));

        listener.store_fragment(session_id, fragments[1].clone());
        listener.store_fragment(session_id, fragments[2].clone());

        let storer = expected.storers.get_mut(&session_id).unwrap();
        storer.insert_fragment(fragments[1].clone());
        storer.insert_fragment(fragments[2].clone());

        assert_eq!(listener, expected);

        assert_eq!(listener.check_storer(session_id), Some(true));
    }

    #[test]
    fn forward_packet_to_transmitter_ok() {
        let (
            listener,
            internal_transmitter_to_listener_tx,
            internal_listener_to_transmitter_rx,
            internal_listener_to_server_logic_rx,
            listener_commands_tx,
            listener_public_tx,
            simulation_controller_rx,
        ) = create_listener_and_channels(1);

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
            recv(internal_listener_to_transmitter_rx) -> packet => {
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
    #[timeout(2000)]
    fn store_fragment_successful() {
        let (
            listener,
            _internal_transmitter_to_listener_tx,
            _internal_listener_to_transmitter_rx,
            _internal_listener_to_server_logic_rx,
            listener_commands_tx,
            listener_public_tx,
            _simulation_controller_rx,
        ) = create_listener_and_channels(1);
        let listener = Arc::new(Mutex::new(listener));
        let listener_clone = Arc::clone(&listener);

        let _ = thread::spawn(move || {
            let mut listener = listener_clone.lock().unwrap();
            listener.run()
        });

        assert_eq!(listener.lock().unwrap().storers.len(), 0);

        let fragment = Fragment {
            fragment_index: 0,
            total_n_fragments: 2,
            length: 128,
            data: [0; 128],
        };

        let fragment_packet = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![1],
            },
            session_id: 10,
            pack_type: PacketType::MsgFragment(fragment.clone()),
        };
        let _ = listener_public_tx.send(fragment_packet.clone());

        sleep(Duration::from_millis(200));
        let _ = listener_commands_tx.send(ListenerCommand::Quit);

        let listener = listener.lock().unwrap();

        assert_eq!(listener.storers.len(), 1);
        let storer = listener.storers.get(&10).unwrap();
        assert!(!storer.is_ready());

        let fragments = storer.get_fragments();
        let expected_fragments = vec![fragment];
        assert_eq!(fragments, expected_fragments);
    }

    #[test]
    #[timeout(2000)]
    fn receive_ack() {
        let (
            listener,
            _internal_transmitter_to_listener_tx,
            internal_listener_to_transmitter_rx,
            _internal_listener_to_server_logic_rx,
            _listener_commands_tx,
            listener_public_tx,
            _simulation_controller_rx,
        ) = create_listener_and_channels(1);

        let listener = Arc::new(Mutex::new(listener));
        let listener_clone = Arc::clone(&listener);

        let _ = thread::spawn(move || {
            let mut listener = listener_clone.lock().unwrap();
            listener.run()
        });

        let ack = Ack { fragment_index: 0 };
        let ack = Packet {
            routing_header: Default::default(),
            session_id: 0,
            pack_type: PacketType::Ack(ack),
        };

        let _ = listener_public_tx.send(ack.clone());

        let received = internal_listener_to_transmitter_rx.recv().unwrap();

        assert_eq!(received, ack);
    }

    #[test]
    #[timeout(2000)]
    fn receive_nack() {
        let (
            listener,
            _internal_transmitter_to_listener_tx,
            internal_listener_to_transmitter_rx,
            _internal_listener_to_server_logic_rx,
            _listener_commands_tx,
            listener_public_tx,
            _simulation_controller_rx,
        ) = create_listener_and_channels(0);

        let listener = Arc::new(Mutex::new(listener));
        let listener_clone = Arc::clone(&listener);

        let _ = thread::spawn(move || {
            let mut listener = listener_clone.lock().unwrap();
            listener.run()
        });

        let nack = Nack {
            fragment_index: 0,
            nack_type: NackType::ErrorInRouting(1),
        };
        let nack = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![0],
            },
            session_id: 0,
            pack_type: PacketType::Nack(nack),
        };

        let _ = listener_public_tx.send(nack.clone());

        let received = internal_listener_to_transmitter_rx.recv().unwrap();

        assert_eq!(received, nack);
    }

    #[test]
    #[timeout(2000)]
    fn receive_flood_request() {
        let (
            listener,
            _internal_transmitter_to_listener_tx,
            internal_listener_to_transmitter_rx,
            _internal_listener_to_server_logic_rx,
            _listener_commands_tx,
            listener_public_tx,
            _simulation_controller_rx,
        ) = create_listener_and_channels(0);

        let listener = Arc::new(Mutex::new(listener));
        let listener_clone = Arc::clone(&listener);

        let _ = thread::spawn(move || {
            let mut listener = listener_clone.lock().unwrap();
            listener.run()
        });

        let flood_request = FloodRequest {
            flood_id: 10,
            initiator_id: 5,
            path_trace: vec![(10, NodeType::Client), (4, NodeType::Drone)],
        };
        let flood_request = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![0],
            },
            session_id: 0,
            pack_type: PacketType::FloodRequest(flood_request),
        };

        let _ = listener_public_tx.send(flood_request.clone());

        let received = internal_listener_to_transmitter_rx.recv().unwrap();

        assert_eq!(received, flood_request);
    }

    #[test]
    #[timeout(2000)]
    fn receive_flood_response() {
        let (
            listener,
            _internal_transmitter_to_listener_tx,
            internal_listener_to_transmitter_rx,
            _internal_listener_to_server_logic_rx,
            _listener_commands_tx,
            listener_public_tx,
            _simulation_controller_rx,
        ) = create_listener_and_channels(0);

        let listener = Arc::new(Mutex::new(listener));
        let listener_clone = Arc::clone(&listener);

        let _ = thread::spawn(move || {
            let mut listener = listener_clone.lock().unwrap();
            listener.run()
        });

        let flood_response = FloodResponse {
            flood_id: 10,
            path_trace: vec![(10, NodeType::Client), (4, NodeType::Drone)],
        };
        let flood_response = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![0],
            },
            session_id: 0,
            pack_type: PacketType::FloodResponse(flood_response),
        };

        let _ = listener_public_tx.send(flood_response.clone());

        let received = internal_listener_to_transmitter_rx.recv().unwrap();

        assert_eq!(received, flood_response);
    }

    #[test]
    #[timeout(2000)]
    fn receive_message_fragments() {
        let (
            listener,
            _internal_transmitter_to_listener_tx,
            internal_listener_to_transmitter_rx,
            internal_listener_to_server_logic_rx,
            listener_commands_tx,
            listener_public_tx,
            _simulation_controller_rx,
        ) = create_listener_and_channels(0);

        let listener = Arc::new(Mutex::new(listener));
        let listener_clone = Arc::clone(&listener);

        let _ = thread::spawn(move || {
            let mut listener = listener_clone.lock().unwrap();
            listener.run()
        });

        let message = Message {
            source_id: 10,
            session_id: 10,
            content: MessageType::Request(RequestType::TextRequest(TextRequest::TextList)),
        };
        let fragments = NaiveAssembler::disassemble(&message.stringify().into_bytes());

        for fragment in &fragments {
            let fragment = fragment.clone();
            let packet = Packet {
                routing_header: SourceRoutingHeader {
                    hop_index: 0,
                    hops: vec![0],
                },
                session_id: 0,
                pack_type: PacketType::MsgFragment(fragment.clone()),
            };

            let _ = listener_public_tx.send(packet.clone());

            let received = internal_listener_to_transmitter_rx.recv().unwrap();

            assert_eq!(received, packet);
        }

        let _ = listener_commands_tx.send(ListenerCommand::Quit);

        let received = internal_listener_to_server_logic_rx.recv().unwrap();
        assert_eq!(received, message);
    }

    /*
     TODO: test that need to be done:
     - receiving different types of packets from tx channel
    */
}
