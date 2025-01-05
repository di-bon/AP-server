use std::thread;
use ntest::timeout;
use wg_2024::network::SourceRoutingHeader;
use wg_2024::packet::{Ack, Fragment, Nack, NackType, Packet, PacketType};
use ap_server::test_utils::{ListenerCommand, TransmitterInternalCommand};
use crate::common::{create_listener, create_simulation_controller_notifier};

mod common;

#[test]
#[timeout(2000)]
fn check_quit_command() -> thread::Result<()> {
    let node_id = 0;

    let (simulation_controller_notifier, simulation_controller_rx) = create_simulation_controller_notifier();
    let (mut listener,
        listener_to_transmitter_rx,
        listener_to_logic_rx,
        drones_to_listener_tx,
        listener_command_tx) = create_listener(node_id, simulation_controller_notifier.clone());

    let handle = thread::spawn(move || {
        listener.run();
    });

    let _ = listener_command_tx.send(ListenerCommand::Quit);

    handle.join()
}

/*
#[test]
#[timeout(2000)]
fn check_internal_transmitter_to_listener_channel() {
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

    let nack = Nack {
        fragment_index: 0,
        nack_type: NackType::ErrorInRouting(10),
    };
    let nack = Packet {
        routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![node_id] },
        session_id: 0,
        pack_type: PacketType::Nack(nack),
    };

    transmitter_to_listener_tx.send(nack).expect("Transmitter cannot communicate with listener");

    let received = listener_to_transmitter_rx.recv().unwrap();

    assert_eq!(received, nack);
}
 */

#[test]
#[timeout(2000)]
fn check_unexpected_recipient() {
    let node_id = 0;

    let (simulation_controller_notifier, simulation_controller_rx) = create_simulation_controller_notifier();
    let (mut listener,
        listener_to_transmitter_rx,
        listener_to_logic_rx,
        drones_to_listener_tx,
        listener_command_tx) = create_listener(node_id, simulation_controller_notifier.clone());

    let handle = thread::spawn(move || {
        listener.run();
    });

    let fragment = Fragment {
        fragment_index: 0,
        total_n_fragments: 1,
        length: 128,
        data: [0; 128],
    };
    let packet = Packet {
        routing_header: SourceRoutingHeader { hop_index: 2, hops: vec![100, node_id, 1] },
        session_id: 0,
        pack_type: PacketType::MsgFragment(fragment),
    };

    drones_to_listener_tx.send(packet.clone()).expect("Transmitter cannot communicate with listener");

    let received = listener_to_transmitter_rx.recv().unwrap();

    let expected = TransmitterInternalCommand::ProcessNack {
        session_id: 0,
        nack: Nack {
            fragment_index: 0,
            nack_type: NackType::UnexpectedRecipient(node_id),
        },
        source: 100,
    };

    assert_eq!(received, expected);

    let fragment = Fragment {
        fragment_index: 0,
        total_n_fragments: 1,
        length: 128,
        data: [0; 128],
    };

    let packet = Packet {
        routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![100, node_id] },
        session_id: 0,
        pack_type: PacketType::MsgFragment(fragment),
    };

    drones_to_listener_tx.send(packet.clone()).expect("Transmitter cannot communicate with listener");

    let received = listener_to_transmitter_rx.recv().unwrap();

    let expected = TransmitterInternalCommand::ProcessNack {
        session_id: 0,
        nack: Nack {
            fragment_index: 0,
            nack_type: NackType::UnexpectedRecipient(node_id),
        },
        source: 100,
    };

    assert_eq!(received, expected);
}