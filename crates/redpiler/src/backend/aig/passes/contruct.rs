use std::collections::HashMap;
use std::default;
use std::io::{BufWriter, Write};
use std::fs::File;
use std::ops::Not;
use std::path::Display;
use std::sync::Arc;

use mchprs_blocks::blocks::ComparatorMode;
use mchprs_blocks::BlockPos;
use petgraph::data::Build;
use petgraph::graph::{EdgeIndex, EdgeReference, NodeIndex};
use petgraph::visit::{EdgeRef, IntoEdgesDirected, IntoNodeReferences, NodeIndexable, NodeRef};
use petgraph::{stable_graph, Directed, Direction};
use petgraph::Direction::{Incoming, Outgoing};
use rustc_hash::FxHashMap;

use crate::compile_graph::{self, CompileGraph, LinkType, NodeType};
use crate::{CompilerOptions, TaskMonitor};
use mchprs_world::{TickEntry, World};

use aigrs::networks::petaig::{self, *};

#[derive(Debug, Clone)]
enum Input {
    None,
    Binary(Node),
    Hex([Node; 15])
}

#[derive(Debug, Clone, Copy)]
enum DOutput {
    None,
    Binary(AigLit),
    Hex([AigLit; 15]),
}

#[derive(Debug)]
struct Data {
    default_input: Input,
    side_input: Input,
    output: DOutput,
}

impl Data {
    fn input(output: AigLit) -> Self {
        Self { default_input: Input::None, side_input: Input::None, output: DOutput::Binary(output) }       
    }
    fn output(default_input: Node) -> Self {
        Self { default_input: Input::Binary(default_input), side_input: Input::None, output: DOutput::None }       
    }

    fn unary(default_input: Node, output: AigLit) -> Self {
        Self { default_input: Input::Binary(default_input), side_input: Input::None, output: DOutput::Binary(output) }       
    }

    fn binary(default_input: Node, side_input: Node, output: AigLit) -> Self {
        Self { default_input: Input::Binary(default_input), side_input: Input::Binary(side_input), output: DOutput::Binary(output) }
    }
}

#[derive(Default)]
pub struct ConstructAig;

impl ConstructAig {
    pub fn compile(
        &mut self,
        graph: CompileGraph,
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        monitor: Arc<TaskMonitor>,
    ) -> aigrs::networks::petaig::Aig {
        let inputs: Vec<petaig::Node> = Vec::new();
        let outputs: FxHashMap<petaig::Node, BlockPos> = FxHashMap::default();

        println!("export AIG");
        let mut node_map: FxHashMap::< petgraph::prelude::NodeIndex, Data> = FxHashMap::default();
        let mut aig = Aig::new();

        dbg!();

        for (index, node) in graph.node_references() {
            match node.ty {
                NodeType::Repeater { delay, facing_diode } => {
                    let locking = graph.edges_directed(index, Incoming).any(|edge| edge.weight().ty == compile_graph::LinkType::Side);
                    

                    let default_input = aig.local_input();
                    let mut i0 = default_input.lit();
                    let mut side_input = Input::None;

                    let (latch_start, mut latch_end) = aig.latch();
                    let first_latch = latch_end;
                    
                    for _ in 1..delay {
                        let (next_state, state) = aig.latch();
                        aig.connect_drain(next_state, latch_end);
                        latch_end = state;
                    }
                    
                    let output = latch_end;
                    
                    if locking {
                        let side = aig.local_input();
                        i0 = aig.mux(side.lit(), latch_end, i0);
                        side_input = Input::Binary(side);
                    }

                    let no_pulse_extension = true;


                    if delay <= 1 || no_pulse_extension {
                        aig.connect_drain(latch_start, i0);
                    } else {
                        // s1 = (s & !o) | (i & !(!s & o))
                        let s = first_latch;
                        let o = latch_end;
                        let i = i0;
                        let state_signal = aig.and(s, !o);
                        let accept_input = !aig.and(!s, o);
                        let input_signal = aig.and(i, accept_input);
                        let s1 = aig.or(state_signal, input_signal);

                        aig.connect_drain(latch_start, s1);
                    }
                    
                    node_map.insert(index, Data {
                        default_input: Input::Binary(default_input),
                        side_input,
                        output: DOutput::Binary(output)
                    });
                },
                NodeType::Torch => {
                    let default_input = aig.local_input();

                    let output = !aig.latch2(default_input.lit());
                    
                    node_map.insert(index, Data::unary(default_input, output));
                },
                NodeType::Comparator { mode, far_input, .. } => {
                    let default_inputs = [(); 15].map(|_| aig.local_input());
                    let side_inputs = [(); 15].map(|_| aig.local_input());
                    let mut outputs = [(); 15].map(|_| Vec::<AigLit>::new());

                    let mut start = 0;
                    if let Some(far_input) = far_input {
                        start = 14;

                        if far_input > 0 {
                            // TODO: this might not output to all the lower signal strengths make sure it is correct
                            let far_input = far_input as usize - 1;
                            match mode {
                                ComparatorMode::Compare => {
                                    let signal = !default_inputs[14].lit();
                                    let block = aig.ors(&side_inputs.clone().map(|n| n.lit())[far_input+1..]);
                                    let gate = aig.and(signal, !block);
                                    outputs[far_input].push(gate);
                                },
                                ComparatorMode::Subtract => {
                                    let signal = !default_inputs[14].lit();
                                    for power_on_sides in 0..far_input {
                                        let block = side_inputs[power_on_sides].lit();
                                        let gate = aig.and(signal, !block);
                                        outputs[far_input-power_on_sides].push(gate);
                                    }
                                },
                            }
                        }
                    }
                    for input_strength in start..15 {    
                        match mode {
                            ComparatorMode::Compare => {
                                let signal = default_inputs[input_strength].lit();
                                let block = aig.ors(&side_inputs.clone().map(|n| n.lit())[input_strength+1..]);
                                let gate = aig.and(signal, !block);
                                outputs[input_strength].push(gate);
                            },
                            ComparatorMode::Subtract => {
                                let signal = default_inputs[input_strength].lit();
                                for power_on_sides in 0..input_strength {
                                    let block = side_inputs[power_on_sides].lit();
                                    let gate = aig.and(signal, !block);
                                    outputs[input_strength-power_on_sides].push(gate);
                                }
                            },
                        }
                    }

                    let outputs = outputs.map(|output| {
                        let output = aig.ors(&output);
                        aig.latch2(output)
                    });

                    node_map.insert(index, Data {
                        default_input: Input::Hex(default_inputs),
                        side_input: Input::Hex(side_inputs),
                        output: DOutput::Hex(outputs),
                    });
                },
                NodeType::Lamp => {
                    let default_input = aig.local_input();
                    aig.output(default_input.lit());
                    node_map.insert(index, Data::output(default_input));
                },
                NodeType::Button => {
                    node_map.insert(index, Data::input(aig.input()));
                },
                NodeType::Lever => {
                    node_map.insert(index, Data::input(aig.input()));

                },
                NodeType::PressurePlate => {
                    node_map.insert(index, Data::input(aig.input()));

                },
                NodeType::Trapdoor => {
                    let default_input = aig.local_input();
                    aig.output(default_input.lit());
                    node_map.insert(index, Data::output(default_input));
                },
                NodeType::Wire => {
                    println!("wire?");
                },
                NodeType::Constant => {
                    node_map.insert(index, Data::input(aig.c(node.state.output_strength > 0)));
                },
                NodeType::NoteBlock { instrument, note } => {
                    let default_input = aig.local_input();
                    aig.output(default_input.lit());
                    node_map.insert(index, Data::output(default_input));
                },
            }
        }
        dbg!();

        {
            let g = petgraph::dot::Dot::new(&aig.g);
            let mut f = BufWriter::new(File::create("target/graph0.dot").unwrap());
            writeln!(f, "{:?}", g).unwrap();
        }


        for (&node, data) in node_map.iter() {
            let mut default_inputs = Vec::new();
            let mut side_inputs = Vec::new();

            for edge in graph.edges_directed(node, Incoming) {
                let input_data = node_map.get(&edge.source()).unwrap();

                if edge.weight().ty == LinkType::Default {
                    default_inputs.push((input_data.output, edge.weight().ss))
                } else {
                    side_inputs.push((input_data.output, edge.weight().ss))
                }
            }

            println!("{:?} {:?} {:?}", node, graph[node].ty, data);
            for (data_input, inputs) in [
                (data.default_input.clone(), default_inputs),
                (data.side_input.clone(), side_inputs)
            ] {
                match data_input {
                    Input::None => {
                        assert_eq!(inputs.len(), 0);
                    }
                    Input::Binary(input) => {
                        let inputs: Vec<AigLit> = inputs.iter().copied().map(|(input, ss)| {
                            match input {
                                DOutput::None => aig.f(),
                                DOutput::Binary(lit) => lit,
                                DOutput::Hex(lits) => {
                                    if ss < 15 {
                                        aig.ors(&lits[ss as usize..])
                                    } else {
                                        aig.f()
                                    }
                                }
                            }                            
                        }).collect();
                        let inputs = aig.ors(&inputs);
                        aig.replace_node(input, inputs);
                    },
                    Input::Hex(input) => {
                        let inputs: Vec<[AigLit; 15]> = inputs.iter().copied().map(|(input, ss)| {
                            let mut inputs = [aig.f(); 15];
                            match input {
                                DOutput::None => {},
                                DOutput::Binary(lit) => {
                                    for i in 0..15-ss as usize {
                                        inputs[i] = lit;
                                    }
                                },
                                DOutput::Hex(lits) => {
                                    for i in 0..15-ss as usize {
                                        inputs[i] = lits[i + ss as usize];
                                    }
                                }
                            }
                            inputs                     
                        }).collect();

                        let mut i = 0;
                        let inputs = [(); 15].map(|_| {
                            let mut arr = Vec::with_capacity(inputs.len());
                            for input in inputs.iter() {
                                arr.push(input[i]);
                            }
                            let or = aig.ors(&arr);
                            i += 1;
                            or
                        });
                        for (old, new) in input.into_iter().zip(inputs) {
                            aig.replace_node(old, new);
                        }
                    },
                }
            }
        } 

        for node in aig.g.node_indices() {
            
            if  aig.g[node] == AigNodeTy::Input {
                assert_eq!(aig.g.edges_directed(node, Incoming).count(), 0);
                continue;
            }

            if aig.g[node] == AigNodeTy::And {
                assert_eq!(aig.g.edges_directed(node, Incoming).count(), 2);
                continue;
            }

            assert_eq!(aig.g.edges_directed(node, Incoming).count(), 1)
        }

        dbg!();

        // {
        //     let g = petgraph::dot::Dot::new(&aig.g);
        //     let mut f = BufWriter::new(File::create("target/graph.dot").unwrap());
        //     writeln!(f, "{:?}", g).unwrap();
        // }

        aig.gc();

        dbg!();

        // {
        //     let g = petgraph::dot::Dot::new(&aig.g);
        //     let mut f = BufWriter::new(File::create("target/graphgc.dot").unwrap());
        //     writeln!(f, "{:?}", g).unwrap();
        // }
        // {
        //     let mut f = BufWriter::new(File::create("target/graph.aig").unwrap());
        //     aig.serialize(&mut f).unwrap();
        // }

        


        dbg!("done");
        aig
    }

    fn status_message(&self) -> &'static str {
        "generating And Inverter Graph"
    }

    fn should_run(&self, options: &crate::CompilerOptions) -> bool {
        true
    }
}