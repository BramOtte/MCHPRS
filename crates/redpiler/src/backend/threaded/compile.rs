use crate::backend::threaded::node::Group;
use crate::backend::threaded::TickScheduler;
use crate::compile_graph::{CompileGraph, LinkType, NodeIdx};
use crate::{CompilerOptions, TaskMonitor};
use itertools::Itertools;
use mchprs_blocks::blocks::{Block, Instrument};
use mchprs_blocks::BlockPos;
use mchprs_world::{TickEntry, TickPriority};
use petgraph::graph::NodeIndex;
use petgraph::visit::{EdgeRef, NodeIndexable};
use petgraph::Direction::{self, Incoming, Outgoing};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::sync::Arc;
use std::time::Instant;
use tracing::trace;

use super::node::{ForwardLink, Node, NodeId, NodeInput, NodeType, Nodes, NonMaxU8};
use super::ThreadedBackend;

#[derive(Debug, Default)]
struct FinalGraphStats {
    update_link_count: usize,
    side_link_count: usize,
    default_link_count: usize,
    nodes_bytes: usize,
}

fn compile_node(
    graph: &CompileGraph,
    node_idx: NodeIdx,
    node_index: usize,
    nodes_len: usize,
    nodes_map: &FxHashMap<NodeIdx, usize>,
    node_to_group: &[u32],
    noteblock_info: &mut Vec<(BlockPos, Instrument, u32)>,
    stats: &mut FinalGraphStats,
) -> Node {
    let node = &graph[node_idx];

    let input_group_id = graph.neighbors_directed(node_idx, Direction::Incoming).next()
        .map(|node| {
            let node = *nodes_map.get(&node).unwrap();
            node_to_group[node]
        });

    const MAX_INPUTS: usize = 255;

    let mut default_input_count = 0;
    let mut side_input_count = 0;

    let mut default_inputs = NodeInput { ss_counts: [0; 16] };
    let mut side_inputs = NodeInput { ss_counts: [0; 16] };
    for edge in graph.edges_directed(node_idx, Direction::Incoming) {
        let weight = edge.weight();
        let distance = weight.ss;
        let source = edge.source();
        let ss = graph[source].state.output_strength.saturating_sub(distance);
        match weight.ty {
            LinkType::Default => {
                if default_input_count >= MAX_INPUTS {
                    panic!(
                        "Exceeded the maximum number of default inputs {}",
                        MAX_INPUTS
                    );
                }
                default_input_count += 1;
                default_inputs.ss_counts[ss as usize] += 1;
            }
            LinkType::Side => {
                if side_input_count >= MAX_INPUTS {
                    panic!("Exceeded the maximum number of side inputs {}", MAX_INPUTS);
                }
                side_input_count += 1;
                side_inputs.ss_counts[ss as usize] += 1;
            }
        }
    }
    stats.default_link_count += default_input_count;
    stats.side_link_count += side_input_count;

    use crate::compile_graph::NodeType as CNodeType;
    let updates = if node.ty != CNodeType::Constant {
        graph
            .edges_directed(node_idx, Direction::Outgoing)
            .sorted_by_key(|edge| nodes_map[&edge.target()])
            .into_group_map_by(|edge| std::mem::discriminant(&graph[edge.target()].ty))
            .into_values()
            .flatten()
            .map(|edge| unsafe {
                let idx = edge.target();
                let idx = nodes_map[&idx];
                assert!(idx < nodes_len);
                // Safety: bounds checked
                let target_id = NodeId::from_index(idx);

                let weight = edge.weight();
                ForwardLink::new(target_id, weight.ty == LinkType::Side, weight.ss)
            })
            .collect()
    } else {
        SmallVec::new()
    };
    stats.update_link_count += updates.len();

    let ty = match &node.ty {
        CNodeType::Repeater {
            delay,
            facing_diode,
        } => NodeType::Repeater {
            delay: *delay,
            facing_diode: *facing_diode,
        },
        CNodeType::Torch => NodeType::Torch,
        CNodeType::Comparator {
            mode,
            far_input,
            facing_diode,
        } => NodeType::Comparator {
            mode: *mode,
            far_input: far_input.map(|value| NonMaxU8::new(value).unwrap()),
            facing_diode: *facing_diode,
        },
        CNodeType::Lamp => NodeType::Lamp,
        CNodeType::Button => NodeType::Button,
        CNodeType::Lever => NodeType::Lever,
        CNodeType::PressurePlate => NodeType::PressurePlate,
        CNodeType::Trapdoor => NodeType::Trapdoor,
        CNodeType::Wire => NodeType::Wire,
        CNodeType::Constant => NodeType::Constant,
        CNodeType::NoteBlock { instrument, note } => {
            let noteblock_id = noteblock_info.len().try_into().unwrap();
            noteblock_info.push((node.block.unwrap().0, *instrument, *note));
            NodeType::NoteBlock { noteblock_id }
        }
    };

    Node {
        ty,
        default_inputs,
        side_inputs,
        updates,
        powered: node.state.powered,
        output_power: node.state.output_strength,
        locked: node.state.repeater_locked,
        pending_tick: 255,
        pending_tick_priority: TickPriority::Normal,
        changed: false,
        is_io: node.is_input || node.is_output,
        group_id: node_to_group[node_index],
        input_group_id
    }
}

pub fn compile(
    backend: &mut ThreadedBackend,
    graph: CompileGraph,
    ticks: Vec<TickEntry>,
    options: &CompilerOptions,
    _monitor: Arc<TaskMonitor>,
) {
    println!("compiling...");
    let thread_count = std::thread::available_parallelism().map_or(8, |thread_count| thread_count.get());
    let node_count = graph.node_count();

    let ideal_group_size = graph.node_count() / thread_count;

    let mut visited = vec![false; graph.node_bound()];
    
    let mut node_to_group: Vec<u32> = Vec::with_capacity(node_count);
    let mut nodeids: Vec<NodeIndex> = Vec::with_capacity(node_count);
    let mut groups: Vec<Group> = Vec::with_capacity(node_count / 2);

    let start = Instant::now();

    let mut processed = 0;
    for nodeid in graph.node_indices() {
        if !graph.contains_node(nodeid) {
            continue;
        }
        if visited[nodeid.index()] {
            continue;
        }
        visited[nodeid.index()] = true;
        
        
        let group_index = groups.len() as u32;
        let group_start = node_to_group.len();
        
        nodeids.push(nodeid);
        
        while processed < node_to_group.len() {
            let nodeid = nodeids[processed];
            processed += 1;
            
            for output in graph.neighbors_directed(nodeid, Outgoing) {
                for input in graph.neighbors_directed(output, Incoming) {
                    if visited[input.index()] {
                        continue;
                    }
                    visited[input.index()] = true;
                    
                    nodeids.push(input);
                }
            }
        }

        node_to_group.resize(nodeids.len(), group_index);

        groups.push(Group {
            nodes: group_start as u32..node_to_group.len() as u32,
            scheduler: TickScheduler::default(),
            tick: [false, false],
        });
    }
    backend.groups.groups = groups;
    // let min_size = groups.iter().map(|g| g.nodes.len()).min().unwrap();
    // let max_size = groups.iter().map(|g| g.nodes.len()).max().unwrap();


    // println!("{:?}", start.elapsed());
    // println!("{}", node_count);
    // println!("{} {} {} {} {}", nodeids.len(), groups.len(), nodeids.len() as f32 / groups.len() as f32, min_size, max_size);


    // Create a mapping from compile to backend node indices
    let mut nodes_map = FxHashMap::with_capacity_and_hasher(graph.node_count(), Default::default());
    for (backend_nodeid, compile_nodeid) in nodeids.iter().copied().enumerate() {
        nodes_map.insert(compile_nodeid, backend_nodeid);
    }
    let nodes_len = nodes_map.len();

    // Lower nodes
    let mut stats = FinalGraphStats::default();
    let nodes = nodeids
        .iter()
        .copied()
        .enumerate()
        .map(|(i, idx)| {
            compile_node(
                &graph,
                idx,
                i,
                nodes_len,
                &nodes_map,
                &node_to_group,
                &mut backend.noteblock_info,
                &mut stats,
            )
        })
        .collect();
    stats.nodes_bytes = nodes_len * std::mem::size_of::<Node>();
    trace!("{:#?}", stats);

    backend.blocks = nodeids
        .iter().copied()
        .map(|node| graph[node].block.map(|(pos, id)| (pos, Block::from_id(id))))
        .collect();
    backend.nodes = Nodes::new(nodes);

    // Create a mapping from block pos to backend NodeId
    for i in 0..backend.blocks.len() {
        if let Some((pos, _)) = backend.blocks[i] {
            backend.pos_map.insert(pos, backend.nodes.get(i));
        }
    }

    // Schedule backend ticks
    for entry in ticks {
        if let Some(node) = backend.pos_map.get(&entry.pos) {
            let groupid = node_to_group[node.index()];
            let tick = backend
            .groups
            .schedule_tick(groupid, *node, entry.ticks_left as usize, entry.tick_priority);
            backend.nodes[*node].pending_tick = tick;
            if entry.ticks_left == 1 {
                backend.groups.groups[groupid as usize].tick[0] = true;
            }
        }
    }

    // Dot file output
    if options.export_dot_graph {
        std::fs::write("backend_graph.dot", format!("{}", backend)).unwrap();
    }
}
