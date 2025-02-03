use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use crossbeam_channel::{select, Receiver, SendError, Sender};
use messages::{ChatRequest, ChatResponse, ErrorType, MediaRequest, MediaResponse, Message, MessageType, RequestType, ResponseType, ServerType, TextRequest, TextResponse};
use rand::{random, Rng};
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
            text_resources,
            media_resources,
            resources_path,
        };
        result.update_resources();
        result
    }

    fn update_resources(&mut self) {
        let text_resources = Self::get_available_files(&self.resources_path, "txt").unwrap_or_else(|err| {
            log::warn!("No text resources available at {resources_path}");
            vec![]
        });
        let media_resources = Self::get_available_files(&self.resources_path, "png").unwrap_or_else(|err| {
            log::warn!("No media resources available at {resources_path}");
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
            TextRequest::Text(index) => {
                let filename = self.text_resources.get(*index as usize);
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
                                MessageType::Response(ResponseType::TextResponse(TextResponse::NotFound))
                            }
                        }
                    },
                    None => {
                        MessageType::Response(ResponseType::TextResponse(TextResponse::NotFound))
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
            MediaRequest::Media(index) => {
                let filename = self.media_resources.get(*index as usize);
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
                                MessageType::Response(ResponseType::MediaResponse(MediaResponse::NotFound))
                            }
                        }
                    },
                    None => {
                        MessageType::Response(ResponseType::MediaResponse(MediaResponse::NotFound))
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