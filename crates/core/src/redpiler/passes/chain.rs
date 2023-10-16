use std::collections::{HashMap, HashSet};

use petgraph::Direction;
use petgraph::visit::EdgeRef;

use super::Pass;
use crate::redpiler::compile_graph::{CompileGraph, LinkType, CompileNode, NodeState, NodeType, CompileLink, NodeIdx};
use crate::redpiler::{CompilerInput, CompilerOptions};
use crate::world::World;

const MIN_CHAIN_LEN: usize = 1;
const MAX_CHAIN_LEN: usize = 127;
const MIN_DECODER_SIZE: usize = 4;
const MIN_DECODER_INPUTS: usize = 2;
const MAX_DECODER_INPUTS: usize = 15;

pub struct RepeaterChainPass;

fn in_chain(graph: &CompileGraph, id: NodeIdx) -> bool {
    let mut inputs = graph.edges_directed(id, Direction::Incoming);
    let Some(input) = inputs.next() else {
        return false;
    };
    if input.weight().ty == LinkType::Side {
        return false;
    }
    if inputs.next().is_some() {
        return false;
    }
    use NodeType as NT;
    return matches!(graph[id].ty, NT::Repeater(1) | NT::Torch)
}

fn is_digital_output(ty: NodeType) -> bool {
    use NodeType as NT;
    matches!(ty,
        NT::Button | NT::Constant | NT::Lever | NT::PressurePlate | NT::Torch | NT::Repeater(_)
    )
}

struct ChainLink {
    source: NodeIdx,
    target: NodeIdx,
    link: CompileLink,
    delay: usize,
    inverted: bool,
    ends_repeater: bool,
}
impl ChainLink {
    fn create_node(&self, graph: &CompileGraph) -> CompileNode {
        let source_node = &graph[self.source];
        let ty = NodeType::Chain(self.delay as u8, self.inverted, self.ends_repeater);
        CompileNode {
            ty,
            block: source_node.block,
            state: source_node.state.clone(),
            facing_diode: source_node.facing_diode,
            comparator_far_input: source_node.comparator_far_input,
        }
    }
}


fn traverse(graph: &CompileGraph, to_remove: &mut Vec<NodeIdx>, chains: &mut Vec<ChainLink>, input_powered: bool, source: NodeIdx, delay: usize, inverted: bool) -> bool {
    let mut makes_chain = true;
    for target in graph.edges_directed(source, Direction::Outgoing) {
        let weight = target.weight();
        let target = target.target();
        if in_chain(graph, target) && is_powered(&graph[target]) == (input_powered != inverted) && delay < MAX_CHAIN_LEN {
            let delay = delay + 1;
            let inverted = if matches!(graph[target].ty, NodeType::Torch) {!inverted} else {inverted};
            let branch_makes_chain = traverse(graph, to_remove, chains, input_powered, target, delay, inverted);
            makes_chain = makes_chain && branch_makes_chain;
            if branch_makes_chain {
                to_remove.push(target);
            }
        } else if delay >= MIN_CHAIN_LEN {
            // end node
            let link = CompileLink { ty: weight.ty, ss: weight.ss };
            chains.push(ChainLink { source, target, link, delay, inverted, ends_repeater: matches!(graph[target].ty, NodeType::Repeater(_))});
        } else {
            makes_chain = false;
        }
    }
    return makes_chain;
}

fn is_powered(node: &CompileNode) -> bool {
    let lit = node.state.powered;
    match node.ty {
        NodeType::Torch => !lit,
        _ => lit
    }
}

fn traverse_back(graph: &CompileGraph, to_remove: &mut Vec<NodeIdx>, chains: &mut Vec<ChainLink>, node: NodeIdx) {
    
}

impl<W: World> Pass<W> for RepeaterChainPass {
    fn should_run(&self, options: &CompilerOptions) -> bool {
        return false;
    }
    
    // only delay of 1 repeaters supported for now
    fn run_pass(&self, graph: &mut CompileGraph, _: &CompilerOptions, _: &CompilerInput<'_, W>) {
        println!("start detecting chain");
        let mut to_remove = Vec::new();
        let mut chain_count = 0;
        let mut candidates = HashSet::new();
        for id in graph.node_indices().collect::<Vec<_>>() {
            if in_chain(graph, id) {
                continue;
            }
            if !is_digital_output(graph[id].ty) {
                continue;
            }
            let mut chains = Vec::new();
            traverse(graph, &mut to_remove, &mut chains, is_powered(&graph[id]), id, 0, false);            
            chain_count += chains.len();

            chains.sort_by_key(|chain| (chain.delay, chain.inverted, chain.ends_repeater));
            
            let mut chains = chains.drain(..).peekable();

            while let Some(chain) = chains.next() {
                let source_node = &graph[chain.source];
                let ty = NodeType::Chain(chain.delay as u8, chain.inverted, chain.ends_repeater);
                let node = CompileNode {
                    ty,
                    block: source_node.block,
                    state: source_node.state.clone(),
                    facing_diode: source_node.facing_diode,
                    comparator_far_input: source_node.comparator_far_input,
                };
                let chain_node = graph.add_node(node);
                graph.add_edge(id, chain_node, CompileLink::default(0));
                graph.add_edge(chain_node, chain.target, chain.link);
                candidates.insert(chain.target);

                while let Some(other) = chains.peek() {
                    if other.delay != chain.delay || other.inverted != chain.inverted || other.ends_repeater != chain.ends_repeater {
                        break;
                    }
                    
                    graph.add_edge(chain_node, other.target, other.link);
                    candidates.insert(other.target);
                    
                    chains.next();
                }
            }
        }
        let remove_count = to_remove.len();
        let mut edges_removed = 0;
        for node in to_remove {
            edges_removed += graph.edges(node).count();
            graph.remove_node(node);
        }
        println!("replaced {} nodes and {} edges with {} chain links", remove_count, edges_removed, chain_count);

        let mut decoders = HashMap::<Box<[DecoderInput]>, Vec<DecoderOutput>>::new();
        for id in candidates {
            let Some((sources, output)) = is_decoder_output(graph, id) else {
                continue;
            };
            decoders.entry(sources).or_default().push(output);
        }

        'output_loop:
        for (sources, mut outputs) in decoders {
            if outputs.len() < MIN_DECODER_SIZE {
                // println!("decoder too small");
                continue;
            }
            outputs.sort_by_key(|output| u32::MAX - output.pattern);
            
            
            if sources.len() < MIN_DECODER_INPUTS || sources.len() > MAX_DECODER_INPUTS {
                // println!("incorrect number of inputs");
                continue;
            }
            
            let mut start_pattern = 0;
            for source in sources.iter().copied() {
                start_pattern = (start_pattern << 1) | if graph[source.node].state.powered {1} else {0};
            }

            // println!("Decoder {}:\n{:#?} -> {:#?}\n", start_pattern, sources, outputs);
            
            {
                let mut outputs = outputs.iter();
                let mut last_pattern = outputs.next().unwrap().pattern;
                for output in outputs {
                    if output.pattern == last_pattern {
                        // println!("duplicate patterns");
                        continue 'output_loop;
                    }
                    last_pattern = output.pattern;
                }
            }

            

            let decoder = graph.add_node(CompileNode {
                ty: NodeType::Decoder(start_pattern),
                block: None,
                state: NodeState::simple(false),
                facing_diode: false,
                comparator_far_input: None
            });
            for (i, source) in sources.iter().copied().enumerate() {
                let ss = (sources.len() - 1 - i) as u8;
                let mut node = source.node;
                if source.delay > 0 {
                    let chain = graph.add_node(CompileNode {
                        ty: NodeType::Chain(source.delay, false, false),
                        block: None,
                        state: graph[node].state.clone(),
                        // TODO handle these properly
                        facing_diode: false,
                        comparator_far_input: None
                    });
                    graph.add_edge(node, chain, CompileLink::default(0));

                    node = chain;
                }
                graph.add_edge(node, decoder, CompileLink::default(ss));
            }

            let mut next_pattern = 1 << sources.len();
            let sinc = graph.add_node(CompileNode {
                ty: NodeType::Constant,
                block: None,
                state: NodeState::simple(false),
                facing_diode: false,
                comparator_far_input: None,
            });
            for output in outputs {
                for chain in graph.neighbors_directed(output.output, Direction::Incoming).collect::<Vec<_>>() {
                    graph.remove_node(chain);
                }
                for _ in output.pattern+1..next_pattern {
                    graph.add_edge(decoder, sinc, CompileLink::default(0));
                }
                next_pattern = output.pattern;

                if let NodeType::Chain(delay, ..) = &mut graph[output.output].ty {
                    *delay += output.delay;
                    graph.add_edge(decoder, output.output, CompileLink::default(0));
                } else if let ty @ (NodeType::Repeater(1) | NodeType::Torch) = &mut graph[output.output].ty {
                    *ty = NodeType::Chain(output.delay + 1, *ty == NodeType::Torch, false);
                    graph.add_edge(decoder, output.output, CompileLink::default(0));
                } else {
                    let chain = graph.add_node(CompileNode {
                        ty: NodeType::Chain(output.delay, false, false),
                        block: None,
                        state: graph[output.output].state.clone(),
                        // TODO handle these properly
                        facing_diode: false,
                        comparator_far_input: None
                    });
                    graph.add_edge(decoder, chain, CompileLink::default(0));
                    // TODO: set proper edge weight for output
                    graph.add_edge(chain, output.output, CompileLink::default(0));
                }
            }
            for _ in 0..next_pattern {
                graph.add_edge(decoder, sinc, CompileLink::default(0));
            }

            println!("decoder {} -> {}", sources.len(), graph.edges_directed(decoder, Direction::Outgoing).count());
        }

        // replace unmatched chains

        // println!("found {} decoders:\n{:#?}\n", decoders.len(), decoder_count);
        
    }
}

// struct Decoder {
//     sources: Box<NodeIdx>
// }
#[derive(Debug, Clone, Copy)]
struct DecoderOutput {
    delay: u8,
    pattern: u32,
    output: NodeIdx
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DecoderInput {
    node: NodeIdx,
    delay: u8,
}

fn is_decoder_output(graph: &CompileGraph, id: NodeIdx) -> Option<(Box<[DecoderInput]>, DecoderOutput)> {
    let mut sources = Vec::new();
    let mut delay = u8::MAX;
    for chain in graph.neighbors_directed(id, Direction::Incoming) {
        let NodeType::Chain(current_delay, inverted, is_repeater) = graph[chain].ty else {
            return None;
        };
        let Some(source) = graph.neighbors_directed(chain, Direction::Incoming).next() else {
            return None;
        };
        delay = delay.min(current_delay);
        sources.push((source, inverted, current_delay));
    }
    sources.sort_by_key(|entry| entry.0);
    sources.dedup();

    let mut pattern = 0;
    let sources = sources.iter().copied().map(|(source, inverted, current_delay)| {
        pattern = (pattern << 1) | if inverted {1} else {0};
        DecoderInput{node: source, delay: current_delay - delay}
    }).collect::<Box<[_]>>();

    let delay = delay - 1;
    return Some((sources, DecoderOutput { delay, pattern, output: id }));
}