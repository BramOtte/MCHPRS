use petgraph::Direction;
use petgraph::visit::NodeIndexable;

use super::Pass;
use crate::redpiler::compile_graph::{CompileGraph, NodeIdx};
use crate::redpiler::{CompilerInput, CompilerOptions};
use crate::world::World;

pub struct IoOnly;

impl<W: World> Pass<W> for IoOnly {
    fn run_pass(&self, graph: &mut CompileGraph, _: &CompilerOptions, _: &CompilerInput<'_, W>) {
        let mut visited = vec![false; graph.node_bound()];
        let mut queue: Vec<NodeIdx> = Vec::with_capacity(graph.node_count());

        // Find all output nodes
        for node in graph.node_indices() {
            if !graph[node].ty.is_output(){
                continue;
            }
            visited[node.index()] = true;
            queue.push(node);
        }
        
        // Do BFS to find all the nodes that can effect an output node
        let mut next_queue: Vec<NodeIdx> = Vec::with_capacity(graph.node_count());

        while queue.len() > 0 {
            for node in queue.drain(..) {
                for neighbor in graph.neighbors_directed(node, Direction::Incoming) {
                    if visited[neighbor.index()] {
                        continue;
                    }
                    visited[neighbor.index()] = true;
                    next_queue.push(neighbor);
                }
            }

            std::mem::swap(&mut queue, &mut next_queue);
        }

        // Retain only the nodes that can effect an output node
        graph.retain_nodes(|_g, node| visited[node.index()]);
    }

    fn should_run(&self, options: &CompilerOptions) -> bool {
        options.io_only && options.optimize
    }
}
