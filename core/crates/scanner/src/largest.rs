//! Keeps the N largest files seen by any worker thread.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::result::LargeFile;

/// One candidate, ordered by allocated size, then path, so the heap is total.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Candidate {
    allocated: u64,
    path: PathBuf,
    logical: u64,
    modified: Option<SystemTime>,
}

/// A bounded min-heap of the largest files.
///
/// `floor` holds the smallest size in a full heap, so most files are turned
/// away without taking the lock or building their path.
pub struct Largest {
    limit: usize,
    floor: AtomicU64,
    heap: Mutex<BinaryHeap<Reverse<Candidate>>>,
}

impl Largest {
    /// Keeps at most `limit` files. A limit of 0 keeps none.
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            floor: AtomicU64::new(0),
            heap: Mutex::new(BinaryHeap::with_capacity(limit.saturating_add(1))),
        }
    }

    /// `true` if a file of `allocated` bytes could make the list.
    pub fn wants(&self, allocated: u64) -> bool {
        self.limit > 0 && allocated >= self.floor.load(Ordering::Relaxed)
    }

    /// Offers a file. `path` is only called if the file may make the list.
    pub fn offer(
        &self,
        allocated: u64,
        logical: u64,
        modified: Option<SystemTime>,
        path: impl FnOnce() -> PathBuf,
    ) {
        if !self.wants(allocated) {
            return;
        }
        let candidate = Candidate {
            allocated,
            path: path(),
            logical,
            modified,
        };
        let Ok(mut heap) = self.heap.lock() else {
            return;
        };
        heap.push(Reverse(candidate));
        if heap.len() > self.limit {
            heap.pop();
        }
        if heap.len() == self.limit {
            if let Some(Reverse(min)) = heap.peek() {
                self.floor.store(min.allocated, Ordering::Relaxed);
            }
        }
    }

    /// The kept files, largest first (ties by path).
    pub fn into_sorted(self) -> Vec<LargeFile> {
        let heap = match self.heap.into_inner() {
            Ok(heap) => heap,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut items: Vec<Candidate> = heap.into_iter().map(|Reverse(c)| c).collect();
        items.sort_by(|a, b| {
            b.allocated
                .cmp(&a.allocated)
                .then_with(|| a.path.cmp(&b.path))
        });
        items
            .into_iter()
            .map(|c| LargeFile {
                path: c.path,
                allocated_bytes: c.allocated,
                logical_bytes: c.logical,
                modified: c.modified,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_largest_in_order() {
        let largest = Largest::new(3);
        for (i, size) in [5u64, 1, 9, 7, 3, 8].into_iter().enumerate() {
            largest.offer(size, size, None, || PathBuf::from(format!("/f{i}")));
        }
        let sizes: Vec<u64> = largest
            .into_sorted()
            .iter()
            .map(|f| f.allocated_bytes)
            .collect();
        assert_eq!(sizes, vec![9, 8, 7]);
    }

    #[test]
    fn limit_zero_keeps_nothing_and_never_builds_paths() {
        let largest = Largest::new(0);
        largest.offer(100, 100, None, || panic!("path should not be built"));
        assert!(largest.into_sorted().is_empty());
    }
}
