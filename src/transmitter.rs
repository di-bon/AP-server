use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use crossbeam_channel::{select, Receiver, Sender};
use tokio::task::JoinHandle;
use wg_2024::network::NodeId;
use wg_2024::packet::{Nack, Packet};
use crate::transmitter::network_controller::NetworkController;
use tokio::time::{sleep, Duration};
use crate::transmitter::transmission_handler::TransmissionHandler;

mod network_controller;
mod transmission_handler_async;
mod gateway;
mod transmission_handler;

#[derive(Debug)]
enum Command {
    Resend(u64),
    Confirmed(u64),
}

struct Transmitter {
    // listener -> transmitter
    listener_channel: Receiver<Packet>, // receives ACKs, NACKs, FloodRequest and FloodResponse
    // server logic -> transmitter
    server_logic_channel: Receiver<Packet>, // HL message!
    network_controller: NetworkController,
    // transmitter -> transmission handlers
    transmission_handlers: HashMap<u64, (Sender<Command>, TransmissionHandler)>,
    // server -> drones - just for gateway initialisation
    connected_drones: HashMap<NodeId, Receiver<Packet>>
}

impl Transmitter {
    // TODO: add parameters
    fn new() -> Self {
        todo!()
    }
}