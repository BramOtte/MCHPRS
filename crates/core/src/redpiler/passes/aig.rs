use std::default;
use std::io::{Write};
use std::fs::File;
use std::ops::Not;
use std::path::Display;

use petgraph::data::Build;
use petgraph::graph::{EdgeIndex, EdgeReference, NodeIndex};
use petgraph::visit::{EdgeRef, IntoEdgesDirected, IntoNodeReferences, NodeIndexable, NodeRef};
use petgraph::{stable_graph, Directed, Direction};
use petgraph::Direction::{Incoming, Outgoing};
use rustc_hash::FxHashMap;

use crate::redpiler::compile_graph::{self, LinkType, NodeType};
use crate::world::World;

use super::{Pass};

use aigrs::networks::petaig::*;

#[derive(Debug, Clone)]
enum Input {
    None,
    Binary(Node),
    Hex([Node; 4])
}

#[derive(Debug, Clone, Copy)]
enum DOutput {
    None,
    Binary(AigLit),
    Hex([AigLit; 4]),
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

pub struct ExportAig;

impl<W: World> Pass<W> for ExportAig {
    fn run_pass(
        &self,
        graph: &mut crate::redpiler::compile_graph::CompileGraph,
        options: &crate::redpiler::CompilerOptions,
        input: &crate::redpiler::CompilerInput<'_, W>,
    ) {
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

                    
                    if delay <= 1 {
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
                NodeType::Comparator { mode, far_input, facing_diode } => {
                    // let default_input = aig.local_input();
                    // let side_input = aig.local_input();

                    // let output = aig.and(default_input.lit(), !side_input.lit());
                    
                    // node_map.insert(index, Data {
                    //     default_input: Input::Binary(default_input),
                    //     side_input: Input::Binary(side_input),
                    //     output: DOutput::Binary(output),
                    // });

                    let default_inputs = [(); 4].map(|_| aig.local_input());
                    let side_inputs = [(); 4].map(|_| aig.local_input());

                    let (outputs, carry) = aigrs::components::const_sub(&mut aig,
                        [0, 1, 2, 3].map(|i| default_inputs[i].lit()),
                        [0, 1, 2, 3].map(|i| side_inputs[i].lit()),
                    );
                    
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
            let mut f = File::create("target/graph0.dot").unwrap();
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

            fn stuffs() {

            }

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
                                    let ss = aigrs::components::const_word(&mut aig, ss as usize);
                                    let (_, on) = aigrs::components::const_sub(&mut aig, lits, ss);
                                    on
                                }
                            }                            
                        }).collect();
                        let inputs = aig.ors(&inputs);
                        aig.replace_input(input, inputs);
                    },
                    Input::Hex(input) => {
                        let inputs: Vec<[AigLit; 4]> = inputs.iter().copied().map(|(input, ss)| {
                            match input {
                                DOutput::None => aigrs::components::const_word(&mut aig, 0),
                                DOutput::Binary(lit) => {
                                    let ss = aigrs::components::const_word(&mut aig, 15 - ss as usize);
                                    let zero = aigrs::components::const_word(&mut aig, 0);
                                    aigrs::components::const_mux(&mut aig, lit, ss, zero)
                                },
                                DOutput::Hex(lits) => lits
                            }                            
                        }).collect();

                        let inputs = aigrs::components::max_tree(&mut aig, &inputs);
                        for (old, new) in input.into_iter().zip(inputs) {
                            aig.replace_input(old, new);
                        }
                    },
                }
            }
        } 

        for node in aig.g.node_indices() {
            println!("{:?} {:?}", node, aig.g[node]);
            if  aig.g[node] == AigNodeTy::Input {
                continue;
            }

            if aig.g[node] == AigNodeTy::And {
                // assert_eq!(aig.g.edges_directed(node, Incoming).count(), 2);
                continue;
            }

            // assert_eq!(aig.g.edges_directed(node, Incoming).count(), 1)
        }

        // 'outer:
        // loop {
        //     for node in aig.g.node_indices() {
        //         if  aig.g[node] != AigNodeTy::And {
        //             continue;
        //         }
        //         let mut input_latches = aig.g.edges_directed(node, Incoming);
        //         let input_latches = [input_latches.next().unwrap(), input_latches.next().unwrap()];

        //         if !input_latches.iter().all(|latch| aig.g[latch.source()] == AigNodeTy::Latch) {
        //             continue;
        //         }

                
        //         let inputs = input_latches.map(|latch| {
        //             let input = aig.g.edges_directed(latch.source(), Incoming).next().unwrap();
        //             (input.source(), latch.weight() ^ input.weight())
        //         });

        //         let outputs = aig.g.edges_directed(node, Outgoing)
        //             .map(|output| output.id())
        //             .collect::<Vec<_>>();

        //         let input_latches = input_latches.map(|latch| latch.id());
                
                
        //         for (input, inverted) in inputs {
        //             aig.edge(input, node, inverted);
        //         }
                
        //         let latch = aig.latch();
                
        //         aig.edge(node, latch, false);

        //         for output in outputs.iter().copied() {
        //             let (_, drain) = aig.g.edge_endpoints(output).unwrap();
        //             let inverted = aig.g[output];
        //             aig.edge(latch, drain, inverted);
        //         }

        //         for output in outputs {
        //             aig.g.remove_edge(output);
        //         }

        //         for latch in input_latches {
        //             aig.g.remove_edge(latch);
        //         }
                
        //         continue 'outer
        //     }

        //     break;
        // }

        // 'outer:
        // loop {
        //     for node in aig.g.node_indices() {
        //         match aig.g[node] {
        //             AigNodeTy::And | AigNodeTy::Latch => {
        //                 if aig.g.edges_directed(node, Outgoing).next().is_some() {
        //                     continue;
        //                 }
        //                 aig.g.remove_node(node);
        //                 continue 'outer;
        //             },
        //             _ => {}
        //         }
        //     }
        //     break;
        // }

        dbg!();

        {
            let g = petgraph::dot::Dot::new(&aig.g);
            let mut f = File::create("target/graph.dot").unwrap();
            writeln!(f, "{:?}", g).unwrap();
        }

        aig.gc();

        {
            let g = petgraph::dot::Dot::new(&aig.g);
            let mut f = File::create("target/graphgc.dot").unwrap();
            writeln!(f, "{:?}", g).unwrap();
        }
        {
            let mut f = File::create("target/graph.aig").unwrap();
            aig.serialize(&mut f).unwrap();
        }


        dbg!("done");
    }

    fn status_message(&self) -> &'static str {
        "generating And Inverter Graph"
    }

    fn should_run(&self, options: &crate::redpiler::CompilerOptions) -> bool {
        true
    }
}