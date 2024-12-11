use std::collections::{HashMap, HashSet};
use wg_2024::packet::Fragment;

pub(super) struct Storer {
    fragment_count: usize,
    fragments: HashMap<u64, Fragment>,
}

impl Storer {
    /// Creates a new instance of Storer with the given fragment count
    pub(super) fn new(fragment_count: usize) -> Self {
        Self {
            fragment_count,
            fragments: Default::default(),
        }
    }

    /// Creates a new instance of Storer from a Fragment, setting the fragment_count
    pub(super) fn new_from_fragment(fragment: Fragment) -> Self {
        let fragment_count = fragment.total_n_fragments as usize;
        let mut result = Self {
            fragment_count,
            fragments: Default::default(),
        };
        result.insert_fragment(fragment);
        result
    }

    /// Inserts a fragment into Storer
    pub(super) fn insert_fragment(&mut self, fragment: Fragment) {
        if (fragment.fragment_index as usize) >= self.fragment_count {
            // TODO: check if there is an alternative to panic maybe?
            panic!("fragment index {} out of bounds", fragment.fragment_index);
        }
        self.fragments.insert(fragment.fragment_index, fragment);
    }

    /// Checks whether all the fragments for this session_id have been received
    pub(super) fn is_ready(&self) -> bool {
        self.fragments.len() == self.fragment_count
    }

    /// Returns a Vec<Fragment> containing all the received fragments
    pub(super) fn get_fragments(&self) -> Vec<Fragment> {
        let mut vector = self.fragments
            .iter()
            .map(|(_fragment_index, fragment)| fragment.clone())
            .collect::<Vec<Fragment>>();
        vector.sort_by_key(|fragment: &Fragment| fragment.fragment_index);
        vector
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use wg_2024::packet::Fragment;
    use crate::listener::storer::Storer;

    #[test]
    fn initialize_new() {
        let storer = Storer::new(1);

        assert_eq!(storer.fragment_count, 1);
        assert_eq!(storer.fragments, HashMap::new());
    }

    #[test]
    fn initialize_from_fragment() {
        let fragment = Fragment {
            fragment_index: 0,
            total_n_fragments: 2,
            length: 0,
            data: [0; 128],
        };
        let storer = Storer::new_from_fragment(fragment.clone());

        assert_eq!(storer.fragment_count, 2);
        assert_eq!(*storer.fragments.get(&0).unwrap(), fragment);
    }

    #[test]
    fn check_ready_true() {
        let fragment = Fragment {
            fragment_index: 0,
            total_n_fragments: 1,
            length: 0,
            data: [0; 128],
        };
        let storer = Storer::new_from_fragment(fragment.clone());

        assert!(storer.is_ready());
    }

    #[test]
    fn check_ready_false() {
        let fragment = Fragment {
            fragment_index: 0,
            total_n_fragments: 2,
            length: 0,
            data: [0; 128],
        };
        let storer = Storer::new_from_fragment(fragment.clone());

        assert!(!storer.is_ready());
    }

    #[test]
    fn insert_fragment() {
        let mut storer = Storer::new(1);
        let fragment = Fragment {
            fragment_index: 0,
            total_n_fragments: 2,
            length: 0,
            data: [0; 128],
        };

        storer.insert_fragment(fragment.clone());
        assert_eq!(fragment, *storer.fragments.get(&0).unwrap())
    }

    #[test]
    fn get_fragments_vector() {
        let fragment_0 = Fragment {
            fragment_index: 0,
            total_n_fragments: 3,
            length: 0,
            data: [0; 128],
        };
        let fragment_1 = Fragment {
            fragment_index: 1,
            total_n_fragments: 3,
            length: 0,
            data: [0; 128],
        };
        let fragment_2 = Fragment {
            fragment_index: 2,
            total_n_fragments: 3,
            length: 0,
            data: [0; 128],
        };

        let mut storer = Storer::new_from_fragment(fragment_0.clone());
        storer.insert_fragment(fragment_1.clone());
        storer.insert_fragment(fragment_2.clone());

        let fragments = storer.get_fragments();

        let expected = vec![fragment_0, fragment_1, fragment_2];
        assert_eq!(fragments, expected);
    }
}
