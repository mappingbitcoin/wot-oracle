use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;

use super::interner::PubkeyInterner;
use super::metrics::{LockMetrics, LockMetricsSnapshot, LockTimer};

/// Node metadata (pubkey is stored separately via interner)
#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub kind3_event_id: Option<String>,
    pub kind3_created_at: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub nodes_with_follows: usize,
}

pub struct WotGraph {
    interner: PubkeyInterner,
    pubkey_to_id: DashMap<Arc<str>, u32>,
    id_to_pubkey: RwLock<Vec<Arc<str>>>,
    // Sorted Vec<u32> for cache-friendly iteration and O(log n) membership checks
    follows: RwLock<Vec<Vec<u32>>>,
    followers: RwLock<Vec<Vec<u32>>>,
    node_info: RwLock<Vec<Option<NodeInfo>>>,
    lock_metrics: LockMetrics,
}

impl WotGraph {
    pub fn new() -> Self {
        Self {
            interner: PubkeyInterner::new(),
            pubkey_to_id: DashMap::new(),
            id_to_pubkey: RwLock::new(Vec::new()),
            follows: RwLock::new(Vec::new()),
            followers: RwLock::new(Vec::new()),
            node_info: RwLock::new(Vec::new()),
            lock_metrics: LockMetrics::new(),
        }
    }

    pub fn get_or_create_node(&self, pubkey: &str) -> u32 {
        // Fast path: check if already exists
        if let Some(id) = self.pubkey_to_id.get(pubkey) {
            return *id;
        }

        let mut id_to_pubkey = self.id_to_pubkey.write();
        let mut follows = self.follows.write();
        let mut followers = self.followers.write();
        let mut node_info = self.node_info.write();

        // Double-check after acquiring write lock
        if let Some(id) = self.pubkey_to_id.get(pubkey) {
            return *id;
        }

        // Intern the pubkey string - single allocation shared everywhere
        let interned = self.interner.intern(pubkey);

        let id = id_to_pubkey.len() as u32;
        id_to_pubkey.push(interned.clone());
        follows.push(Vec::new());
        followers.push(Vec::new());
        node_info.push(None);
        self.pubkey_to_id.insert(interned, id);

        id
    }

    pub fn get_node_id(&self, pubkey: &str) -> Option<u32> {
        self.pubkey_to_id.get(pubkey).map(|r| *r)
    }

    /// Get node ID and Arc<str> reference together (single lookup)
    pub fn get_node_id_and_arc(&self, pubkey: &str) -> Option<(u32, Arc<str>)> {
        self.pubkey_to_id.get(pubkey).map(|r| {
            let id = *r;
            let arc = r.key().clone();
            (id, arc)
        })
    }

    /// Get Arc<str> reference by pubkey string (no allocation if found)
    pub fn get_pubkey_arc_by_str(&self, pubkey: &str) -> Option<Arc<str>> {
        self.pubkey_to_id.get(pubkey).map(|r| r.key().clone())
    }

    /// Get pubkey as Arc<str> for internal use (no allocation)
    pub fn get_pubkey_arc(&self, id: u32) -> Option<Arc<str>> {
        let id_to_pubkey = self.id_to_pubkey.read();
        id_to_pubkey.get(id as usize).cloned()
    }

    pub fn update_follows(
        &self,
        pubkey: &str,
        follow_pubkeys: &[String],
        event_id: Option<String>,
        created_at: Option<i64>,
    ) -> bool {
        let node_id = self.get_or_create_node(pubkey);

        // Check if we should update (only if newer event)
        {
            let node_info = self.node_info.read();
            if let Some(Some(info)) = node_info.get(node_id as usize) {
                if let (Some(existing_ts), Some(new_ts)) = (info.kind3_created_at, created_at) {
                    if new_ts <= existing_ts {
                        return false; // Event is older or same age, skip
                    }
                }
            }
        }

        // Get or create IDs for all follows and sort them
        let mut new_follow_ids: Vec<u32> = follow_pubkeys
            .iter()
            .map(|pk| self.get_or_create_node(pk))
            .collect();
        new_follow_ids.sort_unstable();
        new_follow_ids.dedup();

        // Read old follows under read lock (quick clone)
        let old_follow_ids: Vec<u32> = {
            let follows = self.follows.read();
            follows
                .get(node_id as usize)
                .cloned()
                .unwrap_or_default()
        };

        // Compute diff OUTSIDE any lock - no contention during this work
        let to_remove: Vec<u32> = old_follow_ids
            .iter()
            .filter(|id| new_follow_ids.binary_search(id).is_err())
            .copied()
            .collect();
        let to_add: Vec<u32> = new_follow_ids
            .iter()
            .filter(|id| old_follow_ids.binary_search(id).is_err())
            .copied()
            .collect();

        // Minimal write lock - only actual mutations
        {
            let _timer = LockTimer::write(&self.lock_metrics);
            let mut follows = self.follows.write();
            let mut followers = self.followers.write();

            // Remove old follower references (only changed ones)
            for &old_followed_id in &to_remove {
                if let Some(follower_list) = followers.get_mut(old_followed_id as usize) {
                    if let Ok(pos) = follower_list.binary_search(&node_id) {
                        follower_list.remove(pos);
                    }
                }
            }

            // Update follows list
            if let Some(follow_list) = follows.get_mut(node_id as usize) {
                *follow_list = new_follow_ids;
            }

            // Add new follower references (only changed ones)
            for &followed_id in &to_add {
                if let Some(follower_list) = followers.get_mut(followed_id as usize) {
                    match follower_list.binary_search(&node_id) {
                        Ok(_) => {}
                        Err(pos) => follower_list.insert(pos, node_id),
                    }
                }
            }
        }

        // Update node info (pubkey stored via interner, not duplicated here)
        {
            let mut node_info = self.node_info.write();
            if let Some(info_slot) = node_info.get_mut(node_id as usize) {
                *info_slot = Some(NodeInfo {
                    kind3_event_id: event_id,
                    kind3_created_at: created_at,
                });
            }
        }

        true
    }

    pub fn get_follows(&self, pubkey: &str) -> Option<Vec<String>> {
        let node_id = self.get_node_id(pubkey)?;
        let follows = self.follows.read();
        let id_to_pubkey = self.id_to_pubkey.read();

        follows.get(node_id as usize).map(|follow_list| {
            follow_list
                .iter()
                .filter_map(|&id| id_to_pubkey.get(id as usize).map(|arc| arc.to_string()))
                .collect()
        })
    }

    pub fn get_followers(&self, pubkey: &str) -> Option<Vec<String>> {
        let node_id = self.get_node_id(pubkey)?;
        let followers = self.followers.read();
        let id_to_pubkey = self.id_to_pubkey.read();

        followers.get(node_id as usize).map(|follower_list| {
            follower_list
                .iter()
                .filter_map(|&id| id_to_pubkey.get(id as usize).map(|arc| arc.to_string()))
                .collect()
        })
    }

    /// Execute a closure with read access to both adjacency lists.
    /// Holds a single read lock for the entire operation - use for BFS traversals.
    pub fn with_adjacency<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[Vec<u32>], &[Vec<u32>]) -> R,
    {
        let _timer = LockTimer::read(&self.lock_metrics);
        let follows = self.follows.read();
        let followers = self.followers.read();
        f(&follows, &followers)
    }

    /// Batch resolve node IDs to pubkeys as Arc<str> (no allocation)
    pub fn resolve_pubkeys_arc(&self, ids: &[u32]) -> Vec<Arc<str>> {
        let id_to_pubkey = self.id_to_pubkey.read();
        ids.iter()
            .filter_map(|&id| id_to_pubkey.get(id as usize).cloned())
            .collect()
    }

    pub fn get_node_info(&self, pubkey: &str) -> Option<NodeInfo> {
        let node_id = self.get_node_id(pubkey)?;
        let node_info = self.node_info.read();
        node_info.get(node_id as usize).and_then(|info| info.clone())
    }

    pub fn stats(&self) -> GraphStats {
        let follows = self.follows.read();
        let id_to_pubkey = self.id_to_pubkey.read();

        let node_count = id_to_pubkey.len();
        let edge_count: usize = follows.iter().map(|list| list.len()).sum();
        let nodes_with_follows = follows.iter().filter(|list| !list.is_empty()).count();

        GraphStats {
            node_count,
            edge_count,
            nodes_with_follows,
        }
    }

    /// Get lock contention metrics
    pub fn lock_metrics(&self) -> LockMetricsSnapshot {
        self.lock_metrics.snapshot()
    }

    /// Reset lock metrics (useful after warmup period)
    pub fn reset_lock_metrics(&self) {
        self.lock_metrics.reset();
    }
}

impl Default for WotGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_nodes() {
        let graph = WotGraph::new();
        let id1 = graph.get_or_create_node("pubkey1");
        let id2 = graph.get_or_create_node("pubkey2");
        let id1_again = graph.get_or_create_node("pubkey1");

        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(id1, id1_again);
    }

    #[test]
    fn test_update_follows() {
        let graph = WotGraph::new();

        graph.update_follows(
            "alice",
            &["bob".to_string(), "carol".to_string()],
            Some("event1".to_string()),
            Some(1000),
        );

        let follows = graph.get_follows("alice").unwrap();
        assert_eq!(follows.len(), 2);
        assert!(follows.contains(&"bob".to_string()));
        assert!(follows.contains(&"carol".to_string()));

        let bob_followers = graph.get_followers("bob").unwrap();
        assert!(bob_followers.contains(&"alice".to_string()));
    }

    #[test]
    fn test_replace_follows() {
        let graph = WotGraph::new();

        graph.update_follows("alice", &["bob".to_string()], None, Some(1000));
        graph.update_follows("alice", &["carol".to_string()], None, Some(2000));

        let follows = graph.get_follows("alice").unwrap();
        assert_eq!(follows.len(), 1);
        assert!(follows.contains(&"carol".to_string()));

        // Bob should no longer have alice as follower
        let bob_followers = graph.get_followers("bob").unwrap();
        assert!(!bob_followers.contains(&"alice".to_string()));
    }

    #[test]
    fn test_skip_old_event() {
        let graph = WotGraph::new();

        graph.update_follows("alice", &["bob".to_string()], None, Some(2000));
        let result = graph.update_follows("alice", &["carol".to_string()], None, Some(1000));

        assert!(!result); // Should skip old event

        let follows = graph.get_follows("alice").unwrap();
        assert!(follows.contains(&"bob".to_string()));
        assert!(!follows.contains(&"carol".to_string()));
    }

    #[test]
    fn test_stats() {
        let graph = WotGraph::new();

        graph.update_follows("alice", &["bob".to_string(), "carol".to_string()], None, None);
        graph.update_follows("bob", &["carol".to_string()], None, None);

        let stats = graph.stats();
        assert_eq!(stats.node_count, 3);
        assert_eq!(stats.edge_count, 3);
        assert_eq!(stats.nodes_with_follows, 2);
    }

    #[test]
    fn test_sorted_follows() {
        let graph = WotGraph::new();

        // Insert in random order
        graph.update_follows(
            "alice",
            &["zebra".to_string(), "apple".to_string(), "mango".to_string()],
            None,
            None,
        );

        // Internal IDs should be sorted
        let alice_id = graph.get_node_id("alice").unwrap();

        // Verify sorted order using with_adjacency
        graph.with_adjacency(|follows, _| {
            let follows_ids = &follows[alice_id as usize];
            for i in 1..follows_ids.len() {
                assert!(follows_ids[i - 1] < follows_ids[i], "follows should be sorted");
            }
        });
    }

    #[test]
    fn test_binary_search_is_direct_follow() {
        let graph = WotGraph::new();

        graph.update_follows(
            "alice",
            &["bob".to_string(), "carol".to_string(), "dave".to_string()],
            None,
            None,
        );

        let alice_id = graph.get_node_id("alice").unwrap();
        let bob_id = graph.get_node_id("bob").unwrap();
        let carol_id = graph.get_node_id("carol").unwrap();
        let eve_id = graph.get_or_create_node("eve");

        // Test using with_adjacency and binary search
        graph.with_adjacency(|follows, _| {
            let alice_follows = &follows[alice_id as usize];
            assert!(alice_follows.binary_search(&bob_id).is_ok());
            assert!(alice_follows.binary_search(&carol_id).is_ok());
            assert!(alice_follows.binary_search(&eve_id).is_err());

            let bob_follows = &follows[bob_id as usize];
            assert!(bob_follows.binary_search(&alice_id).is_err());
        });
    }
}
