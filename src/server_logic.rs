use crossbeam_channel::{select, Receiver, SendError, Sender};
use messages::{Message, MessageType, RequestType, ResponseType, TextResponse};
use wg_2024::network::NodeId;

pub enum ServerLogicCommand {
    Quit,
}

pub struct ServerLogic {
    node_id: NodeId,
    server_logic_to_transmitter_tx: Sender<(NodeId, Message)>,
    listener_to_server_logic_rx: Receiver<Message>,
    command_rx: Receiver<ServerLogicCommand>,
}

impl ServerLogic {
    pub fn new(
        node_id: NodeId,
        server_logic_to_transmitter_tx: Sender<(NodeId, Message)>,
        listener_to_server_logic_rx: Receiver<Message>,
        command_rx: Receiver<ServerLogicCommand>
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
                            ServerLogicCommand::Quit => {
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

    fn process_message(&self, message: &Message) {
        let session_id = message.session_id;
        let source_id = message.source_id;

        match &message.content {
            MessageType::Request(request_type) => {
                self.process_request(session_id, source_id, request_type);
            }
            MessageType::Response(response_type) => {
                self.process_response(session_id, source_id, response_type);
            }
        }
    }

    fn process_request(&self, session_id: u64, source_id: NodeId, request_type: &RequestType) {
        match request_type {
            RequestType::TextRequest(text_request) => {
                let content = MessageType::Response(ResponseType::TextResponse(TextResponse::Text(format!("Received {text_request:?}"))));
                let response = self.create_message(session_id, content);
                self.send_message_to_transmitter(source_id, response);
            }
            RequestType::MediaRequest(media_request) => {
                let content = MessageType::Response(ResponseType::TextResponse(TextResponse::Text(format!("Received {media_request:?}"))));
                let response = self.create_message(session_id, content);
                self.send_message_to_transmitter(source_id, response);
            }
            RequestType::ChatRequest(chat_request) => {
                let content = MessageType::Response(ResponseType::TextResponse(TextResponse::Text(format!("Received {chat_request:?}"))));
                let response = self.create_message(session_id, content);
                self.send_message_to_transmitter(source_id, response);
            }
            RequestType::DiscoveryRequest(()) => {
                let content = MessageType::Response(ResponseType::TextResponse(TextResponse::Text(format!("Received {:?}", RequestType::DiscoveryRequest(())))));
                let response = self.create_message(session_id, content);
                self.send_message_to_transmitter(source_id, response);
            }
        }
    }

    fn create_message(&self, session_id: u64, content: MessageType) -> Message {
        Message {
            source_id: self.node_id,
            session_id,
            content
        }
    }

    fn process_response(&self, session_id: u64, source_id: NodeId, response_type: &ResponseType) {
        match response_type {
            ResponseType::TextResponse(text_response) => {
                let content = MessageType::Response(ResponseType::TextResponse(TextResponse::Text(format!("Received {text_response:?}"))));
                let response = self.create_message(session_id, content);
                self.send_message_to_transmitter(source_id, response);
            }
            ResponseType::MediaResponse(media_response) => {
                let content = MessageType::Response(ResponseType::TextResponse(TextResponse::Text(format!("Received {media_response:?}"))));
                let response = self.create_message(session_id, content);
                self.send_message_to_transmitter(source_id, response);
            }
            ResponseType::ChatResponse(chat_response) => {
                let content = MessageType::Response(ResponseType::TextResponse(TextResponse::Text(format!("Received {chat_response:?}"))));
                let response = self.create_message(session_id, content);
                self.send_message_to_transmitter(source_id, response);
            }
            ResponseType::DiscoveryResponse(server_type) => {
                let content = MessageType::Response(ResponseType::TextResponse(TextResponse::Text(format!("Received {server_type:?}"))));
                let response = self.create_message(session_id, content);
                self.send_message_to_transmitter(source_id, response);
            }
        }
    }

    fn send_message_to_transmitter(&self, destination:NodeId, message: Message) {
        match self.server_logic_to_transmitter_tx.send((destination, message)) {
            Ok(()) => { }
            Err(SendError((destination, message))) => {
                log::error!("Server logic cannot communicate with transmitter");
                panic!("Server logic cannot communicate with transmitter");
            }
        }
    }
}