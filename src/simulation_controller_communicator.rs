use crossbeam_channel::Sender;
use messages::node_event::NodeEvent;

pub struct SimulationControllerCommunicator {
    simulation_controller_tx: Sender<NodeEvent>,
}

impl SimulationControllerCommunicator {
    pub fn new(simulation_controller_tx: Sender<NodeEvent>) -> Self {
        Self {
            simulation_controller_tx,
        }
    }

    pub fn send_event(&self, node_event: NodeEvent) {
        match self.simulation_controller_tx.send(node_event.clone()) {
            Ok(()) => log::info!("Node event {node_event:?} sent"),
            Err(err) => {
                log::error!("Cannot send events to simulation controller");
                panic!("Cannot send events to simulation controller");
            }
        }
    }
}