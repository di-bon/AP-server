use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use crossbeam_channel::{select, Receiver, Sender};
use tokio::task::JoinHandle;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Ack, Nack, Packet, PacketType};
use crate::transmitter::network_controller::NetworkController;
use tokio::time::{sleep, Duration};
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
    // listener -> transmitter
    listener_channel: Receiver<Packet>, // receives ACKs, NACKs, FloodRequest and FloodResponse
    // server logic -> transmitter
    server_logic_channel: Receiver<Packet>, // HL message!
    network_controller: NetworkController,
    // transmitter -> transmission handlers
    transmission_handlers: HashMap<u64, (Sender<Command>, TransmissionHandler)>,
    // server -> drones - just for gateway initialisation
    connected_drones: HashMap<NodeId, Receiver<Packet>>,
    // simulation_controller_channel: Receiver<Packet> // TODO: this channel needs to be updated to receive commands - maybe it is just useless?
    gateway: Rc<Gateway>,
}

impl Transmitter {
    pub fn new(
        listener_channel: Receiver<Packet>,
        server_logic_channel: Receiver<Packet>,
        network_controller: NetworkController,
        transmission_handlers: HashMap<u64, (Sender<Command>, TransmissionHandler)>,
        connected_drones: HashMap<NodeId, Receiver<Packet>>,
        gateway: Rc<Gateway>,
    ) -> Self {
        Self {
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
                        match packet.pack_type {
                            PacketType::Ack(ack) => {
                                // if an ack is received, tell the transmission_handler to handle
                                // it, which means updating the transmission window and/or terminating
                            },
                            PacketType::Nack(nack) => {
                                // if a nack is received, tell the transmission_handler to send
                                // the required fragment again
                            },
                            PacketType::FloodRequest(flood_request) => {
                                // if a flood request is received, send a flood_response
                            },
                            PacketType::FloodResponse(flood_response) => {
                                // if a flood response is received, update the network controller
                            },
                            PacketType::MsgFragment(ref fragment) => {
                                // if a fragment is received, send the ack for it back
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
                                        continue;
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
}