use std::collections::HashMap;
use std::rc::Rc;
use crossbeam_channel::{select, Receiver, SendError, Sender};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Ack, FloodResponse, Nack, NackType, NodeType, Packet, PacketType};
use crate::transmitter::network_controller::NetworkController;
use crate::transmitter::gateway::Gateway;
use crate::transmitter::transmission_handler::TransmissionHandler;

mod network_controller;
mod transmission_handler_async;
mod gateway;
mod transmission_handler;

#[derive(Debug)]
enum Command {
    Resend(u64),
    Confirmed(u64),
}

struct Transmitter {
    node_id: NodeId,
    // listener -> transmitter
    listener_channel: Receiver<Packet>, // receives ACKs, NACKs, FloodRequest and FloodResponse
    // server logic -> transmitter
    server_logic_channel: Receiver<Packet>, // HL message!
    network_controller: NetworkController,
    // transmitter -> transmission handlers
    transmission_handlers: HashMap<u64, Sender<Command>>,
    // server -> drones - just for gateway initialisation
    connected_drones: HashMap<NodeId, Receiver<Packet>>,
    // simulation_controller_channel: Receiver<Packet> // TODO: this channel needs to be updated to receive commands - maybe it is just useless?
    gateway: Rc<Gateway>,
}

impl Transmitter {
    pub fn new(
        node_id: NodeId,
        listener_channel: Receiver<Packet>,
        server_logic_channel: Receiver<Packet>,
        network_controller: NetworkController,
        transmission_handlers: HashMap<u64, Sender<Command>>,
        connected_drones: HashMap<NodeId, Receiver<Packet>>,
        gateway: Rc<Gateway>,
    ) -> Self {
        Self {
            node_id,
            listener_channel,
            server_logic_channel,
            network_controller,
            transmission_handlers,
            connected_drones,
            gateway,
        }
    }

    pub fn run(&self) {
        loop {
            select! {
                recv(self.listener_channel) -> packet => {
                    if let Ok(packet) = packet {
                        self.process_packet(packet);
                    } else {
                        // TODO: panic?
                        panic!("Error while receiving from listener_channel");
                    }
                },
                recv(self.server_logic_channel) -> packet => {
                    // to send a server logic message, create a new session_id, pass the high level
                    // message to a new transmission_handler, store a reference to that transmission
                    // handler in a hashmap containing the session_id as the key
                    // The transmission handler will handler the fragmentation by using the assembler
                    // The reference to the transmission handler will be removed when the
                    // transmission_handler will have received every ACK message
                },
                // recv(self.simulation_controller_channel) -> command => {
                //
                // }
            }
        }
    }

    fn process_packet(&self, packet: Packet) {
        match packet.pack_type {
            PacketType::Ack(ref ack) => {
                // if an ack is received, tell the transmission_handler to handle
                // it, which means updating the transmission window and/or terminating
                let session_id = packet.session_id;
                let channel = match self.transmission_handlers.get(&session_id) {
                    Some(channel) => {
                        channel
                    },
                    None => {
                        // TODO: review code below
                        // if there is no entry for the packet session_id, the ack
                        // can just be ignored? Or should it send a Nack::UnexpectedRecipient?
                        let source = match packet.routing_header.source() {
                            Some(source) => source,
                            None => panic!("Received a packet with no sender")
                        };
                        let nack_path = match self.network_controller.get_path(source) {
                            Some(path) => path,
                            None => {
                                self.gateway.send_to_listener(packet.clone());
                                return;
                            }
                        };
                        let nack = Packet::new_nack(
                            SourceRoutingHeader {
                                hop_index: 1,
                                hops: nack_path,
                            },
                            session_id,
                            Nack {
                                fragment_index: ack.fragment_index,
                                nack_type: NackType::UnexpectedRecipient(self.node_id),
                            }
                        );
                        self.gateway.forward(nack);
                        return;
                    }
                };
                match channel.send(Command::Confirmed(ack.fragment_index)) {
                    Ok(()) => {},
                    Err(err) => {
                        panic!("Transmitter cannot communicate to transmission handler associated with session_id {session_id}");
                    }
                }
            },
            PacketType::Nack(nack) => {
                // if a nack is received, tell the transmission_handler to send
                // the required fragment again
                let session_id = packet.session_id;
                let fragment_index = nack.fragment_index;
                let handler_channel = match self.transmission_handlers.get(&session_id) {
                    Some(channel) => {
                        channel
                    },
                    None => {
                        // TODO: what to do here? continue?
                        panic!("no handler found for the required session_id");
                    },
                };
                match handler_channel.send(Command::Resend(fragment_index)) {
                    Ok(()) => {},
                    Err(err) => {
                        // TODO: ignore this?
                        panic!("Cannot communicate with handler");
                    }
                }
            },
            PacketType::FloodRequest(flood_request) => {
                // if a flood request is received, send a flood_response
                let session_id = packet.session_id;
                let mut path_trace = flood_request.path_trace;
                path_trace.push((self.node_id, NodeType::Server));
                path_trace.reverse();
                let flood_response = FloodResponse {
                    flood_id: flood_request.flood_id,
                    path_trace,
                };
                self.gateway.send_flood_response(flood_response, session_id);
            },
            PacketType::FloodResponse(flood_response) => {
                // if a flood response is received, update the network controller
                self.network_controller.update_from_flood_response(flood_response);
            },
            PacketType::MsgFragment(ref fragment) => {
                // if a fragment is received, send back the ack for it
                let source = match packet.routing_header.source() {
                    Some(source) => {
                        source
                    },
                    None => {
                        // TODO: maybe ignore this message?
                        panic!("Received a packet with no sender")
                    }
                };
                let path = self.network_controller.get_path(source);
                let path = match path {
                    Some(path) => {
                        path
                    },
                    None => {
                        // if this happens, it means that there is no existing
                        // route at the moment. This should never happen as
                        // said in the protocol, but this case can still arise if
                        // the flood has not yet been completed.
                        // TODO: handle this case - maybe just send it back to
                        // listener and continue?
                        self.gateway.send_to_listener(packet);
                        return;
                    }
                };
                let packet = Packet {
                    routing_header: SourceRoutingHeader {
                        hop_index: 1,
                        hops: path,
                    },
                    session_id: packet.session_id,
                    pack_type: PacketType::Ack(
                        Ack {
                            fragment_index: fragment.fragment_index,
                        }
                    ),
                };
                self.gateway.forward(packet);
            }
        }
    }
}