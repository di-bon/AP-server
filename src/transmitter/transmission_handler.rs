use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use assembler::Assembler;
use assembler::naive_assembler::NaiveAssembler;
use crossbeam_channel::{select, Receiver, Sender};
use messages::{Message, MessageUtilities};
use messages::node_event::NodeEvent;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Fragment, Packet, PacketType};
use crate::simulation_controller_notifier::SimulationControllerNotifier;
use crate::transmitter::{TransmissionHandlerCommand, TransmissionHandlerEvent};
use crate::transmitter::gateway::Gateway;
use crate::transmitter::network_controller::NetworkController;
// TODO: maybe also handle ACKs?

// TODO: implement backoff time -> move network_controller.get_path() inside transmission_handler
// instead of receiving a routing header in the constructor

/// A `TransmissionHandler` struct that will handle the fragmentation and packet creation, sending
/// said packets to the gateway. All created packets will share the same `SourceRoutingHeader`,
/// unless it gets updated using the `update_source_routing_header` method
pub(super) struct TransmissionHandler {
    source_routing_header: SourceRoutingHeader,
    message: Message,
    fragments: Vec<Fragment>,
    source_id: NodeId,
    session_id: u64,
    gateway: Arc<Gateway>,
    network_controller: Arc<NetworkController>,
    destination_node_id: NodeId,
    command_rx: Receiver<TransmissionHandlerCommand>,
    received_acks: HashSet<u64>,
    transmission_handler_event_tx: Sender<TransmissionHandlerEvent>,
    simulation_controller_notifier: Arc<SimulationControllerNotifier>,
}

impl TransmissionHandler {
    pub fn new(
        message: Message,
        gateway: Arc<Gateway>,
        network_controller: Arc<NetworkController>,
        destination_node_id: NodeId,
        command_rx: Receiver<TransmissionHandlerCommand>,
        transmission_handler_event_tx: Sender<TransmissionHandlerEvent>,
        simulation_controller_notifier: Arc<SimulationControllerNotifier>,
    ) -> Self {
        let fragments = NaiveAssembler::disassemble(&message.stringify().into_bytes());
        let source_id = message.source_id;
        let session_id = message.session_id;
        Self {
            // placeholder for source_routing_header that will be later updated in the run() method
            source_routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops: vec![],
            },
            message,
            fragments,
            source_id,
            session_id,
            gateway,
            network_controller,
            destination_node_id,
            command_rx,
            received_acks: HashSet::new(),
            transmission_handler_event_tx,
            simulation_controller_notifier
        }
    }

    // Basic version: send all the fragments all at once, then wait for commands, exit when receiving an ACK for each fragment
    // Refined version: use a sliding window (using AIMD? (i.e. Additive Increase Multiplicative Decrease)) to send the fragments
    pub fn run(&mut self) {
        let source_routing_header = self.find_new_routing_header();
        self.update_source_routing_header(source_routing_header);

        let event = NodeEvent::StartingMessageTransmission(self.message.clone());
        self.simulation_controller_notifier.send_event(event);

        // Send all packets at once
        for fragment in &self.fragments {
            let packet = self.create_packet(fragment.clone());
            self.gateway.forward(packet);
        };

        // wait for commands from transmitter
        loop {
            select! {
                recv(self.command_rx) -> command => {
                    if let Ok(command) = command {
                        match command {
                            TransmissionHandlerCommand::Resend(fragment_index) => {
                                let fragment = self.fragments.get(fragment_index as usize);
                                match fragment {
                                    Some(fragment) => {
                                        let packet = self.create_packet(fragment.clone());
                                        self.gateway.forward(packet);
                                    },
                                    None => {
                                        log::warn!(
                                            "TransmissionHandler for session {} received a command {:?} with fragment index {fragment_index} out of bounds",
                                            self.session_id,
                                            TransmissionHandlerCommand::Resend(fragment_index)
                                        );
                                    }
                                }
                            }
                            TransmissionHandlerCommand::Confirmed(fragment_index) => {
                                self.received_acks.insert(fragment_index);
                                if self.received_acks.len() == self.fragments.len() {
                                    let event = NodeEvent::MessageSentSuccessfully(self.message.clone());
                                    self.simulation_controller_notifier.send_event(event);
                                    break;
                                }
                            },
                            TransmissionHandlerCommand::UpdateHeader => {
                                let source_routing_header = self.find_new_routing_header();
                                self.update_source_routing_header(source_routing_header);
                            },
                            /*
                            Command::UpdateSourceRoutingHeader(source_routing_header) => {
                                self.update_source_routing_header(source_routing_header);
                                // Note: it is not needed to resend the previous fragments, if a
                                // NACK will be received, then they will be sent again using the
                                // new header
                            }
                             */
                            TransmissionHandlerCommand::Quit => {
                                break;
                            }
                        }
                    }
                }
            }
        }
        let event = TransmissionHandlerEvent::TransmissionCompleted(self.session_id);
        match self.transmission_handler_event_tx.send(event.clone()) {
            Ok(()) => {
                log::info!("Transmission handler for session {} sent {:?} to transmitter", self.session_id, event);
            }
            Err(err) => {
                log::warn!("Transmission handler for session {} cannot send TransmissionHandlerEvent messages to transmitter", self.session_id);
            }
        }
        log::info!("Transmission handler for session {} terminated", self.session_id);
    }

    fn create_packet(&self, fragment: Fragment) -> Packet {
        Packet {
            routing_header: self.source_routing_header.clone(),
            session_id: self.session_id,
            pack_type: PacketType::MsgFragment(fragment),
        }
    }

    fn find_new_routing_header(&self) -> SourceRoutingHeader {
        let mut hops;
        loop {
            hops = self.network_controller.get_path(self.destination_node_id);
            if hops.is_some() {
                break;
            } else {
                thread::sleep(Duration::from_millis(2000));
            }
        }
        let source_routing_header = SourceRoutingHeader {
            hop_index: 0,
            hops: hops.unwrap(),
        };
        source_routing_header
    }

    fn update_source_routing_header(&mut self, source_routing_header: SourceRoutingHeader) {
        self.source_routing_header = source_routing_header;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::thread::JoinHandle;
    use crossbeam_channel::unbounded;
    use super::*;
    use messages::{ChatResponse, Message, MessageType, ResponseType};
    use messages::TextResponse::Text;
    use ntest::timeout;
    use wg_2024::packet::{FloodResponse, NodeType, Packet, PacketType};

    fn create_transmission_handler(message: &Message, node_id: NodeId, node_type: NodeType, destination_node_id: NodeId, paths: Vec<FloodResponse>) -> (TransmissionHandler, Receiver<Packet>, Receiver<NodeEvent>, Sender<TransmissionHandlerCommand>) {
        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let mut connected_drones = HashMap::new();

        let (drone_tx, drone_rx) = unbounded::<Packet>();
        connected_drones.insert(1, drone_tx);

        let (transmitter_to_listener_tx, transmitter_to_listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(0, connected_drones, transmitter_to_listener_tx, simulation_controller_notifier.clone());
        let gateway = Arc::new(gateway);

        let (command_tx, command_rx) = unbounded::<TransmissionHandlerCommand>();
        let network_controller = NetworkController::new(node_id, node_type, gateway.clone(), simulation_controller_notifier.clone());
        let network_controller = Arc::new(network_controller);
        let (transmission_handler_event_tx, transmission_handler_event_rx) = unbounded::<TransmissionHandlerEvent>();

        for path in paths {
            network_controller.update_from_flood_response(path);
            let _ = simulation_controller_rx.recv().unwrap();
        }

        let transmission_handler = TransmissionHandler::new(
            message.clone(),
            gateway,
            network_controller,
            destination_node_id,
            command_rx,
            transmission_handler_event_tx,
            simulation_controller_notifier.clone(),
        );

        (transmission_handler, drone_rx, simulation_controller_rx, command_tx)
    }

    #[test]
    fn initialize() {
        let message = Message {
            source_id: 0,
            session_id: 0,
            content: MessageType::Response(ResponseType::ChatResponse(ChatResponse::MessageSent)),
        };

        let paths = vec![];
        let (transmission_handler, drone_rx, simulation_controller_rx, command_tx) = create_transmission_handler(&message, 0, NodeType::Server, 1, paths);

        assert_eq!(message.source_id, transmission_handler.source_id);
        assert_eq!(message.session_id, transmission_handler.session_id);
        assert_eq!(message.content, transmission_handler.message.content);
    }

    #[test]
    fn check_create_packets() {
        let message = Message {
            source_id: 1,
            session_id: 51,
            content: MessageType::Response(ResponseType::ChatResponse(ChatResponse::MessageSent)),
        };

        let paths = vec![];
        let (transmission_handler, drone_rx, simulation_controller_rx, command_tx) = create_transmission_handler(&message, 0, NodeType::Server, 1, paths);

        let expected_packet = Packet {
            routing_header: Default::default(),
            session_id: 51,
            pack_type: PacketType::MsgFragment(transmission_handler.fragments[0].clone()),
        };
        assert_eq!(expected_packet, transmission_handler.create_packet(transmission_handler.fragments[0].clone()))
    }

    #[test]
    fn update_source_routing_header() {
        let message = Message {
            source_id: 1,
            session_id: 51,
            content: MessageType::Response(ResponseType::ChatResponse(ChatResponse::MessageSent)),
        };
        let source_routing_header = SourceRoutingHeader {
            hop_index: 0,
            hops: vec![],
        };

        let paths = vec![];
        let (mut transmission_handler, drone_rx, simulation_controller_rx, command_tx) = create_transmission_handler(&message, 0, NodeType::Server, 1, paths);

        let expected_source_routing_header = SourceRoutingHeader {
            hop_index: 0,
            hops: vec![],
        };
        assert_eq!(transmission_handler.source_routing_header, expected_source_routing_header);

        let new_source_routing_header = SourceRoutingHeader {
            hop_index: 0,
            hops: vec![1, 2, 3, 4],
        };
        transmission_handler.update_source_routing_header(new_source_routing_header.clone());

        assert_eq!(transmission_handler.source_routing_header, new_source_routing_header);
    }

    #[test]
    #[timeout(2000)]
    fn send_packets() {
        let session_id = 0;

        let message = Message {
            source_id: 0,
            session_id,
            content: MessageType::Response(ResponseType::TextResponse(Text("My super long text response .....................".to_string()))),
        };

        let paths = vec![
            FloodResponse {
                flood_id: 0,
                path_trace: vec![
                    (0, NodeType::Server),
                    (1, NodeType::Drone),
                ],
            }
        ];
        let (mut transmission_handler, drone_rx, simulation_controller_rx, command_tx) = create_transmission_handler(&message, 0, NodeType::Server, 1, paths);

        thread::spawn(move || {
            transmission_handler.run();
        });

        let _ = command_tx.send(TransmissionHandlerCommand::Resend(0)).unwrap();

        let fragments = NaiveAssembler::disassemble(&message.stringify().into_bytes());
        let expected_packets: Vec<Packet> = fragments.iter().map(|fragment: &Fragment|
            Packet {
                routing_header: SourceRoutingHeader { hop_index: 1, hops: vec![0, 1] },
                session_id,
                pack_type: PacketType::MsgFragment(fragment.clone()),
            }
        ).collect();

        for expected_packet in &expected_packets {
            let received = drone_rx.recv().unwrap();
            assert_eq!(received, *expected_packet);

            if let PacketType::MsgFragment(fragment) = received.pack_type {
                match command_tx.send(TransmissionHandlerCommand::Confirmed(fragment.fragment_index)) {
                    Ok(()) => (),
                    Err(err) => panic!("Cannot communicate with transmission handler"),
                }
            } else {
                panic!("Got wrong message type")
            }
        }

        let received = drone_rx.recv().unwrap();
        assert_eq!(received, expected_packets[0]);

        let event = simulation_controller_rx.recv().unwrap();
        assert!(matches!(event, NodeEvent::StartingMessageTransmission(_)));

        let event = simulation_controller_rx.recv().unwrap();
        assert!(matches!(event, NodeEvent::PacketSent(_)));

        let event = simulation_controller_rx.recv().unwrap();
        assert!(matches!(event, NodeEvent::PacketSent(_)));

        let event = simulation_controller_rx.recv().unwrap();
        assert!(matches!(event, NodeEvent::PacketSent(_)));

        let event = simulation_controller_rx.recv().unwrap();
        assert!(matches!(event, NodeEvent::MessageSentSuccessfully(_)));
    }

    #[test]
    #[timeout(2000)]
    fn check_quit_command() -> std::thread::Result<()> {
        let session_id = 0;

        let message = Message {
            source_id: 0,
            session_id,
            content: MessageType::Response(ResponseType::TextResponse(Text("My super long text response .....................".to_string()))),
        };

        let paths = vec![
            FloodResponse {
                flood_id: 0,
                path_trace: vec![
                    (0, NodeType::Server),
                    (1, NodeType::Drone),
                ],
            }
        ];
        let (mut transmission_handler, drone_rx, simulation_controller_rx, command_tx) = create_transmission_handler(&message, 0, NodeType::Server, 1, paths);

        let handle = thread::spawn(move || {
            transmission_handler.run();
        });

        let _ = command_tx.send(TransmissionHandlerCommand::Quit).unwrap();

        handle.join()
    }
}