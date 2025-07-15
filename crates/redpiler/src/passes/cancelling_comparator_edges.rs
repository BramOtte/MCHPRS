use super::Pass;
use crate::compile_graph::{CompileGraph, LinkType, NodeIdx, NodeType};
use crate::{CompilerInput, CompilerOptions};
use mchprs_blocks::blocks::ComparatorMode;
use mchprs_world::World;
use petgraph::Direction;
use rustc_hash::FxHashMap;
use tracing::trace;

pub struct CancellingComparatorEdges;

impl<W: World> Pass<W> for CancellingComparatorEdges {
    fn run_pass(&self, graph: &mut CompileGraph, _: &CompilerOptions, _: &CompilerInput<'_, W>) {
        run_comp(graph);
    }

    fn status_message(&self) -> &'static str {
        "Clamping weights"
    }
}

fn run_comp(graph: &mut CompileGraph) {
    let mut links_removed = 0;
    for i in 0..graph.node_count() {
        let idx = NodeIdx::new(i);
        if !graph.contains_node(idx) {
            continue;
        }
        let node = &graph[idx];

        match node.ty {
            // TODO: handle far_input
            NodeType::Comparator {
                mode, far_input, ..
            } => {
                let mut map = FxHashMap::default();
                let mut walk_incomming: petgraph::stable_graph::WalkNeighbors<u32> =
                    graph.neighbors_directed(idx, Direction::Incoming).detach();
                while let Some(edge_idx) = walk_incomming.next_edge(graph) {
                    let source = graph.edge_endpoints(edge_idx).unwrap().0;
                    if !graph.contains_node(source) {
                        continue;
                    }

                    let weight = &graph[edge_idx];
                    let ss = weight.ss;
                    let ty = weight.ty;
                    let Some((other, other_ty, other_ss)) = map.insert(source, (edge_idx, ty, ss))
                    else {
                        continue;
                    };

                    if ty != other_ty {
                        let (def_ss, def, side_ss, side) = if ty == LinkType::Default {
                            (ss, edge_idx, other_ss, other)
                        } else {
                            (other_ss, other, ss, edge_idx)
                        };

                        if mode == ComparatorMode::Compare {
                            if def_ss > side_ss {
                                graph.remove_edge(def);
                                println!("{def_ss} > {side_ss}");
                                links_removed += 1;
                            }
                            if def_ss <= side_ss {
                                graph.remove_edge(side);
                                links_removed += 1;
                            }
                        } else {
                            if def_ss >= side_ss {
                                graph.remove_edge(def);
                                links_removed += 1;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    trace!("Removed {} links", links_removed);
}
