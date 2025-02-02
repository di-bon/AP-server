use std::collections::{HashMap, HashSet};
use crossbeam_channel::{select, Receiver, SendError, Sender};
use messages::{ChatRequest, ChatResponse, ErrorType, Message, MessageType, RequestType, ResponseType, ServerType, TextResponse};
use rand::{random, Rng};
use wg_2024::network::NodeId;
use crate::server_logic::Command;

pub struct CommunicationServer {
    node_id: NodeId,
    server_logic_to_transmitter_tx: Sender<Message>,
    listener_to_server_logic_rx: Receiver<Message>,
    command_rx: Receiver<Command>,
    registered: HashSet<NodeId>,
    // messages: HashMap<NodeId, Vec<MessageInfo>>,
}

// struct MessageInfo {
//     from: NodeId,
//     message: String,
// }
//
// impl MessageInfo {
//     fn new(from: NodeId, message: String) -> Self {
//         Self {
//             from, message
//         }
//     }
// }

impl CommunicationServer {
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
            registered: HashSet::new(),
            // messages: HashMap::new(),
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
            RequestType::TextRequest(_)
            | RequestType::MediaRequest(_) => {
                let content = MessageType::Error(ErrorType::Unsupported(request_type.clone()));
                let response = self.create_message(session_id, source_id, content);
                self.send_message_to_transmitter(response);
            }
            RequestType::ChatRequest(chat_request) => {
                match chat_request {
                    ChatRequest::ClientList => {
                        let list: Vec<NodeId> = self.registered.iter().copied().collect();
                        let content = MessageType::Response(ResponseType::ChatResponse(ChatResponse::ClientList(list)));
                        let response = self.create_message(session_id, source_id, content);
                        self.send_message_to_transmitter(response);
                    }
                    ChatRequest::Register(node_id) => {
                        self.registered.insert(*node_id);
                    }
                    ChatRequest::SendMessage { from, to, message } => {
                        if !self.is_registered(*from) {
                            self.send_unregistered_error(session_id, *from);
                            return;
                        }
                        if !self.is_registered(*to) {
                            self.send_unregistered_error(session_id, *to);
                            return;
                        }

                        // let message_info = MessageInfo::new(*from, message);
                        // let mut entry = self.messages.entry(*to).or_insert(Vec::new());
                        // entry.push(message_info);

                        let mut rng = rand::rng();
                        let forward_session_id: u64 = rng.random();
                        let content = MessageType::Response(ResponseType::ChatResponse(ChatResponse::MessageFrom {
                            from: *from,
                            message: message.clone(),
                        }));
                        let message = self.create_message(forward_session_id, *to, content);
                        self.send_message_to_transmitter(message);

                        let content = MessageType::Response(ResponseType::ChatResponse(ChatResponse::MessageSent));
                        let confirmation = self.create_message(session_id, *from, content);
                        self.send_message_to_transmitter(confirmation);
                    }
                }
            }
            RequestType::DiscoveryRequest(()) => {
                let content = MessageType::Response(ResponseType::DiscoveryResponse(ServerType::CommunicationServer));
                let response = self.create_message(session_id, source_id, content);
                self.send_message_to_transmitter(response);
            }
        }
    }

    fn send_unregistered_error(&mut self, session_id: u64, from: NodeId) {
        let content = MessageType::Error(ErrorType::Unregistered(from));
        let error_message = self.create_message(session_id, from, content);
        self.send_message_to_transmitter(error_message);
    }

    fn is_registered(&self, id: NodeId) -> bool {
        self.registered.contains(&id)
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