use std::thread;
use ap_server::test_utils::ListenerCommand;
use crate::common::{create_listener, create_simulation_controller_notifier};

mod common;

#[test]
fn check_listener_commands() -> std::thread::Result<()> {
    let node_id = 0;

    let (simulation_controller_notifier, simulation_controller_rx) = create_simulation_controller_notifier();
    let (mut listener,
        listener_to_transmitter_rx,
        transmitter_to_listener_tx,
        listener_to_logic_rx,
        drones_to_listener_tx,
        listener_command_tx) = create_listener(node_id, simulation_controller_notifier.clone());

    let handle = thread::spawn(move || {
        listener.run();
    });

    let _ = listener_command_tx.send(ListenerCommand::Quit);

    handle.join()
}