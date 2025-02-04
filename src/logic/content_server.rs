use std::fmt::format;
use std::fs;
use std::path::Path;
use crossbeam_channel::{Receiver, Sender};
use messages::{ErrorType, MediaRequest, MediaResponse, Message, MessageType, RequestType, ResponseType, ServerType, TextRequest, TextResponse};
use wg_2024::network::NodeId;
use crate::logic::{Command, Getter, Server};

pub struct ContentServer {
    node_id: NodeId,
    server_logic_to_transmitter_tx: Sender<Message>,
    listener_to_server_logic_rx: Receiver<Message>,
    command_rx: Receiver<Command>,
    text_resources: Vec<String>,
    media_resources: Vec<String>,
    resources_path: String,
}

impl Getter for ContentServer {
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

impl Server for ContentServer {
    fn process_request(&mut self, session_id: u64, source: NodeId, request_type: &RequestType) {
        match request_type {
            RequestType::TextRequest(text_request) => {
                self.process_text_request(session_id, source, text_request);
            },
            RequestType::MediaRequest(media_request) => {
                self.process_media_request(session_id, source, media_request);
            }
            RequestType::ChatRequest(_) => {
                let content = MessageType::Error(ErrorType::Unsupported(request_type.clone()));
                let response = self.create_message(session_id, source, content);
                self.send_message_to_transmitter(response);
            }
            RequestType::DiscoveryRequest(()) => {
                let content = MessageType::Response(ResponseType::DiscoveryResponse(ServerType::ContentServer));
                let response = self.create_message(session_id, source, content);
                self.send_message_to_transmitter(response);
            }
        }
    }
}

impl ContentServer {
    pub fn new(
        node_id: NodeId,
        server_logic_to_transmitter_tx: Sender<Message>,
        listener_to_server_logic_rx: Receiver<Message>,
        command_rx: Receiver<Command>,
        resources_path: String,
    ) -> Self {
        let mut result = Self {
            node_id,
            server_logic_to_transmitter_tx,
            listener_to_server_logic_rx,
            command_rx,
            text_resources: vec![],
            media_resources: vec![],
            resources_path,
        };
        result.update_resources();
        result
    }

    fn update_resources(&mut self) {
        let text_resources = Self::get_available_files(&self.resources_path, "txt").unwrap_or_else(|err| {
            log::warn!("No text resources available at {}. Reason: {err:?}", self.resources_path);
            vec![]
        });
        let media_resources = Self::get_available_files(&self.resources_path, "png").unwrap_or_else(|err| {
            log::warn!("No media resources available at {}. Reason: {err:?}", self.resources_path);
            vec![]
        });
        self.text_resources = text_resources;
        self.media_resources = media_resources;
    }

    fn process_text_request(&mut self, session_id: u64, source: NodeId, text_request: &TextRequest) {
        let content = match text_request {
            TextRequest::TextList => {
                MessageType::Response(
                    ResponseType::TextResponse(
                        TextResponse::TextList(
                            self.text_resources.clone()
                        )
                    )
                )
            }
            TextRequest::Text(requested_file) => {
                let filename = self.text_resources.iter().find(|res| *res == requested_file);
                match filename {
                    Some(filename) => {
                        match self.read_file(filename) {
                            Ok(text) => {
                                MessageType::Response(
                                    ResponseType::TextResponse(
                                        TextResponse::Text(
                                            text
                                        )
                                    )
                                )
                            }
                            Err(error) => {
                                log::warn!("Error while reading file {filename}. Error: {error:?}");
                                MessageType::Response(ResponseType::TextResponse(TextResponse::NotFound(requested_file.clone())))
                            }
                        }
                    },
                    None => {
                        MessageType::Response(ResponseType::TextResponse(TextResponse::NotFound(requested_file.clone())))
                    },
                }
            }
        };
        let message = self.create_message(session_id, source, content);
        self.send_message_to_transmitter(message);
    }

    fn read_file(&self, filename: &str) -> std::io::Result<String> {
        let file_path = Path::new(&self.resources_path).join(filename);
        fs::read_to_string(file_path)
    }

    fn read_file_as_bytes(&self, filename: &str) -> std::io::Result<Vec<u8>> {
        let file_path = Path::new(&self.resources_path).join(filename);
        fs::read(file_path)
    }

    fn process_media_request(&mut self, session_id: u64, source: NodeId, media_request: &MediaRequest) {
        let content = match media_request {
            MediaRequest::MediaList => {
                MessageType::Response(
                    ResponseType::MediaResponse(
                        MediaResponse::MediaList(
                            self.media_resources.clone()
                        )
                    )
                )
            }
            MediaRequest::Media(requested_media) => {
                let filename = self.media_resources.iter().find(|res| *res == requested_media);
                match filename {
                    Some(filename) => {
                        match self.read_file_as_bytes(filename) {
                            Ok(media_bytes) => {
                                MessageType::Response(
                                    ResponseType::MediaResponse(
                                        MediaResponse::Media(
                                            media_bytes
                                        )
                                    )
                                )
                            }
                            Err(error) => {
                                log::warn!("Error while reading file {filename}. Error: {error:?}");
                                MessageType::Response(ResponseType::MediaResponse(MediaResponse::NotFound(requested_media.clone())))
                            }
                        }
                    },
                    None => {
                        MessageType::Response(ResponseType::MediaResponse(MediaResponse::NotFound(requested_media.clone())))
                    },
                }
            }
        };
        let message = self.create_message(session_id, source, content);
        self.send_message_to_transmitter(message);
    }

    fn get_available_files(path: &str, required_extension: &str) -> std::io::Result<Vec<String>> {
        let path = Path::new(&path);
        let mut files = Vec::new();

        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let file_path = entry.path();

                if file_path.is_file() {
                    if let Some(extension) = file_path.extension() {
                        if extension == required_extension {
                            if let Some(file_name) = file_path.file_name() {
                                files.push(file_name.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
            }
        }

        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread;
    use crossbeam_channel::unbounded;
    use messages::ChatRequest;
    use super::*;

    fn create_content_server(node_id: NodeId, resources_path: String) -> (
        ContentServer,
        Receiver<Message>,
        Sender<Message>,
        Sender<Command>,
    ) {
        let (logic_to_transmitter_tx, logic_to_transmitter_rx) = unbounded();
        let (listener_to_logic_tx, listener_to_logic_rx) = unbounded();
        let (command_tx, command_rx) = unbounded();

        let server = ContentServer::new(
            node_id,
            logic_to_transmitter_tx,
            listener_to_logic_rx,
            command_rx,
            resources_path,
        );
        (server, logic_to_transmitter_rx, listener_to_logic_tx, command_tx)
    }

    #[test]
    fn initialize() {
        let node_id = 0;
        let resources_path = "./res".to_string();

        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx) = create_content_server(node_id, resources_path.clone());

        assert_eq!(server.node_id, node_id);
        let mut expected_text_resources = vec!["the quacking duck.txt".to_string(), "rust.txt".to_string()];
        expected_text_resources.sort();
        let mut server_text_resources = server.text_resources.clone();
        server_text_resources.sort();
        assert_eq!(server_text_resources, expected_text_resources);
        let mut expected_media_resources: Vec<String> = vec!["ferris.png".to_string()];
        expected_media_resources.sort();
        let mut server_media_resources = server.media_resources.clone();
        server_media_resources.sort();
        assert_eq!(server_media_resources, expected_media_resources);
        assert_eq!(server.resources_path, resources_path);
    }

    #[test]
    fn check_text_request() {
        let node_id = 0;
        let resources_path = "./res".to_string();

        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx) = create_content_server(node_id, resources_path.clone());

        let server = Arc::new(Mutex::new(server));
        let server_clone = server.clone();

        thread::spawn(move || {
            server_clone.lock().unwrap().run();
        });

        let sender = 1;
        let text_list_request = Message {
            source: sender,
            destination: node_id,
            session_id: 0,
            content: MessageType::Request(RequestType::TextRequest(TextRequest::TextList)),
        };

        let _ = listener_to_logic_tx.send(text_list_request);

        let response = logic_to_transmitter_rx.recv().unwrap();

        let panic_message = "Wrong response";
        let mut response_text_list = match response.content {
            MessageType::Response(response) => {
                match response {
                    ResponseType::TextResponse(text_response) => {
                        match text_response {
                            TextResponse::TextList(list) => list,
                            _ => panic!("{panic_message}"),
                        }
                    },
                    _ => panic!("{panic_message}"),
                }
            }
            _ => panic!("{panic_message}"),
        };
        response_text_list.sort();

        let mut expected_text_list = vec!["the quacking duck.txt".to_string(), "rust.txt".to_string()];
        expected_text_list.sort();

        assert_eq!(response_text_list, expected_text_list);

        let session_id = 10;
        let text_request = Message {
            source: sender,
            destination: node_id,
            session_id,
            content: MessageType::Request(RequestType::TextRequest(TextRequest::Text("the quacking duck.txt".to_string()))),
        };

        let _ = listener_to_logic_tx.send(text_request);

        let response = logic_to_transmitter_rx.recv().unwrap();

        let quacking_duck_content = fs::read_to_string("./res/the quacking duck.txt").unwrap();
        let expected = Message {
            source: node_id,
            destination: sender,
            session_id,
            content: MessageType::Response(ResponseType::TextResponse(TextResponse::Text(quacking_duck_content))),
        };

        assert_eq!(response, expected);
    }

    #[test]
    fn check_text_not_found() {
        let node_id = 0;
        let resources_path = "./res".to_string();

        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx) = create_content_server(node_id, resources_path.clone());

        let server = Arc::new(Mutex::new(server));
        let server_clone = server.clone();

        thread::spawn(move || {
            server_clone.lock().unwrap().run();
        });

        let sender = 1;

        let session_id = 10;
        let text_request = Message {
            source: sender,
            destination: node_id,
            session_id,
            content: MessageType::Request(RequestType::TextRequest(TextRequest::Text("boh.txt".to_string()))),
        };

        let _ = listener_to_logic_tx.send(text_request);

        let response = logic_to_transmitter_rx.recv().unwrap();

        let expected = Message {
            source: node_id,
            destination: sender,
            session_id,
            content: MessageType::Response(ResponseType::TextResponse(TextResponse::NotFound("boh.txt".to_string()))),
        };

        assert_eq!(response, expected);
    }

    #[test]
    fn check_media_request() {
        let node_id = 0;
        let resources_path = "./res".to_string();

        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx) = create_content_server(node_id, resources_path.clone());

        let server = Arc::new(Mutex::new(server));
        let server_clone = server.clone();

        thread::spawn(move || {
            server_clone.lock().unwrap().run();
        });

        let sender = 1;
        let media_list_request = Message {
            source: sender,
            destination: node_id,
            session_id: 0,
            content: MessageType::Request(RequestType::MediaRequest(MediaRequest::MediaList)),
        };

        let _ = listener_to_logic_tx.send(media_list_request);

        let response = logic_to_transmitter_rx.recv().unwrap();

        let panic_message = "Wrong response";
        let mut response_media_list = match response.content {
            MessageType::Response(response) => {
                match response {
                    ResponseType::MediaResponse(media_response) => {
                        match media_response {
                            MediaResponse::MediaList(list) => list,
                            _ => panic!("{panic_message}"),
                        }
                    },
                    _ => panic!("{panic_message}"),
                }
            }
            _ => panic!("{panic_message}"),
        };
        response_media_list.sort();

        let mut expected_media_list = vec!["ferris.png".to_string()];
        expected_media_list.sort();

        assert_eq!(response_media_list, expected_media_list);

        let session_id = 10;
        let media_request = Message {
            source: sender,
            destination: node_id,
            session_id,
            content: MessageType::Request(RequestType::MediaRequest(MediaRequest::Media("ferris.png".to_string()))),
        };

        let _ = listener_to_logic_tx.send(media_request);

        let response = logic_to_transmitter_rx.recv().unwrap();

        let ferris_content = fs::read("./res/ferris.png").unwrap();
        let expected = Message {
            source: node_id,
            destination: sender,
            session_id,
            content: MessageType::Response(ResponseType::MediaResponse(MediaResponse::Media(ferris_content))),
        };

        assert_eq!(response, expected);
    }

    #[test]
    fn check_media_not_found() {
        let node_id = 0;
        let resources_path = "./res".to_string();

        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx) = create_content_server(node_id, resources_path.clone());

        let server = Arc::new(Mutex::new(server));
        let server_clone = server.clone();

        thread::spawn(move || {
            server_clone.lock().unwrap().run();
        });

        let sender = 1;

        let session_id = 10;
        let media_request = Message {
            source: sender,
            destination: node_id,
            session_id,
            content: MessageType::Request(RequestType::MediaRequest(MediaRequest::Media("boh.png".to_string()))),
        };

        let _ = listener_to_logic_tx.send(media_request);

        let response = logic_to_transmitter_rx.recv().unwrap();

        let expected = Message {
            source: node_id,
            destination: sender,
            session_id,
            content: MessageType::Response(ResponseType::MediaResponse(MediaResponse::NotFound("boh.png".to_string()))),
        };

        assert_eq!(response, expected);
    }

    #[test]
    fn check_unsupported_message() {
        let node_id = 0;
        let resources_path = "./res".to_string();

        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx) = create_content_server(node_id, resources_path.clone());

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
            content: MessageType::Request(RequestType::ChatRequest(ChatRequest::ClientList)),
        };

        let _ = listener_to_logic_tx.send(unsupported_message);

        let response = logic_to_transmitter_rx.recv().unwrap();
        let expected = Message {
            source: node_id,
            destination: sender,
            session_id: 0,
            content: MessageType::Error(ErrorType::Unsupported(RequestType::ChatRequest(ChatRequest::ClientList))),
        };
        assert_eq!(response, expected);
    }

    #[test]
    fn check_discovery() {
        let node_id = 0;
        let resources_path = "./res".to_string();

        let (server,
            logic_to_transmitter_rx,
            listener_to_logic_tx,
            command_tx) = create_content_server(node_id, resources_path.clone());

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
            content: MessageType::Response(ResponseType::DiscoveryResponse(ServerType::ContentServer)),
        };
        assert_eq!(response, expected);
    }
}