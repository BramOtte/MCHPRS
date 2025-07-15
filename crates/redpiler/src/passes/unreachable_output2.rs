//! # [`UnreachableOutput2`]
//!
//! Removes all links that won't be able to reach any nodes
//! //! This pass requires narrow_outputs.rs to be ran first

use super::Pass;
use crate::compile_graph::{CompileGraph, NodeIdx};
use crate::{CompilerInput, CompilerOptions};
use mchprs_world::World;
use petgraph::visit::NodeIndexable;
use petgraph::Direction;

pub struct UnreachableOutput2;

impl<W: World> Pass<W> for UnreachableOutput2 {
    fn run_pass(&self, graph: &mut CompileGraph, _: &CompilerOptions, _: &CompilerInput<'_, W>) {
        for i in 0..graph.node_bound() {
            let idx = NodeIdx::new(i);
            if !graph.contains_node(idx) {
                continue;
            }

            let max_output = graph[idx].possible_outputs.ilog2() as u8;

            // Now we can go through all the outgoing nodes and remove the ones with a weight that
            // is too high.
            let mut outgoing = graph.neighbors_directed(idx, Direction::Outgoing).detach();
            while let Some((edge_idx, _)) = outgoing.next(graph) {
                if graph[edge_idx].ss >= max_output {
                    graph.remove_edge(edge_idx);
                }
            }
        }
    }

    fn status_message(&self) -> &'static str {
        "Pruning unreachable comparator outputs"
    }
}
