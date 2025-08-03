use super::Pass;
use crate::backend::direct::calculate_comparator_output;
use crate::compile_graph::{CompileGraph, LinkType, NodeIdx, NodeType};
use crate::possible_signal_strength::PossibleSS;
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

pub fn calc_possible_inputs(graph: &CompileGraph, idx: NodeIdx) -> (PossibleSS, PossibleSS) {
    let node = &graph[idx];
    let mut def = PossibleSS::EMPTY;
    let mut side = PossibleSS::EMPTY;
    for edge in graph.edges_directed(idx, Direction::Incoming) {
        let source = edge.source();
        let weight = edge.weight();
        let ss = weight.ss;
        let ty = weight.ty;
        let val = graph[source].possible_outputs;
        let val = val.subtract_ss(ss);
        if ty == LinkType::Default {
            def = def.dust_or(val);
        } else {
            side = side.dust_or(val);
        }
    }

    if let NodeType::Comparator {
        far_input: Some(far_input),
        ..
    } = node.ty
    {
        def = if def == PossibleSS::constant(15) {
            PossibleSS::constant(15)
        } else if def.contains(15) {
            PossibleSS::constant(15).with(far_input)
        } else {
            PossibleSS::constant(far_input)
        };
    }

    def.insert_zero_if_empty();
    side.insert_zero_if_empty();
    (def, side)
}

fn calc_possible_outputs(graph: &CompileGraph, idx: NodeIdx) -> PossibleSS {
    let node = &graph[idx];
    let (def, side) = calc_possible_inputs(graph, idx);

    let mut outputs = PossibleSS::constant(node.state.output_strength);
    match node.ty {
        NodeType::Repeater { .. } => {
            if def.contains(0) {
                outputs.insert(0)
            }
            if def.contains_positive() {
                outputs.insert(15)
            }
        }
        NodeType::Torch => {
            if def.contains(0) {
                outputs.insert(15)
            }
            if def.contains_positive() {
                outputs.insert(0);
            }
        }
        NodeType::Comparator { mode, .. } => {
            for def_ss in 0..=15u8 {
                if !def.contains(def_ss) {
                    continue;
                }
                for side_ss in 0..=15u8 {
                    if !side.contains(side_ss) {
                        continue;
                    }
                    let output = calculate_comparator_output(mode, def_ss, side_ss);
                    outputs.insert(output);
                }
            }
        }
        NodeType::Wire => outputs = def,
        NodeType::Lamp
        | NodeType::Button
        | NodeType::Lever
        | NodeType::PressurePlate
        | NodeType::Trapdoor
        | NodeType::Constant
        | NodeType::NoteBlock { .. } => outputs = node.possible_outputs,
    }
    outputs
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
