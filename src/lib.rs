// TODO: remove this when project is finished
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_must_use)]
#![allow(clippy::missing_panics_doc)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use crossbeam_channel::{unbounded, Receiver, Sender};
use messages::Message;
use messages::node_event::NodeEvent;
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};
use crate::listener::{Listener, ListenerCommand};
use crate::server_logic::ServerLogic;
use crate::simulation_controller_notifier::SimulationControllerNotifier;
use crate::transmitter::{Transmitter, TransmitterCommand};

mod transmitter;
mod listener;
mod server_logic;
mod simulation_controller_notifier;

pub struct NullPointerDibServer {
    transmitter: Arc<Mutex<Transmitter>>,
    listener: Arc<Mutex<Listener>>,
    server_logic: Arc<Mutex<ServerLogic>>
}

impl NullPointerDibServer {
    pub fn new(
        // the server's NodeId
        node_id: NodeId,
        // the channel the server listens on to receive packets from connected drones
        listener_rx: Receiver<Packet>,
        // the HashMap containing every connected drone
        drones_tx: HashMap<NodeId, Sender<Packet>>,
        simulation_controller_tx: Sender<NodeEvent>,
    ) -> Self {
        let (internal_transmitter_to_listener_tx, internal_transmitter_to_listener_rx) = unbounded::<Packet>();
        let (internal_listener_to_transmitter_tx, internal_listener_to_transmitter_rx) = unbounded::<Packet>();
        let (internal_listener_to_server_logic_tx, internal_listener_to_server_logic_rx) = unbounded::<Message>();
        let (internal_server_logic_to_transmitter_tx, internal_server_logic_to_transmitter_rx) = unbounded::<(NodeId, Message)>();
        let (listener_commands_tx, listener_commands_rx) = unbounded::<ListenerCommand>();

        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let (transmitter_command_tx, transmitter_command_rx) = unbounded::<TransmitterCommand>();

        let transmitter = Transmitter::new(
            node_id,
            NodeType::Server,
            internal_listener_to_transmitter_rx,
            internal_transmitter_to_listener_tx,
            internal_server_logic_to_transmitter_rx,
            drones_tx,
            simulation_controller_notifier.clone(),
            transmitter_command_rx
        );
        let transmitter = Arc::new(Mutex::new(transmitter));

        let listener = Listener::new(
            node_id,
            internal_listener_to_transmitter_tx,
            internal_transmitter_to_listener_rx,
            internal_listener_to_server_logic_tx,
            listener_rx,
            listener_commands_rx,
            simulation_controller_notifier.clone(),
        );
        let listener = Arc::new(Mutex::new(listener));

        let server_logic = ServerLogic::new();
        let server_logic = Arc::new(Mutex::new(server_logic));

        Self {
            transmitter,
            listener,
            server_logic,
        }
    }

    pub fn run(&mut self) {
        let listener_clone = self.listener.clone();
        let listener_handle = thread::spawn(move || {
            listener_clone.lock().unwrap().run()
        });

        let transmitter_clone = self.transmitter.clone();
        let transmitter_handle = thread::spawn(move || {
            transmitter_clone.lock().unwrap().run()
        });

        let server_logic_clone = self.server_logic.clone();
        let server_logic_handle = thread::spawn(move || {
            server_logic_clone.lock().unwrap().run();
        });


        // TODO: listen for commands (only shutdown) (either from channel or from CLI)
        // TODO: send shutdown command to listener and transmitter

        listener_handle.join();
        server_logic_handle.join();
        transmitter_handle.join();
    }
}