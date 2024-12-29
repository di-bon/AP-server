use std::sync::RwLock;
use wg_2024::network::NodeId;
use wg_2024::packet::NodeType;

#[derive(Debug)]
pub(super) struct NetworkNode {
    pub(super) node_id: NodeId,
    pub(super) node_type: NodeType,
    pub(super) num_of_dropped_packets: u64, // TODO: maybe it is useful to add some timestamps or whatever
    // to delete old dropped packets, so that if an unreliable drone gets its pdr changed it get
    // selected during the path finding part, or vice versa, if a reliable drone gets its pdr raised
    pub(super) neighbors: RwLock<Vec<NodeId>>
}

impl PartialEq for NetworkNode {
    fn eq(&self, other: &Self) -> bool {
        // TODO: is this ok?
        self.node_id == other.node_id
    }
}

impl Eq for NetworkNode { }

impl NetworkNode {
    pub(super) fn new(
        node_id: NodeId,
        node_type: NodeType,
    ) -> Self {
        Self {
            node_id,
            node_type,
            num_of_dropped_packets: 0,
            neighbors: RwLock::new(vec![]),
        }
    }

    pub(super) fn insert_edge(&self, to: NodeId) {
        self.neighbors.write().unwrap().push(to)
    }

    pub(super) fn remove_edge(&self, to: NodeId) {
        let index = self.neighbors.read().unwrap().iter().position(|node_id| *node_id == to);
        if let Some(index) = index {
            self.neighbors.write().unwrap().remove(index);
        }
    }

    pub(super) fn increment_dropped_packets(&mut self) {
        self.num_of_dropped_packets += 1
    }

    pub(super) fn reset_num_of_dropped_packets(&mut self) {
        self.num_of_dropped_packets = 0;
    }

    pub(super) fn get_num_of_dropped_packets(&self) -> u64 {
        self.num_of_dropped_packets
    }
}

// TODO: update tests to use Arc and RwLock instead of Rc and RefCell
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_no_neighbors() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let node = NetworkNode::new(node_id, node_type);

        let expected = NetworkNode {
            node_id,
            node_type,
            num_of_dropped_packets: 0,
            neighbors: RwLock::new(vec![]),
        };

        assert_eq!(node, expected);
    }

    #[test]
    fn add_neighbors() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let mut node = NetworkNode::new(node_id, node_type);

        node.insert_edge(1);

        let expected = NetworkNode {
            node_id,
            node_type,
            num_of_dropped_packets: 0,
            neighbors: RwLock::new(vec![1]),
        };

        assert_eq!(node, expected);
    }

    #[test]
    fn remove_neighbors() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let mut node = NetworkNode::new(node_id, node_type);

        node.insert_edge(1);

        let expected = NetworkNode {
            node_id,
            node_type,
            num_of_dropped_packets: 0,
            neighbors: RwLock::new(vec![1]),
        };

        assert_eq!(node, expected);

        node.remove_edge(1);

        let expected = NetworkNode {
            node_id,
            node_type,
            num_of_dropped_packets: 0,
            neighbors: RwLock::new(vec![]),
        };

        assert_eq!(node, expected);
    }

    #[test]
    fn increment_dropped_packets() {
        let node_id = 0;
        let node_type = NodeType::Server;
        let mut node = NetworkNode::new(node_id, node_type);

        node.increment_dropped_packets();
        node.increment_dropped_packets();
        node.increment_dropped_packets();

        let expected = NetworkNode {
            node_id,
            node_type,
            num_of_dropped_packets: 3,
            neighbors: RwLock::new(vec![]),
        };

        assert_eq!(node, expected);
    }
}