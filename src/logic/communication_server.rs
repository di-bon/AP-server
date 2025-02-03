use std::collections::{HashMap, HashSet};
use crossbeam_channel::{select, Receiver, SendError, Sender};
use messages::{ChatRequest, ChatResponse, ErrorType, Message, MessageType, RequestType, ResponseType, ServerType, TextResponse};
use rand::{random, Rng};
use wg_2024::network::NodeId;
use crate::logic::{Command, Getter, Server};

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

impl Getter for CommunicationServer {
    fn get_node_id(&self) -> NodeId {
        self.node_id
    }

    fn get_command_rx(&self) -> &Receiver<Command> {
        &self.command_rx
    }

    fn get_listener_to_server_logic_rx(&self) -> &Receiver<Message> {
        &self.listener_to_server_logic_rx
    }
    fn get_server_logic_to_transmitter_tx(&self) -> &Sender<Message> {
        &self.server_logic_to_transmitter_tx
    }
}

impl Server for CommunicationServer {
    fn process_request(&mut self, session_id: u64, source: NodeId, request_type: &RequestType) {
        match request_type {
            RequestType::TextRequest(_)
            | RequestType::MediaRequest(_) => {
                let content = MessageType::Error(ErrorType::Unsupported(request_type.clone()));
                let response = self.create_message(session_id, source, content);
                self.send_message_to_transmitter(response);
            }
            RequestType::ChatRequest(chat_request) => {
                match chat_request {
                    ChatRequest::ClientList => {
                        let list: Vec<NodeId> = self.registered.iter().copied().collect();
                        let content = MessageType::Response(ResponseType::ChatResponse(ChatResponse::ClientList(list)));
                        let response = self.create_message(session_id, source, content);
                        self.send_message_to_transmitter(response);
                    }
                    ChatRequest::Register(node_id) => {
                        self.registered.insert(*node_id);
                    }
                    ChatRequest::SendMessage { from, to, message } => {
                        if !self.is_registered(*from) {
                            let content = MessageType::Error(ErrorType::Unregistered(*from));
                            let message = self.create_message(session_id, *from, content);
                            self.send_message_to_transmitter(message);

                            return;
                        }
                        if !self.is_registered(*to) {
                            let content = MessageType::Error(ErrorType::Unregistered(*to));
                            let message = self.create_message(session_id, *from, content);
                            self.send_message_to_transmitter(message);

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
                let response = self.create_message(session_id, source, content);
                self.send_message_to_transmitter(response);
            }
        }
    }
}

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

    fn is_registered(&self, id: NodeId) -> bool {
        self.registered.contains(&id)
    }
}