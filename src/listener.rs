mod storer;

use crate::listener::storer::Storer;
use assembler::naive_assembler::NaiveAssembler;
use assembler::Assembler;
use crossbeam_channel::{select, Receiver, SendError, Sender};
use messages::node_event::NodeEvent;
use std::collections::HashMap;
use std::sync::Arc;
use messages::Message;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Fragment, Nack, NackType, Packet, PacketType};
use messages::MessageUtilities;
use crate::simulation_controller_notifier::SimulationControllerNotifier;
use crate::transmitter::TransmitterInternalCommand;

pub enum ListenerCommand {
    Quit,
}

#[derive(Debug, Clone)]
pub struct Listener {
    node_id: NodeId,
    // channel to communicate with transmitter some actions to perform when certain packets are received
    listener_to_transmitter_tx: Sender<TransmitterInternalCommand>,
    // channel to forward reassembled messages
    listener_to_logic_tx: Sender<Message>,
    // listener public channel where drones send packets
    drones_to_listener_rx: Receiver<Packet>,
    // channel to listen on to receive `ListenerCommand`s
    command_rx: Receiver<ListenerCommand>,
    simulation_controller_notifier: Arc<SimulationControllerNotifier>,
    // HashMap containing all the pairs ((source, session_id), Storer)
    storers: HashMap<(NodeId, u64), Storer>,
}

impl PartialEq for Listener {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id && self.storers == other.storers
    }
}

impl Listener {
    pub fn new(
        node_id: NodeId,
        listener_to_transmitter_tx: Sender<TransmitterInternalCommand>,
        listener_to_logic_tx: Sender<Message>,
        drones_to_listener_rx: Receiver<Packet>,
        command_rx: Receiver<ListenerCommand>,
        simulation_controller_notifier: Arc<SimulationControllerNotifier>,
    ) -> Self {
        Self {
            node_id,
            listener_to_transmitter_tx,
            listener_to_logic_tx,
            drones_to_listener_rx,
            command_rx,
            simulation_controller_notifier,
            storers: HashMap::default(),
        }
    }

    pub fn get_node_id(&self) -> NodeId {
        self.node_id
    }

    /// Makes the Listener work
    /// # Panic
    /// Panics if
    pub fn run(&mut self) {
        loop {
            select! {
                recv(self.drones_to_listener_rx) -> packet => {
                    if let Ok(packet) = packet {
                        log::info!("Received packet {packet}");
                        self.process_drone_packet(packet);
                    } else {
                        log::error!("Listener cannot receive packets from drones channel");
                        panic!("Listener cannot receive packets from drones channel");
                    }
                },
                recv(self.command_rx) -> command => {
                    if let Ok(command) = command {
                        match command {
                            ListenerCommand::Quit => break,
                        }
                    } else {
                        log::error!("Listener cannot receive packets from command channel");
                        panic!("Listener cannot receive packets from command channel");
                    }
                }
            }
        }
    }

    /// Checks the readiness for the `Storer` associated to the `key: (NodeId, session_id)`. Returns `None` if there is no `Storer` associated to the given `key`
    fn check_storer(&self, key: (NodeId, u64)) -> Option<bool> {
        let storer = self.storers.get(&key)?;
        Some(storer.is_ready())
    }

    /// Stores a `Fragment` into the `Storer` for the given `key: (NodeId, session_id)`
    fn store_fragment(&mut self, key: (NodeId, u64), fragment: Fragment) {
        let storer = self.storers.get_mut(&key);
        match storer {
            Some(storer) => {
                log::info!("Storing fragment {fragment} into storer");
                storer.insert_fragment(fragment);
            }
            None => {
                log::info!("Creating a new storer for fragment {fragment}");
                let storer = Storer::new_from_fragment(fragment);
                self.storers.insert(key, storer);
            }
        }
    }

    /// Processes a `Packet` received from the connected drones based on the `PacketType`
    /// # Panic
    /// - Panics if there is no hop for the given hop_index in packet.routing_header field
    /// - Panics if there is no Storer for a key that was already used (and the message is yet to be reassembled)
    fn process_drone_packet(&mut self, packet: Packet) {
        // notify simulation controller
        let event = NodeEvent::PacketReceived(packet.clone());
        self.simulation_controller_notifier.send_event(event);

        match packet.pack_type {
            PacketType::MsgFragment(ref fragment) => {
                log::info!("Processing a message fragment");
                let session_id = packet.session_id;

                let source = Self::get_source(&packet.routing_header);

                let current_hop_id = match packet.routing_header.current_hop() {
                    Some(id) => id,
                    None => {
                        log::error!("Received a packet with hop_index out of bounds");
                        panic!("Received a packet with hop_index out of bounds");
                    }
                };

                let wrong_destination = current_hop_id != self.node_id;

                if !packet.routing_header.is_last_hop() || wrong_destination {
                    let nack_type = NackType::UnexpectedRecipient(self.node_id);

                    let nack = Nack {
                        fragment_index: fragment.fragment_index,
                        nack_type,
                    };

                    let command = TransmitterInternalCommand::SendNack {
                        session_id,
                        nack,
                        destination: source,
                    };

                    self.send_command_to_transmitter(command);

                    return;
                }

                let command = TransmitterInternalCommand::SendAckFor { session_id, fragment_index: fragment.fragment_index, destination: source };
                self.send_command_to_transmitter(command);

                let key = (source, session_id);

                self.store_fragment(key, fragment.clone());

                match self.storers.get(&key) {
                    Some(storer) => {
                        if storer.is_ready() {
                            log::info!("Storer for session {session_id} is ready for message reassemble");

                            let fragments = storer.get_fragments();
                            let message = NaiveAssembler::reassemble(&fragments);
                            let message = String::from_utf8(message).unwrap();
                            let message: Message = MessageUtilities::from_string(message).unwrap();

                            log::info!("Reassembled message in bytes: {message:?}");
                            self.storers.remove(&key);

                            self.send_message_to_logic(message);
                        }
                    }
                    None => {
                        log::error!("Storer for session {session_id} not found. At this point however it should exist");
                        panic!("Storer for session {session_id} not found. At this point however it should exist");
                    }
                }
            },
            PacketType::Nack(nack) => {
                let source = Self::get_source(&packet.routing_header);

                let command = TransmitterInternalCommand::ProcessNack {
                    session_id: packet.session_id,
                    nack,
                    source,
                };

                self.send_command_to_transmitter(command);
            },
            PacketType::Ack(ack) => {
                let source = Self::get_source(&packet.routing_header);

                let command = TransmitterInternalCommand::ForwardAckTo {
                    session_id: packet.session_id,
                    ack,
                    source,
                };

                self.send_command_to_transmitter(command);
            }
            PacketType::FloodRequest(flood_request) => {
                let command = TransmitterInternalCommand::ProcessFloodRequest(flood_request);
                self.send_command_to_transmitter(command);
            }
            PacketType::FloodResponse(flood_response) => {
                let command = TransmitterInternalCommand::ProcessFloodResponse(flood_response);
                self.send_command_to_transmitter(command);
            }
        }
    }

    /// Sends a `TransmitterInternalCommand` to `Transmitter`
    /// # Panic
    /// Panics if the transmission to `Transmitter` fails
    fn send_command_to_transmitter(&self, command: TransmitterInternalCommand) {
        match self.listener_to_transmitter_tx.send(command) {
            Ok(()) => {
                log::info!("Command successfully sent to transmitter");
            }
            Err(SendError(command)) => {
                log::warn!("Listener cannot send command {command:?} to transmitter");
                panic!("Listener cannot send command {command:?} to transmitter");
            }
        }
    }

    /// Sends a `Message` into logic channel
    /// # Panic
    /// Panics if the transmission to `Logic` fails
    fn send_message_to_logic(&self, message: Message) {
        match self.listener_to_logic_tx.send(message) {
            Ok(()) => {
                log::info!("Listener successfully forwarded a message to server logic");
            },
            Err(SendError(message)) => {
                panic!("Listener cannot forward message {message:?} to server logic");
            }
        }
    }

    /// Return the source (i.e. first hop) for the given `SourceRoutingHeader`
    /// # Panic
    /// Panics if there is no source in the given `SourceRoutingHeader`
    fn get_source(routing_header: &SourceRoutingHeader) -> NodeId {
        if let Some(source) = routing_header.source() {
            source
        } else {
            log::error!("Received a packet with no source");
            panic!("Received a packet with no source");
        }

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use ntest::timeout;
    use std::sync::{Arc, Mutex, RwLock};
    use std::thread;
    use std::time::Duration;
    use messages::{MessageType, RequestType, TextRequest};
    use wg_2024::network::SourceRoutingHeader;
    use wg_2024::packet::{
        Ack, FloodRequest, FloodResponse, Nack, NackType, NodeType, Packet, PacketType,
    };

    fn create_listener_and_channels(
        node_id: NodeId,
    ) -> (
        Listener,
        Sender<Packet>,
        Receiver<TransmitterInternalCommand>,
        Receiver<Message>,
        Sender<ListenerCommand>,
        Sender<Packet>,
        Receiver<NodeEvent>,
    ) {
        let (internal_transmitter_to_listener_tx, internal_transmitter_to_listener_rx) =
            unbounded();
        let (internal_listener_to_transmitter_tx, internal_listener_to_transmitter_rx) =
            unbounded();
        let (internal_listener_to_server_logic_tx, internal_listener_to_server_logic_rx) =
            unbounded();
        let (listener_commands_tx, listener_commands_rx) = unbounded();
        let (listener_public_tx, listener_public_rx) = unbounded();

        let (simulation_controller_tx, simulation_controller_rx) = unbounded();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let listener = Listener::new(
            node_id,
            internal_listener_to_transmitter_tx,
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

        let (transmitter_tx, transmitter_rx) = unbounded::<TransmitterInternalCommand>();
        let (drones_tx, drones_rx) = unbounded::<Packet>();
        let (server_logic_tx, _server_logic_rx) = unbounded::<Message>();
        let (command_tx, command_rx) = unbounded::<ListenerCommand>();
        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let expected = Listener {
            node_id: 1,
            listener_to_transmitter_tx: transmitter_tx,
            listener_to_logic_tx: server_logic_tx,
            drones_to_listener_rx: drones_rx,
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
        let (transmitter_tx, transmitter_rx) = unbounded();
        let (drones_tx, drones_rx) = unbounded::<Packet>();
        let (server_logic_tx, _server_logic_rx) = unbounded::<Message>();
        let (command_tx, command_rx) = unbounded::<ListenerCommand>();
        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let mut expected = Listener {
            node_id: 1,
            listener_to_transmitter_tx: transmitter_tx,
            listener_to_logic_tx: server_logic_tx,
            drones_to_listener_rx: drones_rx,
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

        let source: NodeId = 0;
        let key = (source, session_id);
        assert_eq!(listener.check_storer(key), None);

        listener.store_fragment(key, fragments[0].clone());

        let mut expected_storers = HashMap::new();
        let expected_storer = Storer::new_from_fragment(fragments[0].clone());
        expected_storers.insert(key, expected_storer);
        expected.storers = expected_storers;

        assert_eq!(listener, expected);
        assert_eq!(listener.check_storer(key), Some(false));

        listener.store_fragment(key, fragments[1].clone());
        listener.store_fragment(key, fragments[2].clone());

        let storer = expected.storers.get_mut(&key).unwrap();
        storer.insert_fragment(fragments[1].clone());
        storer.insert_fragment(fragments[2].clone());

        assert_eq!(listener, expected);

        assert_eq!(listener.check_storer(key), Some(true));
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

        let command = TransmitterInternalCommand::ForwardAckTo {
            session_id: 0,
            ack: Ack { fragment_index: 0 },
            source: 0,
        };

        let expected = command.clone();

        listener.send_command_to_transmitter(command);

        let received = internal_listener_to_transmitter_rx.recv().unwrap();
        assert_eq!(received, expected);
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

        let listener = Arc::new(RwLock::new(listener));
        let listener_clone = Arc::clone(&listener);

        let _ = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            let mut listener = listener_clone.write().unwrap();
            listener.run()
        });

        assert_eq!(listener.read().unwrap().storers.len(), 0);

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

        thread::sleep(Duration::from_millis(200));
        let _ = listener_commands_tx.send(ListenerCommand::Quit);

        let listener = listener.read().unwrap();

        assert_eq!(listener.storers.len(), 1);
        let storer = listener.storers.get(&(1, 10)).unwrap();
        assert!(!storer.is_ready());

        let fragments = storer.get_fragments();
        let expected_fragments = vec![fragment];
        assert_eq!(fragments, expected_fragments);
    }

    #[test]
    #[timeout(2000)]
    fn receive_ack() {
        let (
            mut listener,
            _internal_transmitter_to_listener_tx,
            internal_listener_to_transmitter_rx,
            _internal_listener_to_server_logic_rx,
            _listener_commands_tx,
            listener_public_tx,
            _simulation_controller_rx,
        ) = create_listener_and_channels(1);

        let handle = thread::spawn(move || {
            listener.run()
        });

        let ack = Ack { fragment_index: 0 };

        let expected = TransmitterInternalCommand::ForwardAckTo {
            session_id: 0,
            ack: ack.clone(),
            source: 5,
        };

        let ack = Packet {
            routing_header: SourceRoutingHeader { hop_index: 1, hops: vec![5, 1] } ,
            session_id: 0,
            pack_type: PacketType::Ack(ack),
        };

        let _ = listener_public_tx.send(ack);

        let received = internal_listener_to_transmitter_rx.recv().unwrap();

        assert_eq!(received, expected);
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

        let expected = TransmitterInternalCommand::ProcessNack {
            session_id: 0,
            nack: nack.clone(),
            source: 0,
        };

        let nack = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![0],
            },
            session_id: 0,
            pack_type: PacketType::Nack(nack),
        };

        let _ = listener_public_tx.send(nack);

        let received = internal_listener_to_transmitter_rx.recv().unwrap();

        assert_eq!(received, expected);
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

        let expected = TransmitterInternalCommand::ProcessFloodRequest (flood_request.clone());

        let flood_request = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![0],
            },
            session_id: 0,
            pack_type: PacketType::FloodRequest(flood_request),
        };

        let _ = listener_public_tx.send(flood_request);

        let received = internal_listener_to_transmitter_rx.recv().unwrap();

        assert_eq!(received, expected);
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

        let expected = TransmitterInternalCommand::ProcessFloodResponse(flood_response.clone());

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

        assert_eq!(received, expected);
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

            let expected = TransmitterInternalCommand::SendAckFor {
                session_id: 0,
                fragment_index: fragment.fragment_index,
                destination: 0,
            };

            let _ = listener_public_tx.send(packet);

            let received = internal_listener_to_transmitter_rx.recv().unwrap();

            assert_eq!(received, expected);
        }

        let _ = listener_commands_tx.send(ListenerCommand::Quit);

        let received = internal_listener_to_server_logic_rx.recv().unwrap();
        assert_eq!(received, message);
    }
}
