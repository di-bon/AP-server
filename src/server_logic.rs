mod communication_server;
mod content_server;

use wg_2024::network::NodeId;
pub use communication_server::CommunicationServer;
use crate::server_logic::content_server::ContentServer;

pub enum Command {
    Quit,
}

pub enum Type {
    Communication,
    Content,
}

pub struct ServerLogic {
    node_id: NodeId,
}

impl ServerLogic {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id
        }
    }

    pub fn run(&mut self) {
        loop {

        }
    }
}