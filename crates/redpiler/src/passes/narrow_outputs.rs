use super::Pass;
use crate::backend::direct::calculate_comparator_output;
use crate::compile_graph::{CompileGraph, LinkType, NodeIdx, NodeType};
use crate::{CompilerInput, CompilerOptions};
use mchprs_world::World;
use petgraph::visit::{EdgeRef, NodeIndexable};
use petgraph::Direction;

pub struct NarrowOutputs;

impl<W: World> Pass<W> for NarrowOutputs {
    fn run_pass(&self, graph: &mut CompileGraph, _: &CompilerOptions, _: &CompilerInput<'_, W>) {
        loop {
            let num_updated = narrow_iteration(graph);
            if num_updated == 0 {
                break;
            }
        }
    }

    fn should_run(&self, o: &CompilerOptions) -> bool {
        o.optimize
    }

    fn status_message(&self) -> &'static str {
        "Narrowing possible output signal strengths"
    }
}

const POSITIVE: u16 = 0xffff << 1;

#[inline(always)]
fn remove_ss(values: u16, distance: u8) -> u16 {
    (values & 1) | (values >> distance)
}

#[inline(always)]
fn or_possible(a: u16, b: u16) -> u16 {
    let a_lsb = a & (u16::MAX - a);
    let b_lsb = b & (u16::MAX - b);
    (a | b) & a_lsb.wrapping_sub(1) & b_lsb.wrapping_sub(1)
}

#[inline(always)]
pub fn calc_possible_inputs(graph: &CompileGraph, idx: NodeIdx) -> (u16, u16) {
    let node = &graph[idx];
    let mut def = 0;
    let mut side = 0;
    for edge in graph.edges_directed(idx, Direction::Incoming) {
        let source = edge.source();
        let weight = edge.weight();
        let ss = weight.ss;
        let ty = weight.ty;
        let val = graph[source].possible_outputs;
        let val = remove_ss(val, ss);
        if ty == LinkType::Default {
            def = or_possible(def, val);
        } else {
            side = or_possible(side, val);
        }
    }

    if let NodeType::Comparator {
        far_input: Some(far_input),
        ..
    } = node.ty
    {
        def = if far_input == 0 {
            def
        } else if def & (1 << 15) != 0 {
            (1 << 15) | (1 << far_input)
        } else {
            1 << far_input
        }
    }

    if def == 0 {
        def = 1
    }
    if side == 0 {
        side = 1;
    }
    (def, side)
}

#[inline(always)]
fn calc_possible_outputs(graph: &CompileGraph, idx: NodeIdx) -> u16 {
    let node = &graph[idx];
    let (def, side) = calc_possible_inputs(graph, idx);

    let current_output = 1u16 << node.state.output_strength;
    current_output
        | match node.ty {
            NodeType::Repeater { .. } => {
                (if (def & 1) != 0 { 1 } else { 0 })
                    | (if (def & POSITIVE) != 0 { 1 << 15 } else { 0 })
            }
            NodeType::Torch => {
                (if (def & 1) != 0 { 1 << 15 } else { 0 })
                    | (if (def & POSITIVE) != 0 { 1 } else { 0 })
            }
            NodeType::Comparator { mode, .. } => {
                let mut from_inputs = 0;
                for def_ss in 0..=15u8 {
                    let ii = 1 << def_ss;
                    if (ii & def) == 0 {
                        continue;
                    }
                    for side_ss in 0..=15u8 {
                        let jj = 1 << side_ss;
                        if (jj & side) == 0 {
                            continue;
                        }
                        let output = calculate_comparator_output(mode, def_ss, side_ss);
                        from_inputs |= 1 << output;
                    }
                }
                from_inputs
            }
            NodeType::Wire => def,
            NodeType::Lamp
            | NodeType::Button
            | NodeType::Lever
            | NodeType::PressurePlate
            | NodeType::Trapdoor
            | NodeType::Constant
            | NodeType::NoteBlock { .. } => node.possible_outputs,
        }
}

fn narrow_iteration(graph: &mut CompileGraph) -> usize {
    let mut updated = 0;
    for i in 0..graph.node_bound() {
        let idx = NodeIdx::new(i);
        if !graph.contains_node(idx) {
            continue;
        }
        let new_possible_outputs = calc_possible_outputs(graph, idx);

        let node = &mut graph[idx];
        if new_possible_outputs != node.possible_outputs {
            updated += 1;
        }
        node.possible_outputs = new_possible_outputs;
    }
    return updated;
}
