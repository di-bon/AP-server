use std::collections::HashMap;
use std::ptr::read;
use std::thread;
use assembler::Assembler;
use assembler::naive_assembler::NaiveAssembler;
use crossbeam_channel::{unbounded, Receiver, Sender};
use messages::{ChatRequest, ChatResponse, Message, MessageType, MessageUtilities, RequestType, ResponseType, TextRequest};
use messages::TextResponse::TextList;
use ntest::timeout;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Packet, PacketType};
use ap_server::{Command, DibGetter, DibServer, DibServerTrait};
use crate::common::{process_initial_flood_requests, send_message_and_receive_acks};

mod common;

#[test]
#[timeout(2000)]
fn check_text_requests() -> std::thread::Result<()> {
    let server_node_id = 0;

    let mut connected_drones = HashMap::new();
    let mut drones = HashMap::new();
    let (tx, rx) = unbounded();

    connected_drones.insert(1, tx);
    drones.insert(1, rx);

    let (tx, rx) = unbounded();
    connected_drones.insert(9, tx);
    drones.insert(9, rx);

    let (server_public_tx, server_public_rx) = unbounded();
    let (server_to_sc_tx, server_to_sc_rx) = unbounded();

    let (mut server, command_tx) = DibServer::new_content_server(
        server_node_id,
        server_public_rx,
        connected_drones,
        server_to_sc_tx,
        "./res".to_string()
    );

    let server_handler = thread::Builder::new()
        .name(format!("content_server_{}", server.get_node_id()))
        .spawn(move || {
            server.run()
        })
        .unwrap();

    process_initial_flood_requests(&mut drones, &server_public_tx);

    let source = 9;
    let destination = server_node_id;
    let session_id = 4;

    let server_to_drone_source_rx = drones.get(&source).unwrap();

    let text_list_request = Message {
        source,
        destination,
        session_id,
        content: MessageType::Request(RequestType::TextRequest(TextRequest::TextList)),
    };

    send_message_and_receive_acks(server_to_drone_source_rx, &server_public_tx, source, destination, session_id, text_list_request);

    let expected_response = Message {
        source: destination,
        destination: source,
        session_id,
        content: MessageType::Response(ResponseType::TextResponse(TextList(vec!["rust.txt".to_string(), "the quacking duck.txt".to_string()]))),
    };

    let expected_fragments = NaiveAssembler::disassemble(&expected_response.stringify().into_bytes());
    let mut received_fragments = Vec::new();
    for _ in 0..expected_fragments.len() {
        let received = server_to_drone_source_rx.recv().unwrap();
        match received.pack_type {
            PacketType::MsgFragment(fragment) => received_fragments.push(fragment),
            _ => panic!("Received unexpected packet {received:?}"),
        }
    }

    received_fragments.sort_by_key(|fragment| fragment.fragment_index);
    let received_message = NaiveAssembler::reassemble(&received_fragments);
    let received_message = String::from_utf8(received_message).unwrap();
    let received_message: Message = MessageUtilities::from_string(received_message).unwrap();

    assert_eq!(received_message, expected_response);

    let shutdown = Command::Quit;
    let _ = command_tx.send(shutdown);

    server_handler.join()
}