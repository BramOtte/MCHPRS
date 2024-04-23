//! # [`ApplySeparators`]
//!
//! Apply and replace all separator nodes with input and a corresponding output node

use super::Pass;
use crate::redpiler::compile_graph::{Annotations, CompileGraph, CompileNode, NodeIdx, NodeType};
use crate::redpiler::{BackendVariant, CompilerInput, CompilerOptions};
use crate::world::World;
use petgraph::visit::NodeIndexable;
use petgraph::Direction;

pub struct ApplySeparators;

impl<W: World> Pass<W> for ApplySeparators {
    fn run_pass(&self, graph: &mut CompileGraph, _: &CompilerOptions, _: &CompilerInput<'_, W>) {
        for i in 0..graph.node_bound() {
            let idx = NodeIdx::new(i);
            if !graph.contains_node(idx) {
                continue;
            }

            let node = &graph[idx];
            if let Some(delay) = node.annotations.separate {
                let state = node.state.clone();

                let input_node = graph.add_node(CompileNode {
                    ty: NodeType::ExternalInput,
                    block: None,
                    state: state.clone(),
                    is_input: true,
                    is_output: false,
                    annotations: Annotations::default(),
                });
                let output_node = graph.add_node(CompileNode {
                    ty: NodeType::ExternalOutput {
                        target_idx: input_node,
                        delay,
                    },
                    block: None,
                    state: state.clone(),
                    is_input: false,
                    is_output: true,
                    annotations: Annotations::default(),
                });

                let mut incoming_edges =
                    graph.neighbors_directed(idx, Direction::Incoming).detach();
                while let Some(edge_idx) = incoming_edges.next_edge(graph) {
                    let source = graph.edge_endpoints(edge_idx).unwrap().0;
                    let edge = graph.remove_edge(edge_idx).unwrap();
                    graph.add_edge(source, output_node, edge);
                }

                let mut outgoing_edges =
                    graph.neighbors_directed(idx, Direction::Outgoing).detach();
                while let Some(edge_idx) = outgoing_edges.next_edge(graph) {
                    let target = graph.edge_endpoints(edge_idx).unwrap().1;
                    let edge = graph.remove_edge(edge_idx).unwrap();
                    graph.add_edge(input_node, target, edge);
                }

                graph.remove_node(idx);
            }
        }
    }

    fn should_run(&self, options: &CompilerOptions) -> bool {
        options.backend_variant == BackendVariant::Threading
    }
    
    fn status_message(&self) -> &'static str {
        "applying separators"
    }
}
