//! # [`ConstantFold2`]
//!
//! This pass replaces nodes of constant output with a constant node
//! This pass requires narrow_outputs.rs to be ran first
//! This pass replaces constant_coalesce.rs and constant_fold.rs

use super::Pass;
use crate::compile_graph::{CompileGraph, CompileNode, NodeIdx, NodeState, NodeType};
use crate::passes::coalesce2::coalesce;
use crate::possible_signal_strength::PossibleSS;
use crate::{CompilerInput, CompilerOptions};
use mchprs_world::World;
use petgraph::visit::NodeIndexable;

pub struct ConstantFold2;

impl<W: World> Pass<W> for ConstantFold2 {
    fn run_pass(&self, graph: &mut CompileGraph, _: &CompilerOptions, _: &CompilerInput<'_, W>) {
        let constant = graph.add_node(CompileNode {
            ty: NodeType::Constant,
            block: None,
            state: NodeState::ss(15),
            is_input: false,
            is_output: false,
            annotations: Default::default(),
            possible_outputs: PossibleSS::constant(15),
        });

        for i in 0..graph.node_bound() {
            let idx = NodeIdx::new(i);
            if idx == constant || !graph.contains_node(idx) {
                continue;
            }
            let node = &graph[idx];

            if !node.is_removable() {
                continue;
            }

            let Some(ss) = node.possible_outputs.get_constant() else {
                continue;
            };

            coalesce(graph, idx, constant, 15 - ss);
        }
    }

    fn status_message(&self) -> &'static str {
        "Replacing nodes of constant output with constants"
    }
}
