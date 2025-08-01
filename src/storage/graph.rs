use crate::{SymbolId, Relationship, RelationKind};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug)]
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
    
    pub fn remove_symbol(&self, symbol_id: SymbolId) {
        let mut graph = self.graph.write().unwrap();
        let mut node_map = self.node_map.write().unwrap();
        
        if let Some(node_idx) = node_map.remove(&symbol_id) {
            graph.remove_node(node_idx);
        }
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

    /// DFS traversal requires &self for recursive calls to maintain graph context
    #[allow(clippy::only_used_in_recursion)]
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

    pub fn get_impact_radius(&self, symbol_id: SymbolId, max_depth: Option<usize>) -> Vec<SymbolId> {
        let max_depth = max_depth.unwrap_or(5);
        let graph = self.graph.read().unwrap();
        let node_map = self.node_map.read().unwrap();
        
        let mut impacted = Vec::new();
        let mut visited = std::collections::HashSet::new();
        
        if let Some(&start_idx) = node_map.get(&symbol_id) {
            let mut current_level = vec![start_idx];
            visited.insert(start_idx);
            let mut depth = 0;
            
            while !current_level.is_empty() && depth < max_depth {
                let mut next_level = Vec::new();
                
                for node_idx in current_level {
                    // Look at incoming edges (who depends on this symbol)
                    for edge in graph.edges_directed(node_idx, petgraph::Direction::Incoming) {
                        let source = edge.source();
                        if !visited.contains(&source) {
                            visited.insert(source);
                            next_level.push(source);
                            if let Some(&impacted_id) = graph.node_weight(source) {
                                impacted.push(impacted_id);
                            }
                        }
                    }
                }
                
                current_level = next_level;
                depth += 1;
            }
        }
        
        impacted
    }

    pub fn get_dependencies(&self, symbol_id: SymbolId) -> std::collections::HashMap<RelationKind, Vec<SymbolId>> {
        let graph = self.graph.read().unwrap();
        let node_map = self.node_map.read().unwrap();
        let mut dependencies = std::collections::HashMap::new();
        
        if let Some(&node_idx) = node_map.get(&symbol_id) {
            // Group outgoing edges by relationship kind
            for edge in graph.edges(node_idx) {
                let target_id = graph.node_weight(edge.target()).copied();
                if let Some(target_id) = target_id {
                    dependencies
                        .entry(edge.weight().kind)
                        .or_insert_with(Vec::new)
                        .push(target_id);
                }
            }
        }
        
        dependencies
    }
    
    pub fn get_dependents(&self, symbol_id: SymbolId) -> std::collections::HashMap<RelationKind, Vec<SymbolId>> {
        let graph = self.graph.read().unwrap();
        let node_map = self.node_map.read().unwrap();
        let mut dependents = std::collections::HashMap::new();
        
        if let Some(&node_idx) = node_map.get(&symbol_id) {
            // Group incoming edges by relationship kind
            for edge in graph.edges_directed(node_idx, petgraph::Direction::Incoming) {
                let source_id = graph.node_weight(edge.source()).copied();
                if let Some(source_id) = source_id {
                    dependents
                        .entry(edge.weight().kind)
                        .or_insert_with(Vec::new)
                        .push(source_id);
                }
            }
        }
        
        dependents
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
    
    #[test]
    fn test_get_impact_radius() {
        let graph = DependencyGraph::new();
        
        // Create a graph: 1 -> 2 -> 3
        //                  |    |-> 4 -> 5
        //                  |-> 6
        let id1 = SymbolId::new(1).unwrap();
        let id2 = SymbolId::new(2).unwrap();
        let id3 = SymbolId::new(3).unwrap();
        let id4 = SymbolId::new(4).unwrap();
        let id5 = SymbolId::new(5).unwrap();
        let id6 = SymbolId::new(6).unwrap();
        
        graph.add_relationship(id1, id2, Relationship::new(RelationKind::Calls));
        graph.add_relationship(id2, id3, Relationship::new(RelationKind::Calls));
        graph.add_relationship(id2, id4, Relationship::new(RelationKind::Uses));
        graph.add_relationship(id4, id5, Relationship::new(RelationKind::Calls));
        graph.add_relationship(id1, id6, Relationship::new(RelationKind::Uses));
        
        // Get impact of changing id3 (who depends on id3?)
        let impact = graph.get_impact_radius(id3, Some(4));
        assert!(impact.contains(&id2));  // id2 calls id3
        
        // Get impact of changing id2 (who depends on id2?)
        let impact = graph.get_impact_radius(id2, Some(4));
        assert!(impact.contains(&id1));  // id1 calls id2
        
        // Get impact of changing id5 (who depends on id5?)
        let impact = graph.get_impact_radius(id5, Some(4));
        assert!(impact.contains(&id4));  // id4 calls id5
        assert!(impact.contains(&id2));  // id2 uses id4 which calls id5 (depth 2)
        assert!(impact.contains(&id1));  // id1 calls id2 which uses id4 which calls id5 (depth 3)
    }
    
    #[test]
    fn test_get_dependencies_and_dependents() {
        let graph = DependencyGraph::new();
        
        // Create a graph with mixed relationships
        let id1 = SymbolId::new(1).unwrap();
        let id2 = SymbolId::new(2).unwrap();
        let id3 = SymbolId::new(3).unwrap();
        let id4 = SymbolId::new(4).unwrap();
        
        graph.add_relationship(id1, id2, Relationship::new(RelationKind::Calls));
        graph.add_relationship(id1, id3, Relationship::new(RelationKind::Uses));
        graph.add_relationship(id4, id1, Relationship::new(RelationKind::Implements));
        graph.add_relationship(id2, id3, Relationship::new(RelationKind::Uses));
        
        // Test dependencies of id1
        let deps = graph.get_dependencies(id1);
        assert_eq!(deps.get(&RelationKind::Calls).unwrap().len(), 1);
        assert!(deps.get(&RelationKind::Calls).unwrap().contains(&id2));
        assert_eq!(deps.get(&RelationKind::Uses).unwrap().len(), 1);
        assert!(deps.get(&RelationKind::Uses).unwrap().contains(&id3));
        
        // Test dependents of id1
        let dependents = graph.get_dependents(id1);
        assert_eq!(dependents.get(&RelationKind::Implements).unwrap().len(), 1);
        assert!(dependents.get(&RelationKind::Implements).unwrap().contains(&id4));
        
        // Test dependencies of id3 (should have none)
        let deps = graph.get_dependencies(id3);
        assert!(deps.is_empty());
        
        // Test dependents of id3 (should have uses from id1 and id2)
        let dependents = graph.get_dependents(id3);
        assert_eq!(dependents.get(&RelationKind::Uses).unwrap().len(), 2);
        assert!(dependents.get(&RelationKind::Uses).unwrap().contains(&id1));
        assert!(dependents.get(&RelationKind::Uses).unwrap().contains(&id2));
    }
}