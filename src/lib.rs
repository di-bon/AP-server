use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use crossbeam_channel::{unbounded, Receiver, Sender};
use messages::node_event::NodeEvent;
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};
use ap_transmitter::{Transmitter, Command as TransmitterCommand};
use ap_sc_notifier::SimulationControllerNotifier;
use ap_listener::{Listener, Command as ListenerCommand};
use crate::logic::{Command as ServerCommand, CommunicationServer, ContentServer, Getter, Server};

mod logic;

pub enum Command {
    Quit,
    AddNeighbor(NodeId, Sender<Packet>),
    RemoveNeighbor(NodeId),
}

pub struct DibServer {
    node_id: NodeId,
    listener: Arc<Mutex<Listener>>,
    listener_command_tx: Sender<ListenerCommand>,
    logic: Arc<Mutex<dyn Server>>,
    logic_command_tx: Sender<ServerCommand>,
    transmitter: Arc<Mutex<Transmitter>>,
    transmitter_command_tx: Sender<TransmitterCommand>,
    command_rx: Receiver<Command>,
}

impl DibServer {
    /// Return a new instance of `DibServer` implementing a `ContentServer`
    /// # Panics
    /// Panics if `Transmitter`, `Listener` and `ServerLogic` do not share the same `node_id`
    #[must_use]
    pub fn new_content_server(
        node_id: NodeId,
        listener_rx: Receiver<Packet>,
        drones_tx: HashMap<NodeId, Sender<Packet>>,
        simulation_controller_tx: Sender<NodeEvent>,
        resource_path: String,
    ) -> (Self, Sender<Command>) {
        let (listener_to_transmitter_tx, listener_to_transmitter_rx) = unbounded();
        let (listener_to_server_logic_tx, listener_to_server_logic_rx) = unbounded();
        let (logic_to_transmitter_tx, logic_to_transmitter_rx) = unbounded();
        let (listener_command_tx, listener_command_rx) = unbounded();

        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let (transmitter_command_tx, transmitter_command_rx) = unbounded();

        let transmitter = Transmitter::new(
            node_id,
            NodeType::Server,
            listener_to_transmitter_rx,
            logic_to_transmitter_rx,
            drones_tx,
            simulation_controller_notifier.clone(),
            transmitter_command_rx
        );

        let listener = Listener::new(
            node_id,
            listener_to_transmitter_tx,
            listener_to_server_logic_tx,
            listener_rx,
            listener_command_rx,
            simulation_controller_notifier.clone(),
        );

        let (logic_command_tx, logic_command_rx) = unbounded();

        let logic = ContentServer::new(
            node_id,
            logic_to_transmitter_tx,
            listener_to_server_logic_rx,
            logic_command_rx,
            resource_path,
        );

        assert_eq!(transmitter.get_node_id(), listener.get_node_id());
        assert_eq!(transmitter.get_node_id(), logic.get_node_id());

        let transmitter = Arc::new(Mutex::new(transmitter));
        let listener = Arc::new(Mutex::new(listener));
        let logic = Arc::new(Mutex::new(logic));

        let (command_tx, command_rx) = unbounded();

        let result = Self {
            node_id,
            listener,
            listener_command_tx,
            logic,
            logic_command_tx,
            transmitter,
            transmitter_command_tx,
            command_rx,
        };

        (result, command_tx)
    }

    /// Return a new instance of `DibServer` implementing a `CommunicationServer`
    /// # Panics
    /// Panics if `Transmitter`, `Listener` and `ServerLogic` do not share the same `node_id`
    #[must_use]
    pub fn new_communication_server(
        node_id: NodeId,
        listener_rx: Receiver<Packet>,
        drones_tx: HashMap<NodeId, Sender<Packet>>,
        simulation_controller_tx: Sender<NodeEvent>,
    ) -> (Self, Sender<Command>) {
        let (listener_to_transmitter_tx, listener_to_transmitter_rx) = unbounded();
        let (listener_to_server_logic_tx, listener_to_server_logic_rx) = unbounded();
        let (logic_to_transmitter_tx, logic_to_transmitter_rx) = unbounded();
        let (listener_command_tx, listener_command_rx) = unbounded();

        let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
        let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

        let (transmitter_command_tx, transmitter_command_rx) = unbounded();

        let transmitter = Transmitter::new(
            node_id,
            NodeType::Server,
            listener_to_transmitter_rx,
            logic_to_transmitter_rx,
            drones_tx,
            simulation_controller_notifier.clone(),
            transmitter_command_rx
        );

        let listener = Listener::new(
            node_id,
            listener_to_transmitter_tx,
            listener_to_server_logic_tx,
            listener_rx,
            listener_command_rx,
            simulation_controller_notifier.clone(),
        );

        let (logic_command_tx, logic_command_rx) = unbounded();

        let logic = CommunicationServer::new(
            node_id,
            logic_to_transmitter_tx,
            listener_to_server_logic_rx,
            logic_command_rx,
        );

        assert_eq!(transmitter.get_node_id(), listener.get_node_id());
        assert_eq!(transmitter.get_node_id(), logic.get_node_id());

        let transmitter = Arc::new(Mutex::new(transmitter));
        let listener = Arc::new(Mutex::new(listener));
        let logic = Arc::new(Mutex::new(logic));

        let (command_tx, command_rx) = unbounded();

        let result = Self {
            node_id,
            listener,
            listener_command_tx,
            logic,
            logic_command_tx,
            transmitter,
            transmitter_command_tx,
            command_rx,
        };

        (result, command_tx)
    }
}

pub trait DibGetter {
    fn get_node_id(&self) -> NodeId;
    fn get_listener(&self) -> Arc<Mutex<Listener>>;
    fn get_listener_tx(&self) -> &Sender<ListenerCommand>;
    fn get_logic(&self) -> Arc<Mutex<dyn Server>>;
    fn get_logic_tx(&self) -> &Sender<ServerCommand>;
    fn get_transmitter(&self) -> Arc<Mutex<Transmitter>>;
    fn get_transmitter_tx(&self) -> &Sender<TransmitterCommand>;
    fn get_command_rx(&self) -> &Receiver<Command>;
}

impl DibGetter for DibServer {
    fn get_node_id(&self) -> NodeId {
        self.node_id
    }

    fn get_listener(&self) -> Arc<Mutex<Listener>> {
        self.listener.clone()
    }

    fn get_listener_tx(&self) -> &Sender<ListenerCommand> {
        &self.listener_command_tx
    }

    fn get_logic(&self) -> Arc<Mutex<dyn Server>> {
        self.logic.clone()
    }

    fn get_logic_tx(&self) -> &Sender<ServerCommand> {
        &self.logic_command_tx
    }

    fn get_transmitter(&self) -> Arc<Mutex<Transmitter>> {
        self.transmitter.clone()
    }

    fn get_transmitter_tx(&self) -> &Sender<TransmitterCommand> {
        &self.transmitter_command_tx
    }

    fn get_command_rx(&self) -> &Receiver<Command> {
        &self.command_rx
    }
}

pub trait DibServerTrait: DibGetter {
    /// Starts the server
    /// # Panics
    /// - Panics if the transmitter thread cannot acquire the lock on the transmitter
    /// - Panics if the listener thread cannot acquire the lock on the listener
    /// - Panics if the server logic thread cannot acquire the lock on the server logic
    fn run(&mut self) {
        let listener = self.get_listener().clone();

        let listener_handle = thread::Builder::new()
            .name(format!("server_{}_listener", self.get_node_id()))
            .spawn(move || {
                let mut listener = match listener.lock() {
                    Ok(listener) => listener,
                    Err(err) => {
                        log::error!("Error while starting listener: {err:?}");
                        panic!("Error while starting listener: {err:?}");
                    }
                };
                listener.run();
            }).unwrap();

        let transmitter = self.get_transmitter().clone();
        let transmitter_handle = thread::Builder::new()
            .name(format!("server_{}_transmitter", self.get_node_id()))
            .spawn(move || {
                let mut transmitter = match transmitter.lock() {
                    Ok(transmitter) => transmitter,
                    Err(err) => {
                        log::error!("Error while starting transmitter: {err:?}");
                        panic!("Error while starting transmitter: {err:?}");
                    }
                };
                transmitter.run();
            }).unwrap();

        let logic = self.get_logic().clone();
        let server_logic_handle = thread::Builder::new()
            .name(format!("server_{}_logic", self.get_node_id()))
            .spawn(move || {
                let mut logic = match logic.lock() {
                    Ok(logic) => logic,
                    Err(err) => {
                        log::error!("Error while starting logic: {err:?}");
                        panic!("Error while starting logic: {err:?}");
                    }
                };
                logic.run();
            }).unwrap();

        'command_loop: loop {
            let command = self.get_command_rx().recv();
            match command {
                Ok(command) => {
                    match command {
                        Command::Quit => {
                            let command = ListenerCommand::Quit;
                            self.get_listener_tx().send(command).expect("Cannot communicate with listener thread");

                            let command = ServerCommand::Quit;
                            self.get_logic_tx().send(command).expect("Cannot communicate with logic thread");

                            let command = TransmitterCommand::Quit;
                            self.get_transmitter_tx().send(command).expect("Cannot communicate with transmitter thread");

                            break 'command_loop
                        }
                        Command::AddNeighbor(node_id, channel) => {
                            let command = TransmitterCommand::AddNeighbor(node_id, channel);
                            self.get_transmitter_tx().send(command).expect("Cannot communicate with transmitter thread");
                        }
                        Command::RemoveNeighbor(node_id) => {
                            let command = TransmitterCommand::RemoveNeighbor(node_id);
                            self.get_transmitter_tx().send(command).expect("Cannot communicate with transmitter thread");
                        }
                    }
                }
                Err(error) => {
                    let error = format!("Error while receiving Command's. Error: {error:?}");
                    log::error!("{error}");
                    panic!("{error}");
                }
            }
        }

        let _ = listener_handle.join();
        let _ = server_logic_handle.join();
        let _ = transmitter_handle.join();
    }
}

impl DibServerTrait for DibServer {}