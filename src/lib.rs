// TODO: remove this when project is finished
// #![allow(dead_code)]
// #![allow(unused_variables)]
// #![allow(unused_must_use)]
// #![allow(clippy::missing_panics_doc)]

use std::collections::HashMap;
use std::sync::{Arc, LockResult, Mutex};
use std::thread;
use crossbeam_channel::{unbounded, Receiver, Sender};
use messages::Message;
use messages::node_event::NodeEvent;
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};
use crate::listener::{Listener, ListenerCommand};
use crate::server_logic::ServerLogic;
use crate::simulation_controller_notifier::SimulationControllerNotifier;
use crate::transmitter::{Transmitter, TransmitterInternalCommand, TransmitterUserCommand};

mod transmitter;
mod listener;
mod server_logic;
mod simulation_controller_notifier;

#[cfg(any(test, feature = "integration-testing"))]
pub mod test_utils;

pub struct NullPointerDibServer {
    transmitter: Arc<Mutex<Transmitter>>,
    listener: Arc<Mutex<Listener>>,
    server_logic: Arc<Mutex<ServerLogic>>
}

impl NullPointerDibServer {

    #[must_use]
    /// Return a new instance of `NullPointerDibServer`
    /// # Panics
    /// Panics if `Transmitter`, `Listener` and `ServerLogic` do not share the same `node_id`
    pub fn new(
        // the server's NodeId
        node_id: NodeId,
        // the channel the server listens on to receive packets from connected drones
        listener_rx: Receiver<Packet>,
        // the HashMap containing every connected drone
        drones_tx: HashMap<NodeId, Sender<Packet>>,
        simulation_controller_tx: Sender<NodeEvent>,
    ) -> Self {
        let (internal_listener_to_transmitter_tx, internal_listener_to_transmitter_rx) = unbounded::<TransmitterInternalCommand>();
        let (internal_listener_to_server_logic_tx, internal_listener_to_server_logic_rx) = unbounded::<Message>();
        let (internal_server_logic_to_transmitter_tx, internal_server_logic_to_transmitter_rx) = unbounded::<(NodeId, Message)>();
        let (listener_command_tx, listener_command_rx) = unbounded::<ListenerCommand>();

        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let (transmitter_command_tx, transmitter_command_rx) = unbounded::<TransmitterUserCommand>();

        let transmitter = Transmitter::new(
            node_id,
            NodeType::Server,
            internal_listener_to_transmitter_rx,
            internal_server_logic_to_transmitter_rx,
            drones_tx,
            simulation_controller_notifier.clone(),
            transmitter_command_rx
        );

        let listener = Listener::new(
            node_id,
            internal_listener_to_transmitter_tx,
            internal_listener_to_server_logic_tx,
            listener_rx,
            listener_command_rx,
            simulation_controller_notifier.clone(),
        );

        let server_logic = ServerLogic::new(internal_server_logic_to_transmitter_tx, internal_listener_to_server_logic_rx);

        assert_eq!(transmitter.get_node_id(), listener.get_node_id());
        assert_eq!(transmitter.get_node_id(), server_logic.get_node_id());

        let transmitter = Arc::new(Mutex::new(transmitter));
        let listener = Arc::new(Mutex::new(listener));
        let server_logic = Arc::new(Mutex::new(server_logic));

        Self {
            transmitter,
            listener,
            server_logic,
        }
    }

    /// Starts the server
    /// # Panics
    /// - Panics if the transmitter thread cannot acquire the lock on the transmitter
    /// - Panics if the listener thread cannot acquire the lock on the listener
    /// - Panics if the server logic thread cannot acquire the lock on the server logic
    pub fn run(&mut self) {
        let listener = self.listener.clone();
        let listener_handle = thread::spawn(move || {
            let mut listener = match listener.lock() {
                Ok(listener) => listener,
                Err(err) => {
                    log::error!("Error while starting listener: {err:?}");
                    panic!("Error while starting listener: {err:?}");
                }
            };
            listener.run();
        });

        let transmitter = self.transmitter.clone();
        let transmitter_handle = thread::spawn(move || {
            let mut transmitter = match transmitter.lock() {
                Ok(transmitter) => transmitter,
                Err(err) => {
                    log::error!("Error while starting transmitter: {err:?}");
                    panic!("Error while starting transmitter: {err:?}");
                }
            };
            transmitter.run();
        });

        let server_logic = self.server_logic.clone();
        let server_logic_handle = thread::spawn(move || {
            let mut server_logic = match server_logic.lock() {
                Ok(server_logic) => server_logic,
                Err(err) => {
                    log::error!("Error while starting server_logic: {err:?}");
                    panic!("Error while starting server_logic: {err:?}");
                }
            };
            server_logic.run();
        });

        // TODO: listen for commands (only shutdown) (either from channel or from CLI)
        // TODO: send shutdown command to listener and transmitter

        let _ = listener_handle.join();
        let _ = server_logic_handle.join();
        let _ = transmitter_handle.join();
    }
}