use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use crossbeam_channel::{unbounded, Receiver, Sender};
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};
use crate::listener::{Listener, ListenerCommand};
use crate::transmitter::Transmitter;

mod transmitter;
mod listener;
mod server_logic;


pub struct NullPointerDibServer {
    transmitter: Arc<Mutex<Transmitter>>,
    listener: Arc<Mutex<Listener>>,
}

impl NullPointerDibServer {
    pub fn new(
        // the server's NodeId
        node_id: NodeId,
        // the channel the server listens on to receive packets from connected drones
        listener_rx: Receiver<Packet>,
        // the HashMap containing every connected drone
        drones_tx: HashMap<NodeId, Sender<Packet>>,
        // TODO: add simulation controller channel
    ) -> Self {
        let (internal_transmitter_to_listener_tx, internal_transmitter_to_listener_rx) = unbounded::<Packet>();
        let (internal_listener_to_transmitter_tx, internal_listener_to_transmitter_rx) = unbounded::<Packet>();
        let (internal_listener_to_server_logic_tx, internal_listener_to_server_logic_rx) = unbounded::<Packet>();
        let (internal_server_logic_to_transmitter_tx, internal_server_logic_to_transmitter_rx) = unbounded::<Packet>();
        let (listener_commands_tx, listener_commands_rx) = unbounded::<ListenerCommand>();

        let transmitter = Transmitter::new(
            node_id,
            NodeType::Server,
            internal_listener_to_transmitter_rx,
            internal_transmitter_to_listener_tx,
            internal_server_logic_to_transmitter_rx,
            drones_tx
        );
        let transmitter = Arc::new(Mutex::new(transmitter));

        let listener = Listener::new(
            node_id,
            internal_listener_to_transmitter_tx,
            internal_transmitter_to_listener_rx,
            internal_listener_to_server_logic_tx,
            listener_rx,
            listener_commands_rx
        );
        let listener = Arc::new(Mutex::new(listener));

        Self {
            transmitter,
            listener
        }
    }

    pub fn run(&mut self) {
        let mut listener_clone = self.listener.clone();
        let listener_handle = thread::spawn(move || {
            listener_clone.lock().unwrap().run()
        });

        let mut transmitter_clone = self.transmitter.clone();
        let transmitter_handle = thread::spawn(move || {
            transmitter_clone.lock().unwrap().run()
        });

        // TODO: listen for commands (only shutdown) (either from channel or from CLI)
        // TODO: send shutdown command to listener and transmitter

        listener_handle.join();
        transmitter_handle.join();
    }
}