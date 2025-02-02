use std::collections::{HashMap, HashSet};
use crossbeam_channel::{select, Receiver, SendError, Sender};
use messages::{ChatRequest, ChatResponse, ErrorType, Message, MessageType, RequestType, ResponseType, ServerType, TextResponse};
use rand::{random, Rng};
use wg_2024::network::NodeId;
use crate::server_logic::Command;

pub struct ContentServer {
    node_id: NodeId,
    server_logic_to_transmitter_tx: Sender<Message>,
    listener_to_server_logic_rx: Receiver<Message>,
    command_rx: Receiver<Command>,
}

impl ContentServer {
    pub fn new(
        node_id: NodeId,
        server_logic_to_transmitter_tx: Sender<Message>,
        listener_to_server_logic_rx: Receiver<Message>,
        command_rx: Receiver<Command>
    ) -> Self {
        Self {
            node_id,
            server_logic_to_transmitter_tx,
            listener_to_server_logic_rx,
            command_rx,
        }
    }

    pub fn get_node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn run(&mut self) {
        loop {
            select! {
                recv(self.command_rx) -> command => {
                    if let Ok(command) = command {
                        match command {
                            Command::Quit => {
                                break;
                            }
                        }
                    } else {
                        panic!("Error while receiving ServerLogicCommand");
                    }
                },
                recv(self.listener_to_server_logic_rx) -> message => {
                    if let Ok(message) = message {
                        self.process_message(&message);
                    } else {
                        panic!("Error while receiving a message from listener");
                    }
                },
            }
        }
    }

    fn process_message(&mut self, message: &Message) {
        let session_id = message.session_id;
        let source = message.source;

        match &message.content {
            MessageType::Request(request_type) => {
                self.process_request(session_id, source, request_type);
            }
            MessageType::Response(response_type) => {
                self.process_response(session_id, source, response_type);
            }
            MessageType::Error(error_type) => {
                self.process_error(session_id, source, error_type);
            }
        }
    }

    fn process_error(&self, session_id: u64, source_id: NodeId, error_type: &ErrorType) {
        log::warn!("From node {source_id} with session_id {session_id}, received error {error_type:?}");
    }

    fn process_request(&mut self, session_id: u64, source_id: NodeId, request_type: &RequestType) {
        match request_type {
            RequestType::TextRequest(_) => {
                // TODO
            },
            RequestType::MediaRequest(_) => {
                // TODO
            }
            RequestType::ChatRequest(_) => {
                let content = MessageType::Error(ErrorType::Unsupported(request_type.clone()));
                let response = self.create_message(session_id, source_id, content);
                self.send_message_to_transmitter(response);
            }
            RequestType::DiscoveryRequest(()) => {
                let content = MessageType::Response(ResponseType::DiscoveryResponse(ServerType::ContentServer));
                let response = self.create_message(session_id, source_id, content);
                self.send_message_to_transmitter(response);
            }
        }
    }

    fn create_message(&self, session_id: u64, destination: NodeId, content: MessageType) -> Message {
        Message {
            source: self.node_id,
            destination,
            session_id,
            content
        }
    }

    fn process_response(&self, session_id: u64, source_id: NodeId, response_type: &ResponseType) {
        let content = MessageType::Error(ErrorType::Unexpected(response_type.clone()));
        let response = self.create_message(session_id, source_id, content);
        self.send_message_to_transmitter(response);
    }

    fn send_message_to_transmitter(&self, message: Message) {
        match self.server_logic_to_transmitter_tx.send(message) {
            Ok(()) => { }
            Err(SendError(message)) => {
                log::error!("Server logic cannot communicate with transmitter");
                panic!("Server logic cannot communicate with transmitter");
            }
        }
    }
}