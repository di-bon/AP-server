mod communication_server;
mod content_server;

pub use crate::logic::communication_server::CommunicationServer;
pub use crate::logic::content_server::ContentServer;
use crossbeam_channel::{select, Receiver, Sender};
use messages::{ErrorType, Message, MessageType, RequestType, ResponseType};
use wg_2024::network::NodeId;

pub enum ServerCommand {
    Quit,
}

pub trait Getter {
    fn get_node_id(&self) -> NodeId;
    fn get_server_command_rx(&self) -> &Receiver<ServerCommand>;
    fn get_listener_to_server_logic_rx(&self) -> &Receiver<Message>;
    fn get_server_logic_to_transmitter_tx(&self) -> &Sender<Message>;
}

pub trait Server: Getter + Send {
    /// Starts the server, making it able to receive and send `Message`s
    fn run(&mut self) {
        loop {
            select! {
                recv(self.get_server_command_rx()) -> command => {
                    if let Ok(command) = command {
                        match command {
                            ServerCommand::Quit => {
                                break;
                            },
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

    /// Processes a received `Message`
    fn process_message(&mut self, message: &Message) {
        log::info!("Received message {message:?}");

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

    /// Processes a received `RequestType`
    fn process_request(&mut self, session_id: u64, source: NodeId, request_type: &RequestType);

    /// Processes a received `ResponseType`
    fn process_response(&self, session_id: u64, source_id: NodeId, response_type: &ResponseType) {
        let content = MessageType::Error(ErrorType::Unexpected(response_type.clone()));
        let response = self.create_message(session_id, source_id, content);
        self.send_message_to_transmitter(response);
    }

    /// Processes a received `ErrorType`. There is not much to do, so the error just gets logged and then ignored
    fn process_error(&self, session_id: u64, source_id: NodeId, error_type: &ErrorType) {
        log::warn!(
            "From node {source_id} with session_id {session_id}, received error {error_type:?}"
        );
    }

    /// Creates a `Message` with the passed arguments
    fn create_message(
        &self,
        session_id: u64,
        destination: NodeId,
        content: MessageType,
    ) -> Message {
        Message {
            source: self.get_node_id(),
            destination,
            session_id,
            content,
        }
    }

    /// Sends a `Message` to `Transmitter`
    /// # Panics
    /// Panics if the communication fails
    fn send_message_to_transmitter(&self, message: Message) {
        match self.get_server_logic_to_transmitter_tx().send(message) {
            Ok(()) => {}
            Err(error) => {
                panic!("Logic cannot communicate with transmitter. Error: {error:?}");
            }
        }
    }
}
