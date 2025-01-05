use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use crossbeam_channel::{select, unbounded, Receiver, Sender};
use messages::Message;
use messages::node_event::NodeEvent;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Ack, FloodRequest, FloodResponse, Nack, NackType, NodeType, Packet, PacketType};
use crate::simulation_controller_notifier::SimulationControllerNotifier;
use crate::transmitter::network_controller::NetworkController;
use crate::transmitter::gateway::Gateway;
use crate::transmitter::transmission_handler::TransmissionHandler;

mod network_controller;
mod gateway;
mod transmission_handler;

#[derive(Debug, Clone)]
pub enum TransmissionHandlerCommand {
    Resend(u64),
    Confirmed(u64),
    Quit,
    UpdateHeader,
}

#[derive(Debug, Clone)]
enum TransmissionHandlerEvent {
    TransmissionCompleted(u64)
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransmitterInternalCommand {
    SendAckFor { session_id: u64, fragment_index: u64, destination: NodeId },
    ForwardAckTo { session_id: u64, ack: Ack },
    ProcessNack { session_id: u64, nack: Nack, source: NodeId },
    ProcessFloodRequest(FloodRequest),
    ProcessFloodResponse(FloodResponse),
}

pub enum TransmitterUserCommand {
    Quit
}

#[derive(Debug)]
pub struct Transmitter {
    node_id: NodeId,
    // listener -> transmitter
    listener_rx: Receiver<TransmitterInternalCommand>, // receives ACKs, NACKs, FloodRequest and FloodResponse
    // server logic -> transmitter
    server_logic_rx: Receiver<(NodeId, Message)>,
    network_controller: Arc<NetworkController>,
    // transmitter -> transmission handlers
    transmission_handlers: HashMap<u64, Sender<TransmissionHandlerCommand>>,
    transmission_handler_event_rx: Receiver<TransmissionHandlerEvent>,
    transmission_handler_event_tx: Sender<TransmissionHandlerEvent>,
    gateway: Arc<Gateway>,
    simulation_controller_notifier: Arc<SimulationControllerNotifier>,
    transmitter_command_rx: Receiver<TransmitterUserCommand>,
}

impl PartialEq for Transmitter {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id
        && self.network_controller == other.network_controller
        && self.transmission_handlers.keys().eq(other.transmission_handlers.keys())
        && self.gateway.eq(&other.gateway)
    }
}

impl Eq for Transmitter { }

impl Transmitter {
    pub fn new(
        node_id: NodeId,
        node_type: NodeType,
        listener_rx: Receiver<TransmitterInternalCommand>,
        server_logic_rx: Receiver<(NodeId, Message)>,
        connected_drones: HashMap<NodeId, Sender<Packet>>,
        simulation_controller_notifier: Arc<SimulationControllerNotifier>,
        transmitter_command_rx: Receiver<TransmitterUserCommand>,
    ) -> Self {
        let gateway = Gateway::new(node_id, connected_drones, simulation_controller_notifier.clone());
        let gateway = Arc::new(gateway);

        let (transmission_handler_event_tx, transmission_handler_event_rx) = unbounded::<TransmissionHandlerEvent>();

        Self {
            node_id,
            listener_rx,
            server_logic_rx,
            network_controller: Arc::new(NetworkController::new(node_id, node_type, gateway.clone(), simulation_controller_notifier.clone())),
            transmission_handlers: HashMap::new(),
            transmission_handler_event_tx,
            transmission_handler_event_rx,
            gateway,
            simulation_controller_notifier,
            transmitter_command_rx,
        }
    }

    pub fn run(&mut self) {
        // when run is called, transmitter should instantaneously flood the network to discover routes
        loop {
            select! {
                recv(self.listener_rx) -> command => {
                    if let Ok(command) = command {
                        self.process_transmitter_internal_command(command);
                        // self.process_listener_packet(packet);
                    } else {
                        panic!("Error while receiving from listener_channel");
                    }
                },
                recv(self.server_logic_rx) -> message_data => {
                    // to send a server logic message, create a new session_id, pass the high level
                    // message to a new transmission_handler, store a reference to that transmission
                    // handler in a hashmap containing the session_id as the key
                    // The transmission handler will handler the fragmentation by using the assembler
                    // The reference to the transmission handler will be removed when the
                    // transmission_handler will have received every ACK message
                    if let Ok((destination_id, message)) = message_data {
                        self.process_high_level_message(message, destination_id);
                    } else {
                        panic!("Error while receiving from server_logic")
                    }
                },
                recv(self.transmission_handler_event_rx) -> event => {
                    if let Ok(event) = event {
                        let TransmissionHandlerEvent::TransmissionCompleted(session_id) = event;
                        self.transmission_handlers.remove(&session_id);
                    }
                },
                recv(self.transmitter_command_rx) -> command => {
                    if let Ok(command) = command {
                        match command {
                            TransmitterUserCommand::Quit => break,
                        }
                    }
                },
            }
        }
    }

    /// Processes a message received from server logic
    fn process_high_level_message(&mut self, message: Message, destination_id: NodeId) {
        let (command_tx, command_rx) = unbounded::<TransmissionHandlerCommand>();

        let session_id = message.session_id; // TODO: or should be a random number?
        let mut transmission_handler = TransmissionHandler::new(
            message,
            self.gateway.clone(),
            self.network_controller.clone(),
            destination_id,
            command_rx,
            self.transmission_handler_event_tx.clone(),
            self.simulation_controller_notifier.clone(),
        );

        thread::spawn(move || {
            transmission_handler.run();
        });

        self.transmission_handlers.insert(session_id, command_tx);
    }

    fn send_transmission_handler_command(&self, session_id: u64, command: TransmissionHandlerCommand) {
        let handler_channel = match self.transmission_handlers.get(&session_id) {
            Some(channel) => {
                channel
            },
            None => {
                // TODO: what to do here?
                // TODO: send UnexpectedRecipient?
                // let command = TransmitterInternalCommand::ProcessNack {
                //     session_id,
                //     nack: Nack {
                //         fragment_index: 0,
                //         nack_type: NackType::UnexpectedRecipient(self.node_id),
                //     },
                //     source: ,
                // }

                // return;
                panic!("no handler found for the required session_id");
            },
        };
        match handler_channel.send(command) {
            Ok(()) => {},
            Err(err) => {
                // TODO: ignore this?
                panic!("Cannot communicate with handler");
            }
        }
    }

    fn process_transmitter_internal_command(&self, command: TransmitterInternalCommand) {
        match command {
            TransmitterInternalCommand::SendAckFor { session_id, fragment_index, destination} => {
                let ack = Ack {
                    fragment_index,
                };
                // TODO: send ack
            }
            TransmitterInternalCommand::ForwardAckTo { session_id, ack } => {
                let command = TransmissionHandlerCommand::Confirmed(ack.fragment_index);
                self.send_transmission_handler_command(session_id, command);
            }
            TransmitterInternalCommand::ProcessNack { session_id, nack, source } => {
                self.process_nack(session_id, nack, source);
            }
            TransmitterInternalCommand::ProcessFloodRequest(flood_request) => {
                // if a flood request is received, send a flood_response
                let mut path_trace = flood_request.path_trace;
                path_trace.push((self.node_id, NodeType::Server));
                path_trace.reverse();
                let flood_response = FloodResponse {
                    flood_id: flood_request.flood_id,
                    path_trace,
                };
                self.gateway.send_flood_response(flood_response);
            }
            TransmitterInternalCommand::ProcessFloodResponse(flood_response) => {
                self.network_controller.update_from_flood_response(flood_response);
            }
        }
    }

    fn process_nack(&self, session_id: u64, nack: Nack, source: NodeId) {
        self.network_controller.update_from_nack(&nack, source);
        match nack.nack_type {
            NackType::Dropped => {
                let fragment_index = nack.fragment_index;

                let command = TransmissionHandlerCommand::Resend(fragment_index);
                self.send_transmission_handler_command(session_id, command);
            },
            NackType::ErrorInRouting(_) => {
                let fragment_index = nack.fragment_index;

                let command = TransmissionHandlerCommand::UpdateHeader;
                self.send_transmission_handler_command(session_id, command);

                let command = TransmissionHandlerCommand::Resend(fragment_index);
                self.send_transmission_handler_command(session_id, command);
            }
            _ => {
                // TODO: what to do with other nacks?
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::thread;
    use std::thread::{sleep, JoinHandle};
    use std::time::Duration;
    use assembler::Assembler;
    use assembler::naive_assembler::NaiveAssembler;
    use crossbeam_channel::{unbounded, Receiver, Sender};
    use messages::{Message, MessageType, MessageUtilities, ResponseType, TextResponse};
    use messages::node_event::NodeEvent;
    use ntest::timeout;
    use wg_2024::network::{NodeId, SourceRoutingHeader};
    use wg_2024::packet::{Ack, FloodResponse, Fragment, NodeType, Packet, PacketType};
    use crate::simulation_controller_notifier::SimulationControllerNotifier;
    use crate::test_utils::TransmitterInternalCommand;
    use crate::transmitter::gateway::Gateway;
    use crate::transmitter::network_controller::NetworkController;
    use crate::transmitter::{TransmissionHandlerEvent, Transmitter, TransmitterUserCommand};

    fn create_transmitter(node_id: NodeId, node_type: NodeType, connected_drones: HashMap<NodeId, Sender<Packet>>)
        -> (Transmitter, Sender<TransmitterInternalCommand>, Sender<(NodeId, Message)>, Receiver<NodeEvent>, Sender<TransmitterUserCommand>)
    {
        let (listener_to_transmitter_tx, listener_to_transmitter_rx) = unbounded::<TransmitterInternalCommand>();
        let (server_logic_to_transmitter_tx, server_logic_to_transmitter_rx) = unbounded::<(NodeId, Message)>();

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let (transmitter_command_tx, transmitter_command_rx) = unbounded::<TransmitterUserCommand>();

        let transmitter = Transmitter::new(
            node_id,
            node_type,
            listener_to_transmitter_rx,
            server_logic_to_transmitter_rx,
            connected_drones,
            simulation_controller_notifier,
            transmitter_command_rx
        );

        (transmitter, listener_to_transmitter_tx, server_logic_to_transmitter_tx, simulation_controller_rx, transmitter_command_tx)
    }

    #[test]
    fn initialize() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let mut connected_drones: HashMap<NodeId, Sender<Packet>> = HashMap::new();

        let drone_1_id = 1;
        let (drone_1_tx, drone_1_rx) = unbounded::<Packet>();
        connected_drones.insert(drone_1_id, drone_1_tx);

        let (transmitter,
            listener_to_transmitter_tx,
            server_logic_to_transmitter_tx,
            simulation_controller_rx,
            transmitter_command_tx) = create_transmitter(node_id, node_type, connected_drones);


        let mut neighbors = HashMap::new();
        let (tx, rx) = unbounded::<Packet>();
        neighbors.insert(1, tx);

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let gateway = Gateway::new(node_id, neighbors, simulation_controller_notifier.clone());
        let gateway = Arc::new(gateway);

        let (listener_tx, listener_rx) = unbounded::<TransmitterInternalCommand>();
        let (server_logic_tx, server_logic_rx) = unbounded::<(NodeId, Message)>();
        let (transmitter_to_transmission_handler_event_tx, transmitter_to_transmission_handler_event_rx) = unbounded::<TransmissionHandlerEvent>();
        let (transmission_handler_to_transmitter_event_tx, transmission_handler_to_transmitter_event_rx) = unbounded::<TransmissionHandlerEvent>();

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();

        let (transmitter_command_tx, transmitter_command_rx) = unbounded::<TransmitterUserCommand>();

        let expected = Transmitter {
            node_id,
            listener_rx,
            server_logic_rx,
            network_controller: Arc::new(NetworkController::new(node_id, node_type, gateway.clone(), simulation_controller_notifier.clone())),
            transmission_handlers: Default::default(),
            transmission_handler_event_rx: transmission_handler_to_transmitter_event_rx,
            transmission_handler_event_tx: transmission_handler_to_transmitter_event_tx,
            gateway: gateway.clone(),
            simulation_controller_notifier: Arc::new(SimulationControllerNotifier::new(simulation_controller_tx)),
            transmitter_command_rx
        };

        assert_eq!(transmitter, expected);
    }

    #[test]
    #[timeout(2000)]
    fn check_process_high_level_message() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let mut connected_drones: HashMap<NodeId, Sender<Packet>> = HashMap::new();

        let drone_1_id = 1;
        let (drone_1_tx, drone_1_rx) = unbounded::<Packet>();
        connected_drones.insert(drone_1_id, drone_1_tx);

        let (mut transmitter,
            listener_to_transmitter_tx,
            server_logic_to_transmitter_tx,
            simulation_controller_rx,
            transmitter_command_tx
        ) = create_transmitter(node_id, node_type, connected_drones);

        let message = Message {
            source_id: 0,
            session_id: 0,
            content: MessageType::Response(
                ResponseType::TextResponse(
                    TextResponse::Text(
                        "test".to_string()
                    )
                )
            ),
        };

        transmitter.process_high_level_message(message.clone(), 1);

        // let received = simulation_controller_rx.recv().unwrap();
        //
        // assert!(matches!(received, NodeEvent::PacketSent(_)));

        assert_eq!(transmitter.transmission_handlers.len(), 1);
    }

    #[test]
    #[timeout(2000)]
    fn check_command() -> thread::Result<()> {
        let node_id = 0;
        let node_type = NodeType::Server;
        let mut connected_drones: HashMap<NodeId, Sender<Packet>> = HashMap::new();

        let drone_1_id = 1;
        let (drone_1_tx, drone_1_rx) = unbounded::<Packet>();
        connected_drones.insert(drone_1_id, drone_1_tx);

        let (mut transmitter,
            listener_to_transmitter_tx,
            server_logic_to_transmitter_tx,
            simulation_controller_rx,
            transmitter_command_tx) = create_transmitter(node_id, node_type, connected_drones);

        let packet = Packet {
            routing_header: SourceRoutingHeader { hop_index: 1, hops: vec![1, 0] },
            session_id: 0,
            pack_type: PacketType::MsgFragment( Fragment {
                fragment_index: 0,
                total_n_fragments: 1,
                length: 128,
                data: [0; 128],
            }),
        };

        let handle = thread::spawn(move || {
            transmitter.run();
        });

        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![
                (node_id, node_type),
                (1, NodeType::Drone),
            ],
        };

        let flood_response_command = TransmitterInternalCommand::ProcessFloodResponse(flood_response);
        listener_to_transmitter_tx.send(flood_response_command).expect("Cannot communicate with transmitter");

        sleep(Duration::from_millis(200));

        transmitter_command_tx.send(TransmitterUserCommand::Quit);

        handle.join()
    }

    #[test]
    fn check_flood_response_processing() {
        let node_id = 0;
        let node_type = NodeType::Server;

        let (internal_transmitter_to_listener_tx, internal_transmitter_to_listener_rx) = unbounded::<Packet>();
        let (internal_listener_to_transmitter_tx, internal_listener_to_transmitter_rx) = unbounded::<TransmitterInternalCommand>();
        let (internal_server_logic_to_transmitter_tx, internal_server_logic_to_transmitter_rx) = unbounded::<(NodeId, Message)>();

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let (transmitter_command_tx, transmitter_command_rx) = unbounded::<TransmitterUserCommand>();

        let connected_drones = HashMap::new();

        // let mut connected_drones = HashMap::new();
        // let (drone_1_tx, drone_1_rx) = unbounded::<Packet>();
        // connected_drones.insert(1, drone_1_tx);

        let mut transmitter = Transmitter::new(
            node_id,
            node_type,
            internal_listener_to_transmitter_rx,
            internal_server_logic_to_transmitter_rx,
            connected_drones,
            simulation_controller_notifier,
            transmitter_command_rx,
        );

        thread::spawn(move || {
            transmitter.run();
        });

        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![
                (node_id, node_type),
                (1, NodeType::Drone),
                (2, NodeType::Client),
            ],
        };
        // let flood_response = Packet {
        //     routing_header: SourceRoutingHeader {
        //         hop_index: 0,
        //         hops: vec![],
        //     },
        //     session_id: 0,
        //     pack_type: PacketType::FloodResponse(flood_response),
        // };
        let flood_response_command = TransmitterInternalCommand::ProcessFloodResponse(flood_response);
        internal_listener_to_transmitter_tx.send(flood_response_command).expect("Cannot communicate with transmitter");

        // TODO: complete this
        // let expected = NodeEvent::KnownNetworkGraph(
        //
        // );

        let flood_response = FloodResponse {
            flood_id: 0,
            path_trace: vec![
                (node_id, node_type),
                (1, NodeType::Drone),
                (2, NodeType::Client),
            ],
        };
        // let flood_response = Packet {
        //     routing_header: SourceRoutingHeader {
        //         hop_index: 0,
        //         hops: vec![],
        //     },
        //     session_id: 0,
        //     pack_type: PacketType::FloodResponse(flood_response),
        // };
        let flood_response_command = TransmitterInternalCommand::ProcessFloodResponse(flood_response);
        internal_listener_to_transmitter_tx.send(flood_response_command).expect("Cannot communicate with transmitter");

        // TODO: complete this
        // let expected = NodeEvent::KnownNetworkGraph(
        //
        // );
    }
}