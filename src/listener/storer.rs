use std::collections::{HashMap, HashSet};
use wg_2024::packet::Fragment;

pub(super) struct Storer {
    fragment_count: usize,
    fragments: HashMap<u64, Fragment>,
}

impl Storer {
    pub(super) fn new(fragment_count: usize) -> Self {
        Self {
            fragment_count,
            fragments: Default::default(),
        }
    }

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
        self.fragments.insert(fragment.fragment_index, fragment);
    }

    pub(super) fn is_ready(&self) -> bool {
        self.fragments.len() == self.fragment_count
    }

    pub(super) fn get_fragments(&self) -> Vec<Fragment> {
        self
            .fragments
            .iter()
            .map(|(_fragment_index, fragment)| fragment.clone())
            .collect()
    }
}