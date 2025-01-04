use crossbeam_channel::{select, Receiver, Sender};
use messages::{Message, MessageType, RequestType, ResponseType};
use wg_2024::network::NodeId;

pub enum ServerLogicCommand {
    Quit,
}

pub struct ServerLogic {
    server_logic_to_transmitter_tx: Sender<(NodeId, Message)>,
    listener_to_server_logic_rx: Receiver<Message>,
    command_rx: Receiver<ServerLogicCommand>,
}

impl ServerLogic {
    pub fn new(
        server_logic_to_transmitter_tx: Sender<(NodeId, Message)>,
        listener_to_server_logic_rx: Receiver<Message>,
    ) -> Self {
        todo!()
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
                self.process_request(request_type);
            }
            MessageType::Response(response_type) => {
                self.process_response(response_type);
            }
        }
    }

    fn process_request(&self, request_type: &RequestType) {
        match request_type {
            RequestType::TextRequest(text_request) => {
                todo!()
            }
            RequestType::MediaRequest(media_request) => {
                todo!()
            }
            RequestType::ChatRequest(chat_request) => {
                todo!()
            }
            RequestType::DiscoveryRequest(()) => {
                todo!()
            }
        }
    }

    fn process_response(&self, response_type: &ResponseType) {
        match response_type {
            ResponseType::TextResponse(text_response) => {
                todo!()
            }
            ResponseType::MediaResponse(media_response) => {
                todo!()
            }
            ResponseType::ChatResponse(chat_response) => {
                todo!()
            }
            ResponseType::DiscoveryResponse(server_type) => {
                todo!()
            }
        }
    }
}