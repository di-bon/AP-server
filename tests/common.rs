use std::collections::HashMap;
use std::sync::Arc;
use crossbeam_channel::{unbounded, Receiver, Sender};
use messages::Message;
use messages::node_event::NodeEvent;
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};
use ap_server::test_utils::{Listener, ListenerCommand, SimulationControllerNotifier, Transmitter, TransmitterInternalCommand, TransmitterUserCommand};

pub fn create_transmitter(
    node_id: NodeId,
    node_type: NodeType,
    connected_drones: HashMap<NodeId, Sender<Packet>>,
    simulation_controller_notifier: Arc<SimulationControllerNotifier>,
) -> (Transmitter, Sender<TransmitterInternalCommand>, Sender<Message>, Sender<TransmitterUserCommand>) {

    let (listener_to_transmitter_tx, listener_to_transmitter_rx) = unbounded::<TransmitterInternalCommand>();
    let (logic_to_transmitter_tx, logic_to_transmitter_rx) = unbounded();
    let (transmitter_command_tx, transmitter_command_rx) = unbounded();

    let transmitter = Transmitter::new(
        node_id,
        node_type,
        listener_to_transmitter_rx,
        logic_to_transmitter_rx,
        connected_drones,
        simulation_controller_notifier,
        transmitter_command_rx,
    );

    (transmitter, listener_to_transmitter_tx, logic_to_transmitter_tx, transmitter_command_tx)
}

pub fn create_simulation_controller_notifier() -> (Arc<SimulationControllerNotifier>, Receiver<NodeEvent>) {
    let (simulation_controller_tx, simulation_controller_rx) = unbounded();

    let simulation_controller_notifier = SimulationControllerNotifier::new(simulation_controller_tx);
    let simulation_controller_notifier = Arc::new(simulation_controller_notifier);

    (simulation_controller_notifier, simulation_controller_rx)
}

pub fn create_listener(node_id: NodeId, simulation_controller_notifier: Arc<SimulationControllerNotifier>) -> (Listener, Receiver<TransmitterInternalCommand>, Receiver<Message>, Sender<Packet>, Sender<ListenerCommand>) {
    let (listener_to_transmitter_tx, listener_to_transmitter_rx) = unbounded();
    let (listener_to_logic_tx, listener_to_logic_rx) = unbounded();
    let (drones_to_listener_tx, drones_to_listener_rx) = unbounded();
    let (listener_command_tx, listener_command_rx) = unbounded();

    let listener = Listener::new(
        node_id,
        listener_to_transmitter_tx,
        listener_to_logic_tx,
        drones_to_listener_rx,
        listener_command_rx,
        simulation_controller_notifier,
    );

    (listener, listener_to_transmitter_rx, listener_to_logic_rx, drones_to_listener_tx, listener_command_tx)
}