use std::collections::HashSet;
use crossbeam_channel::{Receiver, Sender};
use messages::{ChatRequest, ChatResponse, ErrorType, Message, MessageType, RequestType, ResponseType, ServerType};
use rand::Rng;
use wg_2024::network::NodeId;
use crate::logic::{ServerCommand as ServerCommand, Getter, Server};

#[derive(Debug)]
pub struct CommunicationServer {
    node_id: NodeId,
    server_logic_to_transmitter_tx: Sender<Message>,
    listener_to_server_logic_rx: Receiver<Message>,
    server_command_rx: Receiver<ServerCommand>,
    registered: HashSet<NodeId>,
}

impl Getter for CommunicationServer {
    fn get_node_id(&self) -> NodeId {
        self.node_id
    }

    fn get_server_command_rx(&self) -> &Receiver<ServerCommand> {
        &self.server_command_rx
    }

    fn get_listener_to_server_logic_rx(&self) -> &Receiver<Message> {
        &self.listener_to_server_logic_rx
    }

    fn get_server_logic_to_transmitter_tx(&self) -> &Sender<Message> {
        &self.server_logic_to_transmitter_tx
    }
}

impl Server for CommunicationServer {
    /// Processes a `RequestType`
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
                    ChatRequest::Register => {
                        self.registered.insert(source);
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
        command_rx: Receiver<ServerCommand>,
    ) -> Self {
        Self {
            node_id,
            server_logic_to_transmitter_tx,
            listener_to_server_logic_rx,
            server_command_rx: command_rx,
            registered: HashSet::new(),
        }
    }

    /// Returns whether the given `id` is registered to the server
    fn is_registered(&self, id: NodeId) -> bool {
        self.registered.contains(&id)
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_variables)]
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;
    use crossbeam_channel::unbounded;
    use messages::MediaRequest;
    use ntest::assert_false;
    use wg_2024::controller::DroneCommand;
    use super::*;

    fn create_communication_server(node_id: NodeId) -> (CommunicationServer, Receiver<Message>, Sender<Message>, Sender<ServerCommand>, Sender<DroneCommand>, Receiver<DroneCommand>) {
        let (logic_to_transmitter_tx, logic_to_transmitter_rx) = unbounded();
        let (listener_to_logic_tx, listener_to_logic_rx) = unbounded();
        let (command_tx, command_rx) = unbounded();
        let (drone_command_tx, drone_command_rx) = unbounded();
        let (server_to_transmitter_drone_command_tx, server_to_transmitter_drone_command_rx) = unbounded();

        let server = CommunicationServer::new(
            node_id,
            logic_to_transmitter_tx,
            listener_to_logic_rx,
            command_rx,
            // drone_command_rx,
            // server_to_transmitter_drone_command_tx
        );
        (server, logic_to_transmitter_rx, listener_to_logic_tx, command_tx, drone_command_tx, server_to_transmitter_drone_command_rx)
    }
    #[test]
    fn initialize() {
        let node_id = 0;
        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx,
            drone_command_tx,
            server_to_transmitter_drone_command_rx) = create_communication_server(node_id);

        assert_eq!(server.node_id, node_id);
    }

    #[test]
    fn check_register() {
        let node_id = 0;
        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx,
            drone_command_tx,
            server_to_transmitter_drone_command_rx) = create_communication_server(node_id);

        let source: NodeId = 10;
        assert_false!(server.is_registered(source));

        let register_request = Message {
            source,
            destination: 0,
            session_id: 0,
            content: MessageType::Request(RequestType::ChatRequest(ChatRequest::Register)),
        };
        let _ = listener_to_logic_tx.send(register_request);

        let server = Arc::new(Mutex::new(server));
        let server_clone = server.clone();

        thread::spawn(move || {
            server_clone.lock().unwrap().run();
        });

        thread::sleep(Duration::from_millis(20));

        let _ = command_tx.send(ServerCommand::Quit);

        assert!(server.lock().unwrap().is_registered(source));
    }

    #[test]
    fn check_client_list() {
        let node_id = 0;
        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx,
            drone_command_tx,
            server_to_transmitter_drone_command_rx) = create_communication_server(node_id);

        for node_id in 3..6 {
            let register_request = Message {
                source: node_id,
                destination: 0,
                session_id: 0,
                content: MessageType::Request(RequestType::ChatRequest(ChatRequest::Register)),
            };
            let _ = listener_to_logic_tx.send(register_request);
        }

        let source = 10;
        let client_list_request = Message {
            source,
            destination: node_id,
            session_id: 0,
            content: MessageType::Request(RequestType::ChatRequest(ChatRequest::ClientList)),
        };
        let _ = listener_to_logic_tx.send(client_list_request).unwrap();

        let server = Arc::new(Mutex::new(server));
        let server_clone = server.clone();

        thread::spawn(move || {
            server_clone.lock().unwrap().run();
        });

        thread::sleep(Duration::from_millis(20));

        let _ = command_tx.send(ServerCommand::Quit);

        for node_id in 3..6 {
            assert!(server.lock().unwrap().is_registered(node_id));
        }

        assert_false!(server.lock().unwrap().is_registered(2));
        assert_false!(server.lock().unwrap().is_registered(6));

        let response = logic_to_transmitter_rx.recv().unwrap();

        let expected_list: Vec<NodeId> = vec![3, 4, 5];
        let session_id = 0;
        let expected = Message {
            source: node_id,
            destination: source,
            session_id,
            content: MessageType::Response(ResponseType::ChatResponse(ChatResponse::ClientList(expected_list.clone()))),
        };

        assert_eq!(response.source, node_id);
        assert_eq!(response.destination, source);
        assert_eq!(response.session_id, session_id);

        let panic_message = "Wrong response";
        match response.content {
            MessageType::Response(response_type) => {
                match response_type {
                    ResponseType::ChatResponse(chat_response) => {
                        match chat_response {
                            ChatResponse::ClientList(mut list) => {
                                list.sort();
                                assert_eq!(list, expected_list);
                            },
                            _ => panic!("{panic_message}")
                        }
                    },
                    _ => panic!("{panic_message}")
                }
            },
            _ => panic!("{panic_message}")
        }
    }

    #[test]
    fn check_send_message() {
        let node_id = 0;
        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx,
            drone_command_tx,
            server_to_transmitter_drone_command_rx) = create_communication_server(node_id);

        let sender_node = 1;
        let session_id = 12;
        let register_request = Message {
            source: sender_node,
            destination: node_id,
            session_id,
            content: MessageType::Request(RequestType::ChatRequest(ChatRequest::Register)),
        };
        let _ = listener_to_logic_tx.send(register_request);

        let receiver_node = 2;
        let session_id = 47;
        let register_request = Message {
            source: receiver_node,
            destination: node_id,
            session_id,
            content: MessageType::Request(RequestType::ChatRequest(ChatRequest::Register)),
        };
        let _ = listener_to_logic_tx.send(register_request);

        let session_id = 90;
        let message_string = "Quack".to_string();
        let send_message_request = Message {
            source: sender_node,
            destination: node_id,
            session_id,
            content: MessageType::Request(RequestType::ChatRequest(ChatRequest::SendMessage {
                from: sender_node,
                to: receiver_node,
                message: message_string.clone(),
            })),
        };
        let _ = listener_to_logic_tx.send(send_message_request);

        let server = Arc::new(Mutex::new(server));
        let server_clone = server.clone();

        thread::spawn(move || {
            server_clone.lock().unwrap().run();
        });

        let forwarded_message = logic_to_transmitter_rx.recv().unwrap();
        let expected_content = MessageType::Response(ResponseType::ChatResponse(ChatResponse::MessageFrom {
            from: sender_node,
            message: message_string.clone(),
        }));

        assert_eq!(forwarded_message.source, node_id);
        assert_eq!(forwarded_message.destination, receiver_node);
        assert_eq!(forwarded_message.content, expected_content);

        let response = logic_to_transmitter_rx.recv().unwrap();
        let expected_content = MessageType::Response(ResponseType::ChatResponse(ChatResponse::MessageSent));
        let expected_response = Message {
            source: node_id,
            destination: sender_node,
            session_id,
            content: expected_content,
        };
        assert_eq!(response, expected_response);
    }

    #[test]
    fn check_unregistered_send_message() {
        let node_id = 0;
        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx,
            drone_command_tx,
            server_to_transmitter_drone_command_rx) = create_communication_server(node_id);

        let server = Arc::new(Mutex::new(server));
        let server_clone = server.clone();

        thread::spawn(move || {
            server_clone.lock().unwrap().run();
        });

        let sender_node = 1;
        let receiver_node = 2;

        let send_message_request_session_id = 90;
        let message_string = "Quack".to_string();
        let send_message_request = Message {
            source: sender_node,
            destination: node_id,
            session_id: send_message_request_session_id,
            content: MessageType::Request(RequestType::ChatRequest(ChatRequest::SendMessage {
                from: sender_node,
                to: receiver_node,
                message: message_string.clone(),
            })),
        };
        let _ = listener_to_logic_tx.send(send_message_request.clone());

        let expected = Message {
            source: node_id,
            destination: sender_node,
            session_id: send_message_request_session_id,
            content: MessageType::Error(ErrorType::Unregistered(sender_node)),
        };
        let response = logic_to_transmitter_rx.recv().unwrap();
        assert_eq!(response, expected);

        let session_id = 12;
        let register_request = Message {
            source: sender_node,
            destination: node_id,
            session_id,
            content: MessageType::Request(RequestType::ChatRequest(ChatRequest::Register)),
        };
        let _ = listener_to_logic_tx.send(register_request);

        let _ = listener_to_logic_tx.send(send_message_request.clone());

        let expected = Message {
            source: node_id,
            destination: sender_node,
            session_id: send_message_request_session_id,
            content: MessageType::Error(ErrorType::Unregistered(receiver_node)),
        };
        let response = logic_to_transmitter_rx.recv().unwrap();
        assert_eq!(response, expected);
    }

    #[test]
    fn check_unsupported_message() {
        let node_id = 0;
        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx,
            drone_command_tx,
            server_to_transmitter_drone_command_rx) = create_communication_server(node_id);

        let server = Arc::new(Mutex::new(server));
        let server_clone = server.clone();

        thread::spawn(move || {
            server_clone.lock().unwrap().run();
        });

        let sender = 1;
        let unsupported_message = Message {
            source: sender,
            destination: node_id,
            session_id: 0,
            content: MessageType::Request(RequestType::MediaRequest(MediaRequest::MediaList)),
        };

        let _ = listener_to_logic_tx.send(unsupported_message);

        let response = logic_to_transmitter_rx.recv().unwrap();
        let expected = Message {
            source: node_id,
            destination: sender,
            session_id: 0,
            content: MessageType::Error(ErrorType::Unsupported(RequestType::MediaRequest(MediaRequest::MediaList))),
        };
        assert_eq!(response, expected);
    }

    #[test]
    fn check_discovery() {
        let node_id = 0;
        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx,
            drone_command_tx,
            server_to_transmitter_drone_command_rx) = create_communication_server(node_id);

        let server = Arc::new(Mutex::new(server));
        let server_clone = server.clone();

        thread::spawn(move || {
            server_clone.lock().unwrap().run();
        });

        let sender = 1;
        let discovery_request = Message {
            source: sender,
            destination: node_id,
            session_id: 0,
            content: MessageType::Request(RequestType::DiscoveryRequest(())),
        };

        let _ = listener_to_logic_tx.send(discovery_request);

        let response = logic_to_transmitter_rx.recv().unwrap();
        let expected = Message {
            source: node_id,
            destination: sender,
            session_id: 0,
            content: MessageType::Response(ResponseType::DiscoveryResponse(ServerType::CommunicationServer)),
        };
        assert_eq!(response, expected);
    }
}