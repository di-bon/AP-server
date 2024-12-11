// use std::cell::Cell;
// use std::collections::HashMap;
// use std::marker::PhantomData;
// use std::sync::{Arc, LockResult, Mutex};
// use std::time::Duration;
// use tokio::task::JoinHandle;
// use tokio::sync::mpsc;
// use tokio::sync::mpsc::UnboundedReceiver;
// use tokio::time::sleep;
// use wg_2024::packet::Packet;
// use crate::transmitter::Command;
// use crate::transmitter::gateway::Gateway;
//
// #[derive(Debug)]
// pub struct TransmissionHandler<'a> {
//     command_channel: Mutex<UnboundedReceiver<Command>>,
//     // packets: &'static [Packet], // contains the data to transmit
//     packets: Vec<Packet>,
//     window_size: usize,
//     window_start: Arc<Cell<usize>>,
//     timeout: Duration,
//     gateway: Arc<Gateway>,
//     fragment_channels: Mutex<HashMap<u64, mpsc::UnboundedSender<Command>>>,
//     fragment_acks: Vec<bool>,
//     pd: PhantomData<&'a u8>
// }
//
// impl<'a> TransmissionHandler<'a> {
//     // fn new(command_channel: UnboundedReceiver<Command>, packets: &'static[Packet], gateway: Arc<Gateway>) -> Self {
//     fn new(command_channel: UnboundedReceiver<Command>, packets: Vec<Packet>, gateway: Arc<Gateway>) -> Self {
//         let len = packets.len();
//         Self {
//             command_channel: Mutex::new(command_channel),
//             packets,
//             window_size: 1,
//             window_start: Arc::new(Cell::new(0)),
//             timeout: Duration::from_secs(2),
//             gateway,
//             fragment_channels: Mutex::new(HashMap::new()),
//             fragment_acks: Vec::with_capacity(len),
//             pd: PhantomData::default()
//         }
//     }
//
//     fn on_ack_received(&self) {
//         // check the fragment number before moving the window!
//         let previous_start = self.window_start.get();
//         self.window_start.set(previous_start + 1);
//     }
//
//     async fn run(&self) {
//         /*
//         println!("run called");
//         loop {
//             let start = self.window_start.get();
//             let slice = &self.packets.get(start..self.packets.len().min(start + self.window_size));
//             if let Some(ready_to_send) = slice {
//                 for (fragment_index, packet) in ready_to_send.iter().enumerate() {
//                     let fragment_index = fragment_index as u64;
//                     let fragment_command_channel = self.fragment_channels.lock(); // .get(&fragment_index);
//                     // match fragment_command_channel {
//                     //     Some(_) => { },
//                     //     None => {
//                     //         let (tx, rx) = mpsc::unbounded_channel::<Command>();
//                     //         self.fragment_channels.insert(fragment_index, tx);
//                     //         let handle = Self::spawn_task(fragment_index, self.timeout, || {
//                     //             self.gateway.forward(packet.clone());
//                     //         }, rx);
//                     //     }
//                     // };
//                     match fragment_command_channel {
//                         Ok(mut channel) => {
//                             if !channel.contains_key(&fragment_index) {
//                                 let (tx, rx) = mpsc::unbounded_channel::<Command>();
//                                 channel.insert(fragment_index, tx);
//                                 Self::spawn_task(
//                                     fragment_index,
//                                     self.timeout,
//                                     self.gateway.clone(),
//                                     packet.clone(),
//                                     rx
//                                 );
//                             }
//                         }
//                         Err(error) => {
//                             println!("{error:?}")
//                         }
//                     }
//                 }
//                 let channel = self.command_channel.lock();
//                 if let Ok(mut channel) = channel {
//                     tokio::select! {
//                         command = channel.recv() => {
//                             println!("received {command:?}");
//                             if let Some(command) = command {
//                                 match command {
//                                     Command::Confirmed(fragment_index) => {
//                                         self.packets[fragment_index] = true; // check this line!
//                                         self.on_ack_received();
//                                     },
//                                     Command::Resend(fragment_index) => {
//                                         // match self.fragment_channels.get(&fragment_index) {
//                                         //     Some(channel) => { channel.send(Command::Resend(fragment_index)); }
//                                         //     None => {}
//                                         // }
//                                         match self.fragment_channels.lock() {
//                                             Ok(channels) => {
//                                                 if let Some(channel) = channels.get(&fragment_index) {
//                                                     channel.send(Command::Resend(fragment_index));
//                                                 }
//                                             }
//                                             Err(error) => {  }
//                                         }
//                                     }
//                                 }
//                             }
//                         }
//                     }
//                 }
//             }
//             else {
//                 break;
//             }
//         }
//          */
//     }
//
//     async fn spawn_task(
//         id: u64,
//         timeout: Duration,
//         // task_fn: F,
//         gateway: Arc<Gateway>,
//         packet: Packet,
//         mut command_channel: mpsc::UnboundedReceiver<Command>,
//     ) -> JoinHandle<()>
//     // where
//     //     F: Fn() + Send + 'static
//     {
//         tokio::spawn(async move {
//             loop {
//                 // task_fn();
//                 gateway.forward(packet.clone()).expect("Gateway couldn't forward message"); // TODO: if Err, then terminate task?
//                 tokio::select! {
//                     _ = sleep(timeout) => {
//                         println!("Task {} timed out!", id);
//                     }
//                     Some(command) = command_channel.recv() => {
//                         println!("task {id}: received command: {:?}", command);
//                         match command {
//                             Command::Resend(_) => {
//                                 println!("Processing resend command...");
//                                 continue;
//                             }
//                             Command::Confirmed(_) => {
//                                 println!("Command confirmed, exiting loop.");
//                                 break;
//                             }
//                         }
//                     }
//                     else => {
//                         println!("Command channel closed. Exiting loop.");
//                         break;
//                     }
//                 }
//             }
//             println!("task {id} finished");
//         })
//     }
// }
//
// #[cfg(test)]
// mod test {
//     use std::collections::HashMap;
//     use std::sync::Arc;
//     use std::time::Duration;
//     use crossbeam_channel::select;
//     use tokio::sync::mpsc;
//     use tokio::sync::mpsc::unbounded_channel;
//     use wg_2024::network::SourceRoutingHeader;
//     use wg_2024::packet::{Ack, Nack, NackType, Packet, PacketType};
//     use crate::transmitter::Command;
//     use crate::transmitter::gateway::Gateway;
//     use crate::transmitter::transmission_handler::TransmissionHandler;
//
//     #[test]
//     fn create() {
//         let (command_tx, command_rx) = unbounded_channel::<Command>();
//         let packet = Packet {
//             pack_type: PacketType::Ack(Ack { fragment_index: 0 }),
//             routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![0, 1, 2] },
//             session_id: 0,
//         };
//         let packets = vec![packet];
//         let drone_channels = crossbeam_channel::unbounded::<Packet>();
//         let gateway = Gateway::new(0, HashMap::new(), drone_channels.0);
//         let gateway = Arc::new(gateway);
//         let transmission_handler = TransmissionHandler::new(
//             command_rx,
//             packets,
//             gateway
//         );
//         println!("{:?}", transmission_handler);
//         assert_eq!(transmission_handler.packets.len(), 1);
//         assert_eq!(transmission_handler.timeout, Duration::from_secs(2));
//         assert_eq!(transmission_handler.window_start.get(), 0);
//         assert_eq!(transmission_handler.window_size, 1);
//     }
//
//     #[tokio::test]
//     async fn check_transmission() {
//         let (command_tx, command_rx) = mpsc::unbounded_channel::<Command>();
//         let (drone_tx, drone_rx) = crossbeam_channel::unbounded::<Packet>();
//
//         let packet1 = Packet {
//             pack_type: PacketType::Nack(Nack {
//                 fragment_index: 0,
//                 nack_type: NackType::Dropped,
//             }),
//             routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![1, 2, 3] },
//             session_id: 0,
//         };
//         let packet2 = Packet {
//             pack_type: PacketType::Nack(Nack {
//                 fragment_index: 4,
//                 nack_type: NackType::Dropped,
//             }),
//             routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![10, 7, 4] },
//             session_id: 1,
//         };
//         let packet3 = Packet {
//             pack_type: PacketType::Ack(Ack {
//                 fragment_index: 5,
//             }),
//             routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![4, 3, 2] },
//             session_id: 2,
//         };
//         let packets = vec![packet1.clone(), packet2.clone(), packet3.clone()];
//
//         let gateway = Gateway::new(0, HashMap::new(), drone_tx);
//         let gateway = Arc::new(gateway);
//         let mut transmission_handler = TransmissionHandler::new(command_rx, packets, gateway);
//
//         // Run the transmission handler in the current async context
//         tokio::task::block_in_place(|| {
//             futures::executor::block_on(async {
//                 transmission_handler.run().await;
//             });
//         });
//
//         // Poll `drone_rx` synchronously to collect packets
//         let mut received_packets = Vec::new();
//         loop {
//             match drone_rx.try_recv() {
//                 Ok(packet) => received_packets.push(packet),
//                 Err(crossbeam_channel::TryRecvError::Empty) => break,
//                 Err(crossbeam_channel::TryRecvError::Disconnected) => break,
//             }
//         }
//
//         // Assertions
//         assert_eq!(received_packets.len(), 3);
//         assert_eq!(received_packets[0].session_id, 0);
//         assert_eq!(received_packets[1].session_id, 1);
//         assert_eq!(received_packets[2].session_id, 2);
//     }
//
//     // #[tokio::test]
//     // async fn check_transmission() {
//     //     let (command_tx, command_rx) = unbounded_channel::<Command>();
//     //     let packet1 = Packet {
//     //         pack_type: PacketType::Nack(Nack {
//     //             fragment_index: 0,
//     //             nack_type: NackType::Dropped,
//     //         }),
//     //         routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![1, 2, 3] },
//     //         session_id: 0,
//     //     };
//     //     let packet2 = Packet {
//     //         pack_type: PacketType::Nack(Nack {
//     //             fragment_index: 4,
//     //             nack_type: NackType::Dropped,
//     //         }),
//     //         routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![10, 7, 4] },
//     //         session_id: 1,
//     //     };
//     //     let packet3 = Packet {
//     //         pack_type: PacketType::Ack(Ack {
//     //             fragment_index: 5,
//     //         }),
//     //         routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![4, 3, 2] },
//     //         session_id: 2,
//     //     };
//     //     let packets = vec![packet1.clone(), packet2.clone(), packet3.clone()];
//     //     let drone_channels = crossbeam_channel::unbounded::<Packet>();
//     //     let gateway = Gateway::new(0, HashMap::new(), drone_channels.0);
//     //     let gateway = Arc::new(gateway);
//     //     let mut transmission_handler = TransmissionHandler::new(
//     //         command_rx,
//     //         packets,
//     //         gateway
//     //     );
//     //
//     //     transmission_handler.run().await;
//     //     println!("transmission_handler.run() called!");
//     //
//     //     tokio::time::sleep(Duration::from_millis(3000)).await;
//     //     println!("time elapsed");
//     //     assert_eq!(drone_channels.1.len(), 3);
//     //     select! {
//     //         recv(drone_channels.1) -> packet => {
//     //             if let Ok(packet) = packet {
//     //                 let session_id = packet.session_id;
//     //                 assert_eq!(session_id, 0);
//     //             }
//     //         }
//     //         default(Duration::from_millis(100)) => {
//     //             println!("timed out");
//     //             return;
//     //         },
//     //     }
//     //     assert_eq!(drone_channels.1.len(), 2);
//     //     select! {
//     //         recv(drone_channels.1) -> packet => {
//     //             if let Ok(packet) = packet {
//     //                 let session_id = packet.session_id;
//     //                 assert_eq!(session_id, 1);
//     //             }
//     //         }
//     //         default(Duration::from_millis(100)) => {
//     //             println!("timed out");
//     //             return;
//     //         },
//     //     }
//     //     assert_eq!(drone_channels.1.len(), 1);
//     //     select! {
//     //         recv(drone_channels.1) -> packet => {
//     //             if let Ok(packet) = packet {
//     //                 let session_id = packet.session_id;
//     //                 assert_eq!(session_id, 2);
//     //             }
//     //         }
//     //         default(Duration::from_millis(100)) => {
//     //             println!("timed out");
//     //             return;
//     //         }
//     //     }
//     //     assert_eq!(drone_channels.1.len(), 0);
//     // }
// }