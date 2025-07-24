use crate::{SymbolId, Relationship, RelationKind};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::sync::RwLock;

pub struct DependencyGraph {
    graph: RwLock<DiGraph<SymbolId, Relationship>>,
    node_map: RwLock<HashMap<SymbolId, NodeIndex>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            graph: RwLock::new(DiGraph::new()),
            node_map: RwLock::new(HashMap::new()),
        }
    }

    pub fn add_symbol(&self, symbol_id: SymbolId) -> NodeIndex {
        let mut graph = self.graph.write().unwrap();
        let mut node_map = self.node_map.write().unwrap();
        
        if let Some(&node_idx) = node_map.get(&symbol_id) {
            node_idx
        } else {
            let node_idx = graph.add_node(symbol_id);
            node_map.insert(symbol_id, node_idx);
            node_idx
        }
    }

    pub fn add_relationship(
        &self,
        from: SymbolId,
        to: SymbolId,
        relationship: Relationship,
    ) -> Option<()> {
        let from_idx = self.add_symbol(from);
        let to_idx = self.add_symbol(to);
        
        let mut graph = self.graph.write().unwrap();
        graph.add_edge(from_idx, to_idx, relationship);
        
        Some(())
    }

    pub fn get_relationships(&self, symbol_id: SymbolId, kind: RelationKind) -> Vec<SymbolId> {
        let graph = self.graph.read().unwrap();
        let node_map = self.node_map.read().unwrap();
        
        if let Some(&node_idx) = node_map.get(&symbol_id) {
            graph
                .edges(node_idx)
                .filter(|edge| edge.weight().kind == kind)
                .filter_map(|edge| graph.node_weight(edge.target()).copied())
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_incoming_relationships(
        &self,
        symbol_id: SymbolId,
        kind: RelationKind,
    ) -> Vec<SymbolId> {
        let graph = self.graph.read().unwrap();
        let node_map = self.node_map.read().unwrap();
        
        if let Some(&node_idx) = node_map.get(&symbol_id) {
            graph
                .edges_directed(node_idx, petgraph::Direction::Incoming)
                .filter(|edge| edge.weight().kind == kind)
                .filter_map(|edge| graph.node_weight(edge.source()).copied())
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn traverse_bfs(&self, start: SymbolId, max_depth: Option<usize>) -> Vec<Vec<SymbolId>> {
        let graph = self.graph.read().unwrap();
        let node_map = self.node_map.read().unwrap();
        
        let mut levels = Vec::new();
        
        if let Some(&start_idx) = node_map.get(&start) {
            let mut current_level = vec![start_idx];
            let mut visited = std::collections::HashSet::new();
            visited.insert(start_idx);
            
            while !current_level.is_empty() {
                if let Some(max_d) = max_depth {
                    if levels.len() >= max_d {
                        break;
                    }
                }
                
                let mut level_symbols = Vec::new();
                let mut next_level = Vec::new();
                
                for node_idx in current_level {
                    if let Some(&symbol_id) = graph.node_weight(node_idx) {
                        level_symbols.push(symbol_id);
                        
                        for edge in graph.edges(node_idx) {
                            let target = edge.target();
                            if !visited.contains(&target) {
                                visited.insert(target);
                                next_level.push(target);
                            }
                        }
                    }
                }
                
                if !level_symbols.is_empty() {
                    levels.push(level_symbols);
                }
                
                current_level = next_level;
            }
        }
        
        levels
    }

    pub fn find_paths(&self, from: SymbolId, to: SymbolId) -> Vec<Vec<SymbolId>> {
        let graph = self.graph.read().unwrap();
        let node_map = self.node_map.read().unwrap();
        
        let from_idx = match node_map.get(&from) {
            Some(&idx) => idx,
            None => return Vec::new(),
        };
        
        let to_idx = match node_map.get(&to) {
            Some(&idx) => idx,
            None => return Vec::new(),
        };
        
        // Simple DFS path finding (could be optimized)
        let mut paths = Vec::new();
        let mut current_path = Vec::new();
        let mut visited = HashMap::new();
        
        self.dfs_paths(
            &graph,
            from_idx,
            to_idx,
            &mut current_path,
            &mut visited,
            &mut paths,
        );
        
        paths
    }

    fn dfs_paths(
        &self,
        graph: &DiGraph<SymbolId, Relationship>,
        current: NodeIndex,
        target: NodeIndex,
        current_path: &mut Vec<SymbolId>,
        visited: &mut HashMap<NodeIndex, bool>,
        all_paths: &mut Vec<Vec<SymbolId>>,
    ) {
        if current == target {
            if let Some(&symbol_id) = graph.node_weight(current) {
                current_path.push(symbol_id);
                all_paths.push(current_path.clone());
                current_path.pop();
            }
            return;
        }
        
        visited.insert(current, true);
        
        if let Some(&symbol_id) = graph.node_weight(current) {
            current_path.push(symbol_id);
            
            for edge in graph.edges(current) {
                let next = edge.target();
                if !visited.get(&next).unwrap_or(&false) {
                    self.dfs_paths(graph, next, target, current_path, visited, all_paths);
                }
            }
            
            current_path.pop();
        }
        
        visited.insert(current, false);
    }

    pub fn clear(&self) {
        let mut graph = self.graph.write().unwrap();
        let mut node_map = self.node_map.write().unwrap();
        
        graph.clear();
        node_map.clear();
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_symbol() {
        let graph = DependencyGraph::new();
        let symbol_id = SymbolId::new(1).unwrap();
        
        let node_idx = graph.add_symbol(symbol_id);
        let node_idx2 = graph.add_symbol(symbol_id); // Should return same index
        
        assert_eq!(node_idx, node_idx2);
    }

    #[test]
    fn test_add_relationship() {
        let graph = DependencyGraph::new();
        let id1 = SymbolId::new(1).unwrap();
        let id2 = SymbolId::new(2).unwrap();
        
        let rel = Relationship::new(RelationKind::Calls);
        graph.add_relationship(id1, id2, rel);
        
        let callees = graph.get_relationships(id1, RelationKind::Calls);
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0], id2);
    }

    #[test]
    fn test_get_incoming_relationships() {
        let graph = DependencyGraph::new();
        let id1 = SymbolId::new(1).unwrap();
        let id2 = SymbolId::new(2).unwrap();
        let id3 = SymbolId::new(3).unwrap();
        
        graph.add_relationship(id1, id3, Relationship::new(RelationKind::Calls));
        graph.add_relationship(id2, id3, Relationship::new(RelationKind::Calls));
        
        let callers = graph.get_incoming_relationships(id3, RelationKind::Calls);
        assert_eq!(callers.len(), 2);
        assert!(callers.contains(&id1));
        assert!(callers.contains(&id2));
    }

    #[test]
    fn test_traverse_bfs() {
        let graph = DependencyGraph::new();
        
        // Create a simple graph: 1 -> 2 -> 3
        //                            -> 4
        let id1 = SymbolId::new(1).unwrap();
        let id2 = SymbolId::new(2).unwrap();
        let id3 = SymbolId::new(3).unwrap();
        let id4 = SymbolId::new(4).unwrap();
        
        graph.add_relationship(id1, id2, Relationship::new(RelationKind::Calls));
        graph.add_relationship(id2, id3, Relationship::new(RelationKind::Calls));
        graph.add_relationship(id2, id4, Relationship::new(RelationKind::Calls));
        
        let levels = graph.traverse_bfs(id1, Some(3));
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], vec![id1]);
        assert_eq!(levels[1], vec![id2]);
        assert_eq!(levels[2].len(), 2); // id3 and id4
    }

    #[test]
    fn test_find_paths() {
        let graph = DependencyGraph::new();
        
        // Create a graph with multiple paths: 1 -> 2 -> 4
        //                                       -> 3 -> 4
        let id1 = SymbolId::new(1).unwrap();
        let id2 = SymbolId::new(2).unwrap();
        let id3 = SymbolId::new(3).unwrap();
        let id4 = SymbolId::new(4).unwrap();
        
        graph.add_relationship(id1, id2, Relationship::new(RelationKind::Calls));
        graph.add_relationship(id1, id3, Relationship::new(RelationKind::Calls));
        graph.add_relationship(id2, id4, Relationship::new(RelationKind::Calls));
        graph.add_relationship(id3, id4, Relationship::new(RelationKind::Calls));
        
        let paths = graph.find_paths(id1, id4);
        assert_eq!(paths.len(), 2);
        
        // Check that both paths are valid
        for path in paths {
            assert_eq!(path.first(), Some(&id1));
            assert_eq!(path.last(), Some(&id4));
            assert_eq!(path.len(), 3);
        }
    }
}