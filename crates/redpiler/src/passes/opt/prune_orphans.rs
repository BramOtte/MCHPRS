//! # [`PruneOrphans`]
//!
//! This pass removes any nodes in the graph that aren't transitively connected to an output
//! redstone component by using Depth-First-Search.

use crate::compile_graph::{CompileGraph, Direction};
use crate::passes::{AnalysisInfos, Pass};
use crate::{CompilerInput, CompilerOptions};
use itertools::Itertools;
use mchprs_blocks::BlockPos;
use mchprs_world::World;
use tracing::warn;

pub struct PruneOrphans;

impl<W: World> Pass<W> for PruneOrphans {
    fn run_pass(
        &self,
        graph: &mut CompileGraph,
        _: &CompilerOptions,
        _: &CompilerInput<'_, W>,
        _: &mut AnalysisInfos,
    ) {
        // We start searching from output nodes
        let mut worklist = graph
            .node_indices()
            .filter(|&idx| graph[idx].is_output)
            .collect_vec();

        let mut visited = vec![false; graph.node_bound()];

        // Visit initial nodes
        for &idx in &worklist {
            visited[idx.index()] = true;
        }

        while let Some(idx) = worklist.pop() {
            for incoming in graph.neighbors(idx, Direction::Incoming) {
                if !visited[incoming.index()] {
                    visited[incoming.index()] = true;
                    worklist.push(incoming);
                }
            }
        }

        let mut useless_inputs: Vec<BlockPos> = Vec::new();

        graph.retain_nodes(|g, idx| {
            let visible = visited[idx.index()];
            let is_input = g[idx].is_input;

            if !visible && is_input {
                useless_inputs.extend(g[idx].block.iter().map(|(pos, _)| pos));
            }

            // Retain inputs so they can be updated when used
            visible || is_input
        });

        if useless_inputs.len() > 0 {
            // TODO: send warning to player instead of logs
            warn!(
                "The following inputs are not connected to any outputs: {:#?}",
                useless_inputs
            );
        }
    }

    fn status_message(&self) -> &'static str {
        "Pruning orphans"
    }

    fn driver_key(&self) -> &'static str {
        "prune-orphans"
    }
}
