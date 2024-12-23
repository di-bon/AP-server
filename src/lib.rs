use std::collections::HashMap;
use crossbeam_channel::unbounded;
use wg_2024::packet::Packet;
use crate::transmitter::Transmitter;

mod transmitter;
mod listener;
mod server_logic;

pub fn run() {
    // just for testing cargo clippy
    // let (tx, rx) = unbounded::<Packet>();
    // let (sl_tx, sl_rx) = unbounded::<Packet>();
    // let transmitter = Transmitter::new(0, rx, tx, sl_rx, HashMap::new());
    // transmitter.run();
}