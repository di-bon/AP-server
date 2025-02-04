mod communication_server;
mod content_server;

use crossbeam_channel::{select, Receiver, Sender};
use messages::{ErrorType, Message, MessageType, RequestType, ResponseType};
use wg_2024::network::NodeId;
pub use crate::logic::communication_server::CommunicationServer;
pub use crate::logic::content_server::ContentServer;

pub enum Command {
    Quit,
}

pub trait Getter {
    fn get_node_id(&self) -> NodeId;
    fn get_command_rx(&self) -> &Receiver<Command>;
    fn get_listener_to_server_logic_rx(&self) -> &Receiver<Message>;
    fn get_server_logic_to_transmitter_tx(&self) -> &Sender<Message>;
}

pub trait Server: Getter + Send {
    fn run(&mut self) {
        loop {
            select! {
                recv(self.get_command_rx()) -> command => {
                    if let Ok(command) = command {
                        match command {
                            Command::Quit => {
                                break;
                            }
                        }
                    }
                    panic!("Error while receiving ServerLogicCommand");
                },
                recv(self.get_listener_to_server_logic_rx()) -> message => {
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
    fn process_request(&mut self, session_id: u64, source: NodeId,request_type: &RequestType);
    fn process_response(&self, session_id: u64, source_id: NodeId, response_type: &ResponseType) {
        let content = MessageType::Error(ErrorType::Unexpected(response_type.clone()));
        let response = self.create_message(session_id, source_id, content);
        self.send_message_to_transmitter(response);
    }
    fn process_error(&self, session_id: u64, source_id: NodeId, error_type: &ErrorType) {
        log::warn!("From node {source_id} with session_id {session_id}, received error {error_type:?}");
    }
    fn create_message(&self, session_id: u64, destination: NodeId, content: MessageType) -> Message {
        Message {
            source: self.get_node_id(),
            destination,
            session_id,
            content
        }
    }
    fn send_message_to_transmitter(&self, message: Message) {
        match self.get_server_logic_to_transmitter_tx().send(message) {
            Ok(()) => { }
            Err(error) => {
                let error = format!("Logic cannot communicate with transmitter. Error: {error:?}");
                log::error!("{}", error);
                panic!("{}", error);
            }
        }
    }
}