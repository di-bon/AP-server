use std::cell::Cell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc};
use std::sync::mpsc::{Receiver};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::sleep;
use wg_2024::packet::Packet;
use crate::transmitter::Command;
use crate::transmitter::gateway::Gateway;

#[derive(Debug)]
pub struct TransmissionHandler<'a> {
    command_channel: UnboundedReceiver<Command>,
    packets: &'a [Packet], // contains the data to transmit
    window_size: usize,
    window_start: Cell<usize>,
    timeout: Duration,
    gateway: Arc<Gateway>,
    fragment_channels: HashMap<u64, mpsc::UnboundedSender<Command>>,
    pd: PhantomData<&'a u32>
}

impl<'a> TransmissionHandler<'a> {
    fn new(command_channel: UnboundedReceiver<Command>, packets: &'a[Packet], gateway: Arc<Gateway>) -> Self {
        Self {
            command_channel,
            packets,
            window_size: 1,
            window_start: Cell::new(0),
            timeout: Duration::from_secs(2),
            gateway,
            fragment_channels: HashMap::new(),
            pd: PhantomData::default() // TODO: remove this when lifetimes are used or no longer needed
        }
    }

    fn on_ack_received(&self) {
        let previous_start = self.window_start.get();
        self.window_start.set(previous_start + 1);
    }

    // 'static required to pass self.gateway to tasks
    async fn run(&'static mut self) {
        loop {
            let slice = &self.packets.get(self.window_start.get()..self.packets.len().min(self.window_start.get() + self.window_size));
            if let Some(ready_to_send) = slice {
                for (fragment_index, packet) in ready_to_send.iter().enumerate() {
                    let fragment_index = fragment_index as u64;
                    let fragment_command_channel = self.fragment_channels.get(&fragment_index);
                    match fragment_command_channel {
                        Some(_) => { },
                        None => {
                            let (tx, rx) = mpsc::unbounded_channel::<Command>();
                            self.fragment_channels.insert(fragment_index, tx);
                            let handle = Self::spawn_task(fragment_index, self.timeout, || {
                                self.gateway.forward(packet.clone());
                            }, rx);
                        }
                    };
                }
                tokio::select! {
                    command = self.command_channel.recv() => {
                        println!("received {command:?}");
                        if let Some(command) = command {
                            match command {
                                Command::Confirmed => {
                                    self.on_ack_received();
                                },
                                Command::Resend(fragment_index) => {
                                    match self.fragment_channels.get(&fragment_index) {
                                        Some(channel) => { channel.send(Command::Resend(fragment_index)); }
                                        None => {}
                                    }
                                }
                            }
                        }
                    }
                }
                // match self.command_channel.recv() {
                //     Some(command) => {
                //         println!("received {command:?}");
                //         match command {
                //             Command::Confirmed => {
                //                 self.on_ack_received();
                //             },
                //             Command::Resend(fragment_index) => {
                //                 match self.fragment_channels.get(&fragment_index) {
                //                     Some(channel) => { channel.send(Command::Resend(fragment_index)); }
                //                     None => {}
                //                 }
                //             }
                //         }
                //     },
                //     None => {
                //         // "should never happen"
                //     }
                // }
            }
            else {
                break;
            }
        }
    }

    async fn spawn_task<F>(
        id: u64,
        timeout: Duration,
        task_fn: F,
        mut command_channel: mpsc::UnboundedReceiver<Command>,
    ) -> JoinHandle<()>
    where
        F: Fn() + Send + 'static
    {
        tokio::spawn(async move {
            loop {
                task_fn();
                tokio::select! {
                    _ = sleep(timeout) => {
                        println!("Task {} timed out!", id);
                    }
                    Some(command) = command_channel.recv() => {
                        println!("task {id}: received command: {:?}", command);
                        match command {
                            Command::Resend(_) => {
                                println!("Processing resend command...");
                                continue;
                            }
                            Command::Confirmed => {
                                println!("Command confirmed, exiting loop.");
                                break;
                            }
                        }
                    }
                    else => {
                        println!("Command channel closed. Exiting loop.");
                        break;
                    }
                }
            }
            println!("task {id} finished");
        })
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::mpsc::unbounded_channel;
    use wg_2024::network::SourceRoutingHeader;
    use wg_2024::packet::{Ack, Packet, PacketType};
    use crate::transmitter::Command;
    use crate::transmitter::gateway::Gateway;
    use crate::transmitter::transmission_handler::TransmissionHandler;

    #[test]
    fn create() {
        let (command_tx, command_rx) = unbounded_channel::<Command>();
        let packet = Packet {
            pack_type: PacketType::Ack(Ack { fragment_index: 0 }),
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![0, 1, 2] },
            session_id: 0,
        };
        let packets = vec![packet];
        let drone_channels = crossbeam_channel::unbounded::<Packet>();
        let gateway = Gateway::new(0, HashMap::new(), drone_channels.0);
        let gateway = Arc::new(gateway);
        let transmission_handler = TransmissionHandler::new(
            command_rx,
            &packets[..],
            gateway
        );
        println!("{:?}", transmission_handler);
        assert_eq!(transmission_handler.packets.len(), 1);
        assert_eq!(transmission_handler.timeout, Duration::from_secs(2));
        assert_eq!(transmission_handler.window_start.get(), 0);
        assert_eq!(transmission_handler.window_size, 1);
    }

    #[test]
    fn check_transmission() {
        let (command_tx, command_rx) = unbounded_channel::<Command>();
        let packet = Packet {
            pack_type: PacketType::Ack(Ack { fragment_index: 0 }),
            routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![0, 1, 2] },
            session_id: 0,
        };
        let packets = vec![packet];
        let drone_channels = crossbeam_channel::unbounded::<Packet>();
        let gateway = Gateway::new(0, HashMap::new(), drone_channels.0);
        let gateway = Arc::new(gateway);
        let transmission_handler = TransmissionHandler::new(
            command_rx,
            &packets[..],
            gateway
        );
    }
}