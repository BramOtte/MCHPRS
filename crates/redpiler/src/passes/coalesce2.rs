//! # [`Coalesce2`]
//!
//! This pass combines duplicate logic, aka nodes with the same type state and inputs are merged into a single node.
//! For the best results run constant_fold2 first
//! This pass replaces coalesce.rs

use std::hash::Hash;

use super::Pass;
use crate::compile_graph::{CompileGraph, CompileLink, LinkType, NodeIdx, NodeState, NodeType};
use crate::{CompilerInput, CompilerOptions};
use mchprs_world::World;
use petgraph::visit::{EdgeRef, NodeIndexable};
use petgraph::Direction;
use rustc_hash::FxHashMap;
use tracing::trace;

pub struct Coalesce2;

impl<W: World> Pass<W> for Coalesce2 {
    fn run_pass(&self, graph: &mut CompileGraph, _: &CompilerOptions, _: &CompilerInput<'_, W>) {
        let mut total = 0;
        loop {
            let num_coalesced = run_iteration(graph);
            trace!("Iteration combined {} nodes", num_coalesced);
            if num_coalesced == 0 {
                break;
            }
            total += num_coalesced;
        }
        trace!("Total {}", total);
    }

    fn status_message(&self) -> &'static str {
        "Combining duplicate logic but better"
    }
}

#[derive(PartialEq, Eq, Hash)]
struct Nod {
    default_inputs: Vec<(NodeIdx, u16)>,
    side_inputs: Vec<(NodeIdx, u16)>,
    ty: NodeType,
    state: NodeState,
}
const POSITIVE: u16 = 0xffff << 1;

fn run_iteration(graph: &mut CompileGraph) -> usize {
    let mut num_coalesced = 0;
    let mut nodes = FxHashMap::<Nod, NodeIdx>::default();
    for i in 0..graph.node_bound() {
        let idx = NodeIdx::new(i);
        if !graph.contains_node(idx) {
            continue;
        }
        let node = &graph[idx];
        if node.is_input || node.is_output {
            continue;
        }

        let mut nod = Nod {
            default_inputs: Vec::new(),
            side_inputs: Vec::new(),
            ty: node.ty.clone(),
            state: node.state.clone(),
        };

        let is_bool = node.ty.is_bool();

        for edge in graph.edges_directed(idx, Direction::Incoming) {
            let source = edge.source();
            let source_node = &graph[source];
            let weight = edge.weight();
            let ss_dist = weight.ss;

            let possible_ss = if is_bool {
                source_node.possible_outputs & (POSITIVE << ss_dist)
            } else {
                (source_node.possible_outputs & 1)
                    | ((source_node.possible_outputs & POSITIVE) >> ss_dist)
            };

            let link_type = weight.ty;

            if link_type == LinkType::Default {
                nod.default_inputs.push((source, possible_ss));
            } else {
                nod.side_inputs.push((source, possible_ss));
            }
        }

        nod.default_inputs.sort();
        nod.side_inputs.sort();

        let Some(&same_node) = nodes.get(&nod) else {
            nodes.insert(nod, idx);
            continue;
        };

        coalesce(graph, idx, same_node, 0);

        num_coalesced += 1;
    }
    num_coalesced
}

pub fn coalesce(graph: &mut CompileGraph, node: NodeIdx, into: NodeIdx, extra_distance: u8) {
    if node == into {
        return;
    }

    let mut neighbors = graph.neighbors_directed(node, Direction::Outgoing).detach();
    while let Some((edge, dest)) = neighbors.next(graph) {
        let CompileLink { ty, ss } = graph[edge];
        let ss = ss + extra_distance;
        if ss < 15 {
            graph.add_edge(into, dest, CompileLink::new(ty, ss));
        }
    }
    graph.remove_node(node);
}
