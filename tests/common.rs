use std::collections::HashMap;
use assembler::Assembler;
use assembler::naive_assembler::NaiveAssembler;
use crossbeam_channel::{Receiver, Sender};
use messages::{Message, MessageUtilities};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Ack, FloodResponse, NodeType, Packet, PacketType};

pub fn process_initial_flood_requests(drones: &mut HashMap<NodeId, Receiver<Packet>>, server_public_tx: &Sender<Packet>) {
    for (drone_id, channel) in drones {
        let mut received = channel.recv().unwrap();
        match &mut received.pack_type {
            PacketType::FloodRequest(ref mut request) => {
                request.path_trace.push((*drone_id, NodeType::Drone));
                let response = Packet {
                    routing_header: Default::default(),
                    session_id: received.session_id,
                    pack_type: PacketType::FloodResponse(
                        FloodResponse {
                            flood_id: request.flood_id,
                            path_trace: request.path_trace.clone(),
                        }
                    ),
                };
                let _ = server_public_tx.send(response);
            },
            _ => panic!("Received unexpected packet {received:?}"),
        }
    }
}

pub fn send_message_and_receive_acks(server_to_source_rx: &Receiver<Packet>, drone_to_server_tx: &Sender<Packet>, source: NodeId, destination: NodeId, session_id: u64, message: Message) {
    let packets = NaiveAssembler::disassemble(&message.stringify().into_bytes());
    let packets = packets
        .into_iter()
        .map(|fragment| {
            let fragment_index = fragment.fragment_index;
            let packet = Packet {
                routing_header: SourceRoutingHeader {
                    hop_index: 1,
                    hops: vec![source, destination],
                },
                session_id,
                pack_type: PacketType::MsgFragment(fragment),
            };
            (fragment_index, packet)
        });

    for (fragment_index, packet) in packets {
        let _ = drone_to_server_tx.send(packet);
        let response = server_to_source_rx.recv().unwrap();
        let expected = Packet {
            routing_header: SourceRoutingHeader {
                hop_index: 1,
                hops: vec![destination, source],
            },
            session_id,
            pack_type: PacketType::Ack(
                Ack {
                    fragment_index,
                }
            ),
        };
        assert_eq!(response, expected);
    }
}