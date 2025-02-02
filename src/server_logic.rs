mod communication_server;

pub use communication_server::CommunicationServer;

pub enum Command {
    Quit,
}