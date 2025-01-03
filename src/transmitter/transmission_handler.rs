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
use crate::transmitter::{Command, TransmissionHandlerEvent};
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
    command_rx: Receiver<Command>,
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
        command_rx: Receiver<Command>,
        transmission_handler_event_tx: Sender<TransmissionHandlerEvent>,
        simulation_controller_notifier: Arc<SimulationControllerNotifier>,
    ) -> Self {
        let to_be_fragmented = message.content.clone();
        let fragments = NaiveAssembler::disassemble(&to_be_fragmented.stringify().into_bytes());
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
        self.source_routing_header = source_routing_header;

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
                            Command::Resend(fragment_index) => {
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
                                            Command::Resend(fragment_index)
                                        );
                                    }
                                }
                            }
                            Command::Confirmed(fragment_index) => {
                                self.received_acks.insert(fragment_index);
                                if self.received_acks.len() == self.fragments.len() {
                                    let event = NodeEvent::MessageSentSuccessfully(self.message.clone());
                                    self.simulation_controller_notifier.send_event(event);
                                    break;
                                }
                            }
                            /*
                            Command::UpdateSourceRoutingHeader(source_routing_header) => {
                                self.update_source_routing_header(source_routing_header);
                                // Note: it is not needed to resend the previous fragments, if a
                                // NACK will be received, then they will be sent again using the
                                // new header
                            }
                             */
                            Command::Quit => {
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

    fn update_source_routing_header(&mut self, source_routing_header: SourceRoutingHeader) {
        self.source_routing_header = source_routing_header;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use crossbeam_channel::unbounded;
    use super::*;
    use messages::{ChatResponse, Message, MessageType, ResponseType};
    use wg_2024::packet::{NodeType, Packet, PacketType};

    #[test]
    fn initialize() {
        let message = Message {
            source_id: 0,
            session_id: 0,
            content: MessageType::Response(ResponseType::ChatResponse(ChatResponse::MessageSent)),
        };

        let (listener_tx, _listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(0, HashMap::new(), listener_tx);
        let gateway = Arc::new(gateway);
        let (_command_tx, command_rx) = unbounded::<Command>();
        let network_controller = NetworkController::new(0, NodeType::Server, gateway.clone());
        let network_controller = Arc::new(network_controller);
        let destination_node_id: NodeId = 1;
        let (transmission_handler_event_tx, transmission_handler_event_rx) = unbounded::<TransmissionHandlerEvent>();

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let transmission_handler = TransmissionHandler::new(
            message.clone(),
            gateway,
            network_controller,
            destination_node_id,
            command_rx,
            transmission_handler_event_tx,
            simulation_controller_notifier,
        );

        assert_eq!(message.source_id, transmission_handler.source_id);
        assert_eq!(message.session_id, transmission_handler.session_id);
        assert_eq!(message.content, transmission_handler.message.content);
    }

    #[test]
    fn prepare_packets() {
        let message = Message {
            source_id: 1,
            session_id: 51,
            content: MessageType::Response(ResponseType::ChatResponse(ChatResponse::MessageSent)),
        };

        let (listener_tx, listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(0, HashMap::new(), listener_tx);
        let gateway = Arc::new(gateway);
        let (command_tx, command_rx) = unbounded::<Command>();
        let network_controller = NetworkController::new(0, NodeType::Server, gateway.clone());
        let network_controller = Arc::new(network_controller);
        let destination_node_id: NodeId = 1;
        let (transmission_handler_event_tx, transmission_handler_event_rx) = unbounded::<TransmissionHandlerEvent>();

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let transmission_handler = TransmissionHandler::new(
            message.clone(),
            gateway,
            network_controller,
            destination_node_id,
            command_rx,
            transmission_handler_event_tx,
            simulation_controller_notifier
        );

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
        let (listener_tx, listener_rx) = unbounded::<Packet>();
        let gateway = Gateway::new(0, HashMap::new(), listener_tx);
        let gateway = Arc::new(gateway);
        let (command_tx, command_rx) = unbounded::<Command>();
        let network_controller = NetworkController::new(0, NodeType::Server, gateway.clone());
        let network_controller = Arc::new(network_controller);
        let destination_node_id: NodeId = 1;
        let (transmission_handler_event_tx, transmission_handler_event_rx) = unbounded::<TransmissionHandlerEvent>();

        let (simulation_controller_tx, simulation_controller_rx) = unbounded::<NodeEvent>();
        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let mut transmission_handler = TransmissionHandler::new(
            message.clone(),
            gateway,
            network_controller,
            destination_node_id,
            command_rx,
            transmission_handler_event_tx,
            simulation_controller_notifier,
        );

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
}