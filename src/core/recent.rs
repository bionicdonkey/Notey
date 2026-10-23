use serde::{Deserialize, Serialize};

/// Most-recently-used list of paths, newest first, without duplicates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentList {
    pub max: usize,
    pub items: Vec<String>,
}

impl RecentList {
    pub fn new(max: usize) -> Self {
        Self { max, items: Vec::new() }
    }

    pub fn push(&mut self, path: &str) {
        self.items.retain(|p| p != path);
        self.items.insert(0, path.to_string());
        self.items.truncate(self.max);
    }

    pub fn remove(&mut self, path: &str) {
        self.items.retain(|p| p != path);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_deduplicated_and_bounded() {
        let mut r = RecentList::new(3);
        for p in ["a", "b", "c", "a", "d"] {
            r.push(p);
        }
        assert_eq!(r.items, ["d", "a", "c"]);
        r.remove("a");
        assert_eq!(r.items, ["d", "c"]);
        r.clear();
        assert!(r.items.is_empty());
    }
}
