use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use crossbeam_channel::{select, Receiver, Sender};
use tokio::task::JoinHandle;
use wg_2024::network::NodeId;
use wg_2024::packet::{Nack, Packet};
use crate::transmitter::network_controller::NetworkController;
use crate::transmitter::transmission_handler::TransmissionHandler;
use tokio::time::{sleep, Duration};

mod network_controller;
mod transmission_handler;
mod gateway;

#[derive(Debug)]
enum Command {
    Resend(u64),
    Confirmed,
}

struct Transmitter<'a> {
    receiver_channel: Receiver<Nack>,
    server_logic_channel: Receiver<Packet>,
    network_controller: NetworkController<'a>,
    transmission_handler: Rc<TransmissionHandler<'a>>,
    command_channel: Sender<Command>,
    connected_drones: HashMap<NodeId, Receiver<Packet>>
}

impl<'a> Transmitter<'a> {
    // TODO: add parameters
    fn new() -> Self {
        todo!()
    }
}