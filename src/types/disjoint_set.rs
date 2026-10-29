use std::cell::Cell;

pub struct DisjointSet {
    parents: Vec<Cell<usize>>,
    members_of: Vec<Vec<usize>>,
}

impl DisjointSet {
    fn get_parent(&self, id: usize) -> usize {
        self.parents[id].get()
    }

    fn set_parent(&self, id: usize, new: usize) {
        self.parents[id].set(new);
    }

    // find with path splitting
    pub fn root_of(&self, mut child: usize) -> usize {
        let mut parent = self.get_parent(child);

        if child == parent {
            return child;
        };

        loop {
            let grandparent = self.get_parent(parent);
            if parent == grandparent {
                return parent;
            }

            self.set_parent(child, grandparent);
            child = parent;
            parent = grandparent;
        }
    }

    // union
    pub fn union(&mut self, x: usize, y: usize) {
        let root_x = self.root_of(x);
        let root_y = self.root_of(y);

        if root_x == root_y {
            return;
        }

        let (root, merger) = if self.members_of[root_x].len() >= self.members_of[root_y].len() {
            (root_x, root_y)
        } else {
            (root_y, root_x)
        };

        self.set_parent(merger, root);
        let moved = std::mem::take(&mut self.members_of[merger]);
        self.members_of[root].extend(moved);
    }

    pub fn in_same_group(&self, x: usize, y: usize) -> bool {
        self.root_of(x) == self.root_of(y)
    }

    pub fn members_of(&self, id: usize) -> &[usize] {
        let root = self.root_of(id);
        &self.members_of[root]
    }

    pub fn get_roots(&self) -> Vec<usize> {
        (0..self.len())
            .filter(|&index| self.parents[index].get() == index)
            .collect()
    }

    pub fn with_len(len: usize) -> Self {
        Self {
            parents: (0..len).map(Cell::new).collect(),
            members_of: (0..len).map(|x| vec![x]).collect(),
        }
    }

    pub fn new() -> Self {
        Self {
            parents: Vec::new(),
            members_of: Vec::new(),
        }
    }

    fn len(&self) -> usize {
        self.parents.len()
    }
}

impl Default for DisjointSet {
    fn default() -> Self {
        Self::new()
    }
}
