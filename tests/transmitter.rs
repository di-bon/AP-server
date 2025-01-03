// use std::collections::HashMap;
// use std::sync::{Arc, Mutex};
// use std::thread;
// use crossbeam_channel::unbounded;
// use ntest::timeout;
// use wg_2024::network::SourceRoutingHeader;
// use wg_2024::packet::{FloodResponse, NodeType, Packet, PacketType};
// use AP_server::Transmitter;
//
// #[test]
// #[timeout(2000)]
// fn process_flood_response() {
//     let node_id = 0;
//     let node_type = NodeType::Server;
//
//     let (internal_transmitter_to_listener_tx, internal_transmitter_to_listener_rx) = unbounded::<Packet>();
//     let (internal_listener_to_transmitter_tx, internal_listener_to_transmitter_rx) = unbounded::<Packet>();
//     let (internal_listener_to_server_logic_tx, internal_listener_to_server_logic_rx) = unbounded::<Packet>();
//     let (internal_server_logic_to_transmitter_tx, internal_server_logic_to_transmitter_rx) = unbounded::<Packet>();
//     let (listener_commands_tx, listener_commands_rx) = unbounded::<ListenerCommand>();
//
//     let mut connected_drones = HashMap::new();
//     let (drone_1_tx, drone_1_rx) = unbounded::<Packet>();
//     connected_drones.insert(1, drone_1_tx);
//
//     let transmitter = Transmitter::new(
//         node_id,
//         node_type,
//         internal_listener_to_transmitter_rx,
//         internal_transmitter_to_listener_tx,
//         internal_server_logic_to_transmitter_rx,
//         connected_drones
//     );
//
//     thread::spawn(move || {
//        transmitter.run()
//     });
//
//     let flood_response = FloodResponse {
//         flood_id: 0,
//         path_trace: vec![
//             (node_id, node_type),
//             (1, NodeType::Drone),
//             (2, NodeType::Client),
//         ],
//     };
//     let flood_response = Packet {
//         routing_header: SourceRoutingHeader {
//             hop_index: 0,
//             hops: vec![],
//         },
//         session_id: 0,
//         pack_type: PacketType::FloodResponse(flood_response),
//     };
//
//     internal_listener_to_transmitter_tx.send(flood_response).expect("Cannot send packet to transmitter");
//
//
// }