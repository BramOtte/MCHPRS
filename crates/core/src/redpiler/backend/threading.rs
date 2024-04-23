use super::common::{calculate_comparator_output, ForwardLink, NodeId, NodeIdWithData, NonMaxU8};
use super::JITBackend;
use crate::redpiler::compile_graph::{self, CompileGraph, LinkType, NodeIdx};
use crate::redpiler::{block_powered_mut, CompilerOptions, TaskMonitor};
use crate::redstone::bool_to_ss;
use crate::world::World;
use itertools::Itertools;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ComparatorMode};
use mchprs_blocks::BlockPos;
use mchprs_world::{TickEntry, TickPriority};
use nodes::Nodes;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use realtime_channel::{Receiver, RingBuffer, Sender};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::cmp::Reverse;
use std::fmt::Debug;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::{array, fmt, mem};
use thread_priority::{ThreadBuilder, ThreadPriority};
use tracing::{debug, trace, warn};

const WORK_CHANNEL_SIZE: usize = 1 << 10;
const UPDATES_CHANNEL_SIZE: usize = 1 << 20;
const CHANGES_CHANNEL_SIZE: usize = 1 << 20;
const FLUSH_OFFSET: usize = 2;

#[derive(Debug, Default)]
struct FinalGraphStats {
    update_link_count: usize,
    side_link_count: usize,
    default_link_count: usize,
    nodes_bytes: usize,
}

mod nodes {
    use crate::redpiler::backend::common::NodeId;

    use super::Node;
    use std::ops::{Index, IndexMut};

    // This is Pretty Bad:tm: because one can create a NodeId using another instance of Nodes,
    // but at least some type system protection is better than none.
    #[derive(Default, Debug)]
    pub struct Nodes {
        nodes: Box<[Node]>,
    }

    impl Nodes {
        pub fn new(nodes: Box<[Node]>) -> Nodes {
            Nodes { nodes }
        }

        pub fn inner(&self) -> &[Node] {
            &self.nodes
        }

        pub fn into_inner(self) -> Box<[Node]> {
            self.nodes
        }

        pub fn len(&self) -> usize {
            self.nodes.len()
        }
    }

    impl Index<NodeId> for Nodes {
        type Output = Node;

        // The index here MUST have been created by this instance, otherwise scary things will happen !
        fn index(&self, index: NodeId) -> &Self::Output {
            unsafe { self.nodes.get_unchecked(index.index()) }
        }
    }

    impl IndexMut<NodeId> for Nodes {
        fn index_mut(&mut self, index: NodeId) -> &mut Self::Output {
            unsafe { self.nodes.get_unchecked_mut(index.index()) }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum NodeType {
    Repeater {
        delay: u8,
        facing_diode: bool,
    },
    Torch,
    Comparator {
        mode: ComparatorMode,
        far_input: Option<NonMaxU8>,
        facing_diode: bool,
    },
    Lamp,
    Button,
    Lever,
    PressurePlate,
    Trapdoor,
    Wire,
    Constant,
    ExternalInput,
    ExternalOutput {
        output_index: u8,
        target_id: NodeId,
    },
    ComparatorLine {
        delay: u8,
    },
}

// struct is 128 bytes to fit nicely into cachelines
// which are usualy 64 bytes, it can vary but is almost always a power of 2
// Note: Removing the alignment and reducing the updates smallvec to 8 seems to have no performance impact
#[derive(Debug, Clone)]
#[repr(align(128))]
pub struct Node {
    ty: NodeType,
    default_inputs: [u8; 16],
    side_inputs: [u8; 16],
    default_power: u8,
    side_power: u8,
    updates: SmallVec<[ForwardLink; 18]>,
    is_io: bool,

    /// Powered or lit
    powered: bool,
    output_power: u8,
    changed: bool,
    pending_tick: bool,
}

impl Node {
    fn from_compile_node(
        graph: &CompileGraph,
        node_idx: NodeIdx,
        nodes_map: &FxHashMap<NodeIdx, (WorkerId, NodeId)>,
        output_indicies: &mut Vec<(WorkerId, u32)>,
        stats: &mut FinalGraphStats,
    ) -> Self {
        let node = &graph[node_idx];

        const MAX_INPUTS: usize = 255;

        let mut default_input_count = 0;
        let mut side_input_count = 0;

        let mut default_inputs = [0; 16];
        let mut side_inputs = [0; 16];
        let mut default_input = 0;
        let mut side_input = 0;
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
                    if node.is_analog() {
                        default_inputs[ss as usize] += 1;
                    } else if ss > 0 {
                        default_input += 1;
                    }
                }
                LinkType::Side => {
                    if side_input_count >= MAX_INPUTS {
                        panic!("Exceeded the maximum number of side inputs {}", MAX_INPUTS);
                    }
                    side_input_count += 1;

                    if node.is_analog() {
                        side_inputs[ss as usize] += 1;
                    } else if ss > 0 {
                        side_input += 1;
                    }
                }
            }
        }
        stats.default_link_count += default_input_count;
        stats.side_link_count += side_input_count;

        use crate::redpiler::compile_graph::NodeType as CNodeType;
        let updates = if node.ty != CNodeType::Constant {
            graph
                .edges_directed(node_idx, Direction::Outgoing)
                .map(|edge| {
                    let idx = edge.target();
                    let (_, node_id) = nodes_map[&idx];

                    let weight = edge.weight();
                    ForwardLink::new(node_id, weight.ty == LinkType::Side, weight.ss)
                })
                .collect()
        } else {
            SmallVec::new()
        };
        stats.update_link_count += updates.len();

        if node.is_analog() {
            default_input = last_index_positive(&default_inputs);
            side_input = last_index_positive(&side_inputs);
        }

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
                far_input: far_input.map(|x| NonMaxU8::new(x).unwrap()),
                facing_diode: *facing_diode,
            },
            CNodeType::Lamp => NodeType::Lamp,
            CNodeType::Button => NodeType::Button,
            CNodeType::Lever => NodeType::Lever,
            CNodeType::PressurePlate => NodeType::PressurePlate,
            CNodeType::Trapdoor => NodeType::Trapdoor,
            CNodeType::Wire => NodeType::Wire,
            CNodeType::Constant => NodeType::Constant,
            CNodeType::ExternalInput => NodeType::ExternalInput,
            CNodeType::ExternalOutput { target_idx, delay } => {
                let (worker_id, target_id) = nodes_map[target_idx];
                let output_index = output_indicies
                    .iter()
                    .position(|(id, _)| *id == worker_id)
                    .unwrap_or_else(|| {
                        output_indicies.push((worker_id, *delay));
                        output_indicies.len() - 1
                    })
                    .try_into()
                    .unwrap();
                assert_eq!(output_indicies[output_index as usize].1, *delay);
                NodeType::ExternalOutput {
                    output_index,
                    target_id,
                }
            }
            //CNodeType::ComparatorLine { .. } => unreachable!(),
            CNodeType::ComparatorLine { states } => NodeType::ComparatorLine {
                delay: {
                    let delay = states.len();
                    assert!(delay >= 2 && delay < TickScheduler::NUM_QUEUES);
                    delay.try_into().unwrap()
                },
            },
            CNodeType::NoteBlock { instrument, note } => todo!(),
        };

        Node {
            ty,
            default_inputs,
            side_inputs,
            updates,
            powered: node.state.powered,
            output_power: node.state.output_strength,
            pending_tick: false,
            changed: false,
            is_io: node.is_input || node.is_output,
            default_power: default_input,
            side_power: side_input,
        }
    }

    fn is_analog(&self) -> bool {
        matches!(
            self.ty,
            NodeType::Comparator { .. }
                | NodeType::Wire
                | NodeType::ExternalOutput { .. }
                | NodeType::ComparatorLine { .. }
        )
    }
}

#[derive(Clone)]
struct Queues([Vec<NodeIdWithData>; TickScheduler::NUM_PRIORITIES]);

impl Queues {
    unsafe fn new(capacity: usize) -> Self {
        Self(array::from_fn(|_| Vec::with_capacity(capacity)))
    }
}

struct TickScheduler {
    queues_deque: [Queues; Self::NUM_QUEUES],
    pos: usize,
}

impl TickScheduler {
    const NUM_PRIORITIES: usize = 4;
    const NUM_QUEUES: usize = 64;

    unsafe fn new(capacity: usize) -> Self {
        Self {
            queues_deque: std::array::from_fn(|_| Queues::new(capacity)),
            pos: 0,
        }
    }

    fn schedule_tick(&mut self, node_data: NodeIdWithData, delay: usize, priority: TickPriority) {
        debug_assert!(delay < Self::NUM_QUEUES);
        let queues_index = (self.pos + delay) % Self::NUM_QUEUES;
        let queue = &mut self.queues_deque[queues_index].0[priority as usize];
        debug_assert!(queue.len() < queue.capacity());
        unsafe {
            queue.as_mut_ptr().add(queue.len()).write(node_data);
            queue.set_len(queue.len() + 1);
        }
    }

    fn queues_this_tick(&mut self) -> Queues {
        self.pos = (self.pos + 1) % Self::NUM_QUEUES;
        let dummy = Queues([vec![], vec![], vec![], vec![]]);
        mem::replace(&mut self.queues_deque[self.pos], dummy)
    }

    fn end_tick(&mut self, queues: Queues) {
        let dummy = mem::replace(&mut self.queues_deque[self.pos % Self::NUM_QUEUES], queues);
        mem::forget(dummy);
    }

    fn reset<W: World>(&mut self, world: &mut W, blocks: &[Option<(BlockPos, Block)>]) {
        for (idx, queues) in self.queues_deque.iter().enumerate() {
            let delay = if self.pos >= idx {
                idx + Self::NUM_QUEUES
            } else {
                idx
            } - self.pos;
            for (entries, priority) in queues.0.iter().zip(Self::priorities()) {
                for node in entries {
                    let Some((pos, _)) = blocks[node.node().index()] else {
                        warn!("Cannot schedule tick for node {:?} because block information is missing", node);
                        continue;
                    };
                    world.schedule_tick(pos, delay as u32, priority);
                }
            }
        }
        for queues in self.queues_deque.iter_mut() {
            for queue in queues.0.iter_mut() {
                queue.clear();
            }
        }
    }

    fn priorities() -> [TickPriority; Self::NUM_PRIORITIES] {
        [
            TickPriority::Highest,
            TickPriority::Higher,
            TickPriority::High,
            TickPriority::Normal,
        ]
    }
}

#[derive(Default)]
pub struct ThreadingBackend {
    handles: Vec<JoinHandle<Worker>>,
    change_receivers: Vec<Receiver<ChangeMessage>>,
    work_senders: Vec<Sender<WorkerMessage>>,
    pos_map: FxHashMap<BlockPos, (WorkerId, NodeId)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkerId(u16);

struct Worker {
    id: WorkerId,
    inputs: Vec<Receiver<UpdateMessage>>,
    outputs: Vec<Sender<UpdateMessage>>,
    work_receiver: Receiver<WorkerMessage>,
    change_sender: Sender<ChangeMessage>,
    nodes: Nodes,
    scheduler: TickScheduler,
    blocks: Vec<Option<(BlockPos, Block)>>,
    last_io_index: usize,
}

enum UpdateMessage {
    Update {
        node_id: NodeId,
        signal_strength: u8,
    },
    UpdatesEnd,
}

enum WorkerMessage {
    Tick,
    UseBlock { node_id: NodeId },
    PressurePlate { node_id: NodeId, powered: bool },
    Inspect { node_id: NodeId },
    Flush { io_only: bool },
    Reset,
}

enum ChangeMessage {
    Change { pos: BlockPos, block: Block },
    ChangesEnd,
}

impl Worker {
    fn set_node(&mut self, node_id: NodeId, powered: bool, new_power: u8) {
        let node = &mut self.nodes[node_id];
        let old_power = node.output_power;

        node.changed = true;
        node.powered = powered;
        node.output_power = new_power;
        for i in 0..node.updates.len() {
            let node = &self.nodes[node_id];
            let update_link = unsafe { *node.updates.get_unchecked(i) };
            let side = update_link.side();
            let distance = update_link.ss();
            let update = update_link.node();

            let update_ref = &mut self.nodes[update];
            if update_ref.is_analog() {
                let old_power = old_power.saturating_sub(distance);
                let new_power = new_power.saturating_sub(distance);
                if old_power == new_power {
                    continue;
                }

                let (input, inputs) = if side {
                    (&mut update_ref.side_power, &mut update_ref.side_inputs)
                } else {
                    (
                        &mut update_ref.default_power,
                        &mut update_ref.default_inputs,
                    )
                };

                unsafe {
                    *inputs.get_unchecked_mut(old_power as usize) -= 1;
                    *inputs.get_unchecked_mut(new_power as usize) += 1;
                }

                let power = last_index_positive(inputs);

                if *input == power {
                    continue;
                }
                *input = power;

                if side {
                    update_node_side_analog(&mut self.scheduler, update, update_ref);
                } else {
                    update_node_default_analog(
                        &mut self.scheduler,
                        &mut self.outputs,
                        update,
                        update_ref,
                    );
                }
            } else {
                let old_power = old_power > distance;
                let new_power = new_power > distance;

                if old_power == new_power {
                    continue;
                }

                let input = if side {
                    &mut update_ref.side_power
                } else {
                    &mut update_ref.default_power
                };

                if new_power {
                    *input += 1;
                    if *input != 1 {
                        continue;
                    }
                } else {
                    *input -= 1;
                    if *input != 0 {
                        continue;
                    }
                }

                if side {
                    update_node_side_digital(&mut self.scheduler, update, update_ref);
                } else {
                    update_node_default_digital(&mut self.scheduler, update, update_ref);
                }
            }
            //update_node(&mut self.scheduler, &mut self.outputs, update, update_ref);
        }
    }
}

impl JITBackend for ThreadingBackend {
    fn inspect(&mut self, pos: BlockPos) {
        let Some(&(worker_id, node_id)) = self.pos_map.get(&pos) else {
            debug!("could not find node at pos {}", pos);
            return;
        };
        self.work_senders[worker_id.0 as usize].send(WorkerMessage::Inspect { node_id });
    }

    fn reset<W: World>(&mut self, world: &mut W, io_only: bool) {
        for _ in 0..FLUSH_OFFSET {
            for recv in self.change_receivers.iter_mut() {
                while let ChangeMessage::Change { pos, block } = recv.recv() {
                    world.set_block(pos, block);
                }
            }
        }

        for worker in self.work_senders.iter_mut() {
            worker.send(WorkerMessage::Reset);
        }
        for handle in self.handles.drain(..) {
            let mut worker = handle.join().unwrap();

            worker.scheduler.reset(world, &worker.blocks);

            let nodes = std::mem::take(&mut worker.nodes);

            for (i, node) in nodes.into_inner().iter().enumerate() {
                let Some((pos, block)) = worker.blocks[i] else {
                    continue;
                };
                if matches!(node.ty, NodeType::Comparator { .. }) {
                    let block_entity = BlockEntity::Comparator {
                        output_strength: node.output_power,
                    };
                    world.set_block_entity(pos, block_entity);
                }

                if io_only && !node.is_io {
                    world.set_block(pos, block);
                }
            }
        }
        self.handles.clear();
        self.change_receivers.clear();
        self.work_senders.clear();
        self.pos_map.clear();
    }

    fn on_use_block(&mut self, pos: BlockPos) {
        let (worker_id, node_id) = self.pos_map[&pos];
        self.work_senders[worker_id.0 as usize].send(WorkerMessage::UseBlock { node_id });
    }

    fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        let (worker_id, node_id) = self.pos_map[&pos];
        self.work_senders[worker_id.0 as usize]
            .send(WorkerMessage::PressurePlate { node_id, powered });
    }

    fn tick(&mut self) {
        for worker in self.work_senders.iter_mut() {
            worker.send(WorkerMessage::Tick);
        }
    }

    fn compile(&mut self, graph: CompileGraph, ticks: Vec<TickEntry>, options: &CompilerOptions, monitor: Arc<TaskMonitor>) {
        let groups = compile_graph::weakly_connected_components(&graph);
        let big_group_count = groups
            .iter()
            .filter(|group| group.len() > graph.node_count() / 10_000)
            .count();

        trace!(
            "Found groups. Big groups: {}, Small groups: {}",
            big_group_count,
            groups.len()
        );

        trace!(
            "Partitioned groups. Count: {}, Sizes: {:?}",
            groups.len(),
            groups
                .iter()
                .map(|group| group.len())
                .sorted()
                .collect_vec()
        );

        let thread_count = std::thread::available_parallelism().unwrap().get() - 1;
        let mut groups = compile_graph::multiway_number_partitioning(
            groups,
            big_group_count.min(thread_count).max(1),
        );
        trace!(
            "Partitioned groups. Count: {}, Sizes: {:?}",
            groups.len(),
            groups.iter().map(|group| group.len()).collect_vec()
        );

        let mut workers = vec![];

        for group in groups.iter_mut() {
            group.sort_by_key(|&idx| Reverse(graph[idx].is_io_and_flushable()));
        }

        let nodes_map: FxHashMap<NodeIdx, (WorkerId, NodeId)> = groups
            .iter()
            .enumerate()
            .flat_map(|(worker_id, group)| {
                group.iter().enumerate().map(move |(node_id, node_idx)| {
                    (
                        *node_idx,
                        (WorkerId(worker_id.try_into().unwrap()), unsafe {
                            NodeId::from_index(node_id)
                        }),
                    )
                })
            })
            .collect();

        let mut worker_output_indicies = vec![];

        for (worker_id, group) in groups.into_iter().enumerate() {
            let mut stats = FinalGraphStats::default();
            let mut nodes = vec![];
            let mut blocks = vec![];
            let mut output_indicies = vec![];
            let mut last_io_index = 0;
            let mut tickable_nodes = 0;

            for node_idx in group.iter() {
                let (worker_id, node_id) = nodes_map[node_idx];
                let node = &graph[*node_idx];

                if let Some((pos, id)) = node.block {
                    blocks.push(Some((pos, Block::from_id(id))));
                    self.pos_map.insert(pos, (worker_id, node_id));
                } else {
                    blocks.push(None);
                }

                if node.is_io_and_flushable() {
                    last_io_index = last_io_index.max(node_id.index());
                }

                if node.can_be_ticked() {
                    tickable_nodes += 1;
                }

                nodes.push(Node::from_compile_node(
                    &graph,
                    *node_idx,
                    &nodes_map,
                    &mut output_indicies,
                    &mut stats,
                ));
            }
            stats.nodes_bytes = nodes.len() * std::mem::size_of::<Node>();
            trace!("{:#?}", stats);

            let (mut change_sender, change_receiver) = RingBuffer::new(CHANGES_CHANNEL_SIZE);
            for _ in 0..FLUSH_OFFSET {
                change_sender.send(ChangeMessage::ChangesEnd);
            }
            self.change_receivers.push(change_receiver);

            let (work_sender, work_receiver) = RingBuffer::new(WORK_CHANNEL_SIZE);
            self.work_senders.push(work_sender);

            let scheduler = unsafe { TickScheduler::new(tickable_nodes) };

            let worker = Worker {
                id: WorkerId(worker_id.try_into().unwrap()),
                inputs: vec![],
                outputs: vec![],
                work_receiver,
                change_sender,
                nodes: Nodes::new(nodes.into_boxed_slice()),
                scheduler,
                blocks,
                last_io_index,
            };
            // Dot file output
            // println!("{}", worker);
            workers.push(worker);
            worker_output_indicies.push(output_indicies);
        }

        for worker_id in 0..workers.len() {
            for &(target_id, delay) in worker_output_indicies[worker_id].iter() {
                assert_ne!(delay, 0);
                let (mut sender, receiver) = RingBuffer::new(UPDATES_CHANNEL_SIZE);
                for _ in 0..(delay as usize * TickScheduler::NUM_PRIORITIES) {
                    sender.send(UpdateMessage::UpdatesEnd);
                }
                workers[worker_id].outputs.push(sender);
                workers[target_id.0 as usize].inputs.push(receiver);
            }
        }

        for entry in ticks {
            if let Some((worker_id, node_id)) = self.pos_map.get(&entry.pos) {
                let worker = &mut workers[worker_id.0 as usize];
                worker.scheduler.schedule_tick(
                    NodeIdWithData::new(*node_id),
                    entry.ticks_left as usize,
                    entry.tick_priority,
                );
                worker.nodes[*node_id].pending_tick = true;
            }
        }

        for worker in workers {
            self.handles.push(
                ThreadBuilder::default()
                    .name(format!("Worker Thread {}", worker.id.0))
                    .priority(ThreadPriority::Max)
                    .spawn_careless(|| worker.run())
                    .expect("failed to spawn worker thread"),
            );
        }
    }

    fn flush<W: World>(&mut self, world: &mut W, io_only: bool) {
        for root in self.work_senders.iter_mut() {
            root.send(WorkerMessage::Flush { io_only });
        }
        for recv in self.change_receivers.iter_mut() {
            while let ChangeMessage::Change { pos, block } = recv.recv() {
                world.set_block(pos, block);
            }
        }
    }
}

impl Worker {
    fn tick_node(&mut self, node_data: NodeIdWithData) {
        let node_id = node_data.node();
        let internal_tick = node_data.internal_tick();
        let node = &mut self.nodes[node_id];
        if !internal_tick {
            node.pending_tick = false;
        }

        match node.ty {
            NodeType::Repeater { delay, .. } => {
                if node.side_power > 0 {
                    return;
                }

                let should_be_powered = get_bool_input(node);
                if node.powered && !should_be_powered {
                    self.set_node(node_id, false, 0);
                } else if !node.powered {
                    if !should_be_powered {
                        schedule_tick(
                            &mut self.scheduler,
                            NodeIdWithData::new(node_id),
                            node,
                            delay as usize,
                            TickPriority::Higher,
                        );
                    }
                    self.set_node(node_id, true, 15);
                }
            }
            NodeType::Torch => {
                let should_be_off = get_bool_input(node);
                let lit = node.powered;
                if lit && should_be_off {
                    self.set_node(node_id, false, 0);
                } else if !lit && !should_be_off {
                    self.set_node(node_id, true, 15);
                }
            }
            NodeType::Comparator {
                mode, far_input, ..
            } => {
                let mut input_power = get_analog_input(node);
                let side_input_power = get_analog_side(node);
                if let Some(far_override) = far_input {
                    if input_power < 15 {
                        input_power = far_override.get();
                    }
                }
                let old_strength = node.output_power;
                let new_strength = calculate_comparator_output(mode, input_power, side_input_power);
                if new_strength != old_strength {
                    self.set_node(node_id, new_strength > 0, new_strength);
                }
            }
            NodeType::Lamp => {
                let should_be_lit = get_bool_input(node);
                if node.powered && !should_be_lit {
                    self.set_node(node_id, false, 0);
                }
            }
            NodeType::Button => {
                if node.powered {
                    self.set_node(node_id, false, 0);
                }
            }
            NodeType::ComparatorLine { delay } => {
                if internal_tick {
                    let new_strength = node_data.ss();
                    if new_strength != node.output_power {
                        self.set_node(node_id, new_strength > 0, new_strength);
                    }
                } else {
                    let input_power = get_analog_input(node);
                    schedule_tick(
                        &mut self.scheduler,
                        NodeIdWithData::new_with_data(node_id, true, input_power),
                        node,
                        delay as usize - 1,
                        TickPriority::Normal,
                    );
                    node.pending_tick = false;
                }
            }
            _ => warn!("Node {:?} should not be ticked!", node.ty),
        }
    }

    fn handle_tick(&mut self) {
        let mut queues = self.scheduler.queues_this_tick();

        // for sender in self.outputs.iter_mut() {
        //     sender.begin_commit(1024);
        // }

        for queue in queues.0.iter_mut() {
            // let mut inputs = std::mem::take(&mut self.inputs);
            // for input in inputs.iter_mut() {
            //     while let UpdateMessage::Update {
            //         node_id,
            //         signal_strength,
            //     } = input.recv()
            //     {
            //         self.set_node(node_id, signal_strength > 0, signal_strength);
            //     }
            // }
            // self.inputs = inputs;

            for node_data in queue.drain(..) {
                self.tick_node(node_data);
            }

            // for output in self.outputs.iter_mut() {
            //     output.send_unsafe(UpdateMessage::UpdatesEnd);
            // }
        }

        // for sender in self.outputs.iter_mut() {
        //     sender.end_commit();
        // }

        self.scheduler.end_tick(queues);
    }

    fn handle_use_block(&mut self, node_id: NodeId) {
        let node = &self.nodes[node_id];
        match node.ty {
            NodeType::Button => {
                if !node.powered {
                    self.scheduler.schedule_tick(
                        NodeIdWithData::new(node_id),
                        10,
                        TickPriority::Normal,
                    );
                    self.set_node(node_id, true, 15);
                }
            }
            NodeType::Lever => {
                self.set_node(node_id, !node.powered, bool_to_ss(!node.powered));
            }
            _ => warn!("Tried to use a {:?} redpiler node", node.ty),
        }
    }

    fn handle_pressure_plate(&mut self, node_id: NodeId, powered: bool) {
        let node = &self.nodes[node_id];
        match node.ty {
            NodeType::PressurePlate => {
                self.set_node(node_id, powered, bool_to_ss(powered));
            }
            _ => warn!("Tried to set pressure plate state for a {:?}", node.ty),
        }
    }

    fn handle_flush(&mut self, io_only: bool) {
        let num = if io_only {
            self.last_io_index + 1
        } else {
            self.nodes.len()
        };
        self.change_sender.begin_commit(num + 1);

        for i in 0..num {
            let Some((pos, block)) = &mut self.blocks[i] else {
                continue;
            };
            let idx = unsafe { NodeId::from_index(i) };
            let node = &mut self.nodes[idx];
            if node.changed {
                if let Some(powered) = block_powered_mut(block) {
                    *powered = node.powered
                }
                if let Block::RedstoneWire { wire, .. } = block {
                    wire.power = node.output_power
                };
                if let Block::RedstoneRepeater { repeater } = block {
                    repeater.locked = node.side_power > 0;
                }
                self.change_sender.send_unsafe(ChangeMessage::Change {
                    pos: *pos,
                    block: *block,
                });
                node.changed = false;
            }
        }
        self.change_sender.send_unsafe(ChangeMessage::ChangesEnd);
        self.change_sender.end_commit();
    }

    fn run(mut self) -> Self {
        loop {
            match self.work_receiver.recv() {
                WorkerMessage::Tick => self.handle_tick(),
                WorkerMessage::UseBlock { node_id } => self.handle_use_block(node_id),
                WorkerMessage::PressurePlate { node_id, powered } => {
                    self.handle_pressure_plate(node_id, powered)
                }
                WorkerMessage::Inspect { node_id } => {
                    debug!("Node {:?}: {:#?}", node_id, self.nodes[node_id])
                }
                WorkerMessage::Flush { io_only } => self.handle_flush(io_only),
                WorkerMessage::Reset => return self,
            }
        }
    }
}

/// Set node for use in `update`. None of the nodes here have usable output power,
/// so this function does not set that.
fn set_node(node: &mut Node, powered: bool) {
    node.powered = powered;
    node.changed = true;
}

fn schedule_tick(
    scheduler: &mut TickScheduler,
    node_data: NodeIdWithData,
    node: &mut Node,
    delay: usize,
    priority: TickPriority,
) {
    node.pending_tick = true;
    scheduler.schedule_tick(node_data, delay, priority);
}

fn get_bool_input(node: &Node) -> bool {
    node.default_power > 0
}

fn get_bool_side(node: &Node) -> bool {
    node.side_power > 0
}

fn last_index_positive(array: &[u8; 16]) -> u8 {
    // Note: this might be slower on big-endian systems
    let value = u128::from_le_bytes(*array);
    if value == 0 {
        0
    } else {
        15 - (value.leading_zeros() >> 3) as u8
    }
}

fn get_analog_input(node: &Node) -> u8 {
    node.default_power
}

fn get_analog_side(node: &Node) -> u8 {
    node.side_power
}

//#[inline(never)]
fn update_node_default_digital(scheduler: &mut TickScheduler, node_id: NodeId, node: &mut Node) {
    if node.pending_tick {
        return;
    }
    let should_be_powered = get_bool_input(node);
    match node.ty {
        NodeType::Repeater {
            delay,
            facing_diode,
        } => {
            let locked = get_bool_side(node);

            if !node.pending_tick && !locked && should_be_powered != node.powered {
                let priority = if facing_diode {
                    TickPriority::Highest
                } else if !should_be_powered {
                    TickPriority::Higher
                } else {
                    TickPriority::High
                };
                schedule_tick(
                    scheduler,
                    NodeIdWithData::new(node_id),
                    node,
                    delay as usize,
                    priority,
                );
            }
        }
        NodeType::Torch => {
            if !node.pending_tick && should_be_powered == node.powered {
                schedule_tick(
                    scheduler,
                    NodeIdWithData::new(node_id),
                    node,
                    1,
                    TickPriority::Normal,
                );
            }
        }
        NodeType::Lamp => {
            if should_be_powered && !node.powered {
                set_node(node, true);
            } else if !should_be_powered && node.powered {
                schedule_tick(
                    scheduler,
                    NodeIdWithData::new(node_id),
                    node,
                    2,
                    TickPriority::Normal,
                );
            }
        }
        NodeType::Trapdoor => {
            set_node(node, should_be_powered);
        }
        _ => unsafe { std::hint::unreachable_unchecked() },
    }
}

//#[inline(never)]
fn update_node_default_analog(
    scheduler: &mut TickScheduler,
    outputs: &mut [Sender<UpdateMessage>],
    node_id: NodeId,
    node: &mut Node,
) {
    let input_power = get_analog_input(node);
    match node.ty {
        NodeType::Comparator {
            mode,
            far_input,
            facing_diode,
        } => {
            if node.pending_tick {
                return;
            }
            let mut input_power = get_analog_input(node);
            let side_input_power = get_analog_side(node);
            if let Some(far_override) = far_input {
                if input_power < 15 {
                    input_power = far_override.get();
                }
            }
            let old_strength = node.output_power;
            let output_power = calculate_comparator_output(mode, input_power, side_input_power);
            if output_power != old_strength {
                let priority = if facing_diode {
                    TickPriority::High
                } else {
                    TickPriority::Normal
                };
                schedule_tick(scheduler, NodeIdWithData::new(node_id), node, 1, priority);
            }
        }
        NodeType::Wire => {
            node.output_power = input_power;
            node.changed = true;
        }
        NodeType::ExternalOutput {
            output_index,
            target_id,
        } => {
            node.output_power = input_power;
            node.changed = true;

            let output = unsafe { outputs.get_unchecked_mut(output_index as usize) };
            output.send_unsafe(UpdateMessage::Update {
                node_id: target_id,
                signal_strength: input_power,
            });
        }
        NodeType::ComparatorLine { .. } => {
            if !node.pending_tick {
                schedule_tick(
                    scheduler,
                    NodeIdWithData::new_with_data(node_id, false, 0),
                    node,
                    1,
                    TickPriority::Normal,
                );
            }
        }
        _ => unsafe { std::hint::unreachable_unchecked() },
    }
}

//#[inline(never)]
fn update_node_side_digital(scheduler: &mut TickScheduler, node_id: NodeId, node: &mut Node) {
    match node.ty {
        NodeType::Repeater {
            delay,
            facing_diode,
        } => {
            let locked = get_bool_side(node);
            node.changed = true;

            let should_be_powered = get_bool_input(node);
            if !locked && !node.pending_tick && should_be_powered != node.powered {
                let priority = if facing_diode {
                    TickPriority::Highest
                } else if !should_be_powered {
                    TickPriority::Higher
                } else {
                    TickPriority::High
                };
                schedule_tick(
                    scheduler,
                    NodeIdWithData::new(node_id),
                    node,
                    delay as usize,
                    priority,
                );
            }
        }
        _ => unsafe { std::hint::unreachable_unchecked() },
    }
}

//#[inline(never)]
fn update_node_side_analog(scheduler: &mut TickScheduler, node_id: NodeId, node: &mut Node) {
    match node.ty {
        NodeType::Comparator {
            mode,
            far_input,
            facing_diode,
        } => {
            if node.pending_tick {
                return;
            }
            let mut input_power = get_analog_input(node);
            let side_input_power = get_analog_side(node);
            if let Some(far_override) = far_input {
                if input_power < 15 {
                    input_power = far_override.get();
                }
            }
            let old_strength = node.output_power;
            let output_power = calculate_comparator_output(mode, input_power, side_input_power);
            if output_power != old_strength {
                let priority = if facing_diode {
                    TickPriority::High
                } else {
                    TickPriority::Normal
                };
                schedule_tick(scheduler, NodeIdWithData::new(node_id), node, 1, priority);
            }
        }
        _ => unsafe { std::hint::unreachable_unchecked() },
    }
}

impl fmt::Display for Worker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("digraph{")?;
        for (id, node) in self.nodes.inner().iter().enumerate() {
            if matches!(node.ty, NodeType::Wire) {
                continue;
            }
            let label = match node.ty {
                NodeType::Constant => format!("Constant: {}", node.output_power),
                _ => format!("{:?}", node.ty)
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .to_string(),
            };
            let pos = if let Some((pos, _)) = self.blocks[id] {
                format!("{}, {}, {}", pos.x, pos.y, pos.z)
            } else {
                "No Pos".to_string()
            };
            write!(f, "n{}[label=\"{}\\n({})\"];", id, label, pos,)?;
            for link in node.updates.iter() {
                let out_index = link.node().index();
                let distance = link.ss();
                let color = if link.side() { ",color=\"blue\"" } else { "" };
                write!(
                    f,
                    "n{}->n{}[label=\"{}\"{}];",
                    id, out_index, distance, color
                )?;
            }
            // for update in &node.updates {
            //     write!(f, "n{}->n{}[style=dotted];", id, update)?;
            // }
        }
        f.write_str("}\n")
    }
}
