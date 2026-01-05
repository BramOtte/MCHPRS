//! The direct backend does not do code generation and operates on the `CompileNode` graph directly

mod compile;
mod node;
mod tick;
mod update;

use super::JITBackend;
use crate::backend::direct::node::ForwardLink;
use crate::backend::direct::update::update_node;
use crate::compile_graph::CompileGraph;
use crate::passes::AnalysisInfos;
use crate::task_monitor::TaskMonitor;
use crate::{block_powered_mut, CompilerOptions};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ComparatorMode, Instrument};
use mchprs_blocks::BlockPos;
use mchprs_redstone::{bool_to_ss, noteblock};
use mchprs_sync::spsc;
use mchprs_world::{TickEntry, TickPriority, World};
use node::{Node, NodeId, NodeType, Nodes};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::{fmt, mem};
use tracing::{debug, warn};

#[derive(Default, Clone)]
struct Queues([Vec<NodeId>; TickScheduler::NUM_PRIORITIES]);

impl Queues {
    fn drain_iter(&mut self) -> impl Iterator<Item = NodeId> + '_ {
        self.0.iter_mut().flat_map(|q| q.drain(..))
    }
}

#[derive(Default, Clone)]
struct TickScheduler {
    queues_deque: [Queues; Self::NUM_QUEUES],
    pos: usize,
}

impl TickScheduler {
    const NUM_PRIORITIES: usize = 4;
    const NUM_QUEUES: usize = 16;

    fn reset<W: World>(&mut self, world: &mut W, blocks: &[impl AsRef<[(BlockPos, Block)]>]) {
        for (idx, queues) in self.queues_deque.iter().enumerate() {
            let delay = if self.pos >= idx {
                idx + Self::NUM_QUEUES
            } else {
                idx
            } - self.pos;
            for (entries, priority) in queues.0.iter().zip(Self::priorities()) {
                for node in entries {
                    if blocks[node.index()].as_ref().is_empty() {
                        warn!("Cannot schedule tick for node {:?} because block information is missing", node);
                    }
                    for (pos, _) in blocks[node.index()].as_ref().iter().copied() {
                        world.schedule_tick(pos, delay as u32, priority);
                    }
                }
            }
        }
        for queues in self.queues_deque.iter_mut() {
            for queue in queues.0.iter_mut() {
                queue.clear();
            }
        }
    }

    fn schedule_tick(&mut self, node: NodeId, delay: usize, priority: TickPriority) {
        self.queues_deque[(self.pos + delay) % Self::NUM_QUEUES].0[priority as usize].push(node);
    }

    fn queues_this_tick(&mut self) -> Queues {
        self.pos = (self.pos + 1) % Self::NUM_QUEUES;
        mem::take(&mut self.queues_deque[self.pos])
    }

    fn end_tick(&mut self, mut queues: Queues) {
        for queue in &mut queues.0 {
            queue.clear();
        }
        self.queues_deque[self.pos] = queues;
    }

    fn priorities() -> [TickPriority; Self::NUM_PRIORITIES] {
        [
            TickPriority::Highest,
            TickPriority::Higher,
            TickPriority::High,
            TickPriority::Normal,
        ]
    }

    fn has_pending_ticks(&self) -> bool {
        for queues in &self.queues_deque {
            for queue in &queues.0 {
                if !queue.is_empty() {
                    return true;
                }
            }
        }
        false
    }
}

#[derive(Clone)]
enum Event {
    NoteBlockPlay { noteblock_id: u16 },
}


#[derive(Default, Debug)]
enum Message {
    Update(NodeId, bool, u8, u8),
    #[default]
    Tick,
    TickN(u64),
    Stop,
    Flush(bool),
    FlushUpdate(BlockPos, Block),
    UseBlock(BlockPos),
}

const UPDATE_QUEUE_SIZE: usize = 1024;
type UpdateQueue = [Message; UPDATE_QUEUE_SIZE];
type UpdateSender = spsc::Sender<UpdateQueue>;
type UpdateReceiver = spsc::Receiver<UpdateQueue>;

#[derive(Default)]
pub struct DirectBackend {
    active: Option<Active>,
}

pub struct Active {
    threads: Vec<JoinHandle<()>>,
    input_sender: UpdateSender,
    output_receiver: UpdateReceiver,
    state: DirectState,
    shared: Arc<Shared>,
}

struct Shared {
    has_ticks_pending: Vec<AtomicBool>,
}

impl JITBackend for DirectBackend {
    fn compile(&mut self, graph: CompileGraph, ticks: &[TickEntry], options: &CompilerOptions, monitor:Arc<TaskMonitor>, analysis_infos: &AnalysisInfos) {
        let (input_sender, input_receiver) = spsc::channel_using([(); UPDATE_QUEUE_SIZE].map(|_| Default::default()));
        
        let (between_sender, between_receiver) = spsc::channel_using([(); UPDATE_QUEUE_SIZE].map(|_| Default::default()));

        let (output_sender, output_receiver) = spsc::channel_using([(); UPDATE_QUEUE_SIZE].map(|_| Default::default()));

        let (tmp_sender, tmp_receiver) = spsc::channel_using([(); UPDATE_QUEUE_SIZE].map(|_| Default::default()));

        let mut has_ticks_pending = Vec::new();
        for _ in 0..2 {
            has_ticks_pending.push(AtomicBool::new(true));
        }

        let shared = Arc::new(Shared {
            has_ticks_pending,
        });

        let mut input_state = DirectState::new(between_sender, shared.clone());
        let mut output_state = DirectState::new(output_sender, shared.clone());
        let mut state = DirectState::new(tmp_sender, shared.clone());

        for state in [&mut input_state, &mut output_state, &mut state] {
            for queues in state.scheduler.queues_deque.iter_mut() {
                for queue in queues.0.iter_mut() {
                    queue.clear();
                }
            }
        }


        input_state.compile(graph.clone(), ticks, options, monitor.clone(), analysis_infos);
        state.compile(graph.clone(), ticks, options, monitor.clone(), analysis_infos);

        output_state.compile(graph, ticks, options, monitor, analysis_infos);

        let [input_thread, output_thread] = [(input_state, input_receiver, 0), (output_state, between_receiver, 1)].map(|(mut state, mut receiver, i)| std::thread::spawn(move || {
            // let mut update_cnt = 0;
            // let mut tick_cnt = 0;
            let mut running = true;
            while running {
                receiver.recv(|update| {
                    match update {
                        &Message::Update(node_id, side, old_power, new_power) => {
                            let update_ref = &mut state.nodes[node_id];
                            let inputs = if side {
                                &mut update_ref.side_inputs
                            } else {
                                &mut update_ref.default_inputs
                            };

                            // Safety: signal strength is never larger than 15
                            unsafe {
                                *inputs.ss_counts.get_unchecked_mut(old_power as usize) -= 1;
                                *inputs.ss_counts.get_unchecked_mut(new_power as usize) += 1;
                            }

                            update_node(&mut state.scheduler, &mut state.events, &mut state.nodes, node_id);
                        },
                        Message::Tick => {
                            state.tick();
                            state.update_sender.send(|msg| *msg = Message::Tick);
                            // state.shared.has_ticks_pending[i].store(state.has_pending_ticks(), Ordering::Relaxed);
                        },
                        &Message::TickN(n) => {
                            for _ in 0..n {
                                state.tick();
                                state.update_sender.send(|msg| *msg = Message::Tick);
                            }
                        },
                        &Message::UseBlock(pos) => {
                            state.on_use_block(pos);
                        }
                        &Message::Flush(io_only) => {
                            state.flush2(io_only);
                            state.update_sender.send(|msg| *msg = Message::Flush(io_only));
                            return;
                        },
                        Message::FlushUpdate(pos, block) => {
                            state.update_sender.send(|msg| *msg = Message::FlushUpdate(*pos, *block));
                        },
                        Message::Stop => {
                            state.update_sender.send(|msg| *msg = Message::Stop);
                            running = false;
                            return;
                        },
                    } 
                });
            }
        }));

        self.active = Some(Active {
            threads: vec![input_thread, output_thread],
            input_sender,
            output_receiver,
            state,
            shared
        });
    }

    fn tickn(&mut self, n: u64) {
        let Some(active) = &mut self.active else {
            return;
        };

        active.input_sender.send(|msg| *msg = Message::TickN(n));
    }

    fn tick(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };

        active.input_sender.send(|msg| *msg = Message::Tick);
    }

    fn on_use_block(&mut self, pos: BlockPos) {
        let Some(active) = &mut self.active else {
            return;
        };

        active.input_sender.send(|msg| *msg = Message::UseBlock(pos));
    }

    fn set_pressure_plate(&mut self,pos:BlockPos,powered:bool) {
        todo!()
    }

    fn flush<W:World>(&mut self, world: &mut W, io_only:bool) {
        let Some(active) = &mut self.active else {
            return;
        };

        active.input_sender.send(|msg| *msg = Message::Flush(io_only));

        let mut running = true;
        while running {
            active.output_receiver.recv(|msg| {
                match msg {
                    &Message::FlushUpdate(pos, block) => {
                        // println!("{:?} {:?}", pos, block);
                        world.set_block(pos, block);
                    }
                    &Message::Update(node, side, old, new ) => {
                        println!("## {:?} {:?} {:?} {:?} {:?}", node, active.state.nodes[node], side, old, new);
                        panic!();
                    }
                    Message::Flush(_) | Message::Stop => running = false,
                    _ => {}
                }
            });
        }
    }

    fn reset<W:World>(&mut self, world: &mut W, io_only:bool) {
        todo!()
    }

    fn has_pending_ticks(&self) -> bool {
        let Some(active) = &self.active else {
            return false;
        };

        for pending in active.shared.has_ticks_pending.iter() {
            if pending.load(Ordering::Relaxed) {
                return true;
            }
        }

        return false;
    }

    #[doc = " Inspect block for debugging"]
    fn inspect(&mut self, pos:BlockPos) {
        todo!()
    }
}

pub struct DirectState {
    nodes: Nodes,
    forward_links: Vec<ForwardLink>,
    blocks: Vec<SmallVec<[(BlockPos, Block); 1]>>,
    pos_map: FxHashMap<BlockPos, NodeId>,
    scheduler: TickScheduler,
    events: Vec<Event>,
    noteblock_info: Vec<(SmallVec<[BlockPos; 1]>, Instrument, u32)>,

    update_sender: UpdateSender,
    shared: Arc<Shared>,
}

impl DirectState {
    fn new(update_sender: UpdateSender, shared: Arc<Shared>) -> Self {
        Self {
            nodes: Default::default(),
            forward_links: Default::default(),
            blocks: Default::default(),
            pos_map: Default::default(),
            scheduler: Default::default(),
            events: Default::default(),
            noteblock_info: Default::default(),
            update_sender,
            shared
        }
    }

    fn schedule_tick(&mut self, node_id: NodeId, delay: usize, priority: TickPriority) {
        self.scheduler.schedule_tick(node_id, delay, priority);
    }

    fn set_node(&mut self, node_id: NodeId, powered: bool, new_power: u8) {
        let node = &mut self.nodes[node_id];
        let partition = node.partition;
        let old_power = node.output_power;

        node.changed = true;
        node.powered = powered;
        node.output_power = new_power;

        for forward_link in &self.forward_links[node.fwd_link_begin..node.fwd_link_end] {
            let side = forward_link.side();
            let distance = forward_link.ss();
            let update = forward_link.node();

            let update_ref = &mut self.nodes[update];
            let inputs = if side {
                &mut update_ref.side_inputs
            } else {
                &mut update_ref.default_inputs
            };

            let old_power = old_power.saturating_sub(distance);
            let new_power = new_power.saturating_sub(distance);

            if old_power == new_power {
                continue;
            }

            if update_ref.partition != partition {
                self.update_sender.send(|msg| *msg = Message::Update(update, side, old_power, new_power));
                continue;
            }

            // Safety: signal strength is never larger than 15
            unsafe {
                *inputs.ss_counts.get_unchecked_mut(old_power as usize) -= 1;
                *inputs.ss_counts.get_unchecked_mut(new_power as usize) += 1;
            }

            update::update_node(
                &mut self.scheduler,
                &mut self.events,
                &mut self.nodes,
                update,
            );
        }
    }

    fn flush2(&mut self, io_only: bool) {
        for (i, node) in self.nodes.inner_mut().iter_mut().enumerate() {
            if !node.changed || (io_only && !node.is_io) {
                continue;
            }
            node.changed = false;
            for (pos, block) in &mut self.blocks[i] {
                if let Some(powered) = block_powered_mut(block) {
                    *powered = node.powered
                }
                if let Block::RedstoneWire { wire, .. } = block {
                    wire.power = node.output_power
                };
                if let Block::RedstoneRepeater { repeater } = block {
                    repeater.locked = node.locked;
                }

                self.update_sender.send(|msg| *msg = Message::FlushUpdate(*pos, *block));
            }
        }
    }
}

impl DirectState {
    fn inspect(&mut self, pos: BlockPos) {
        let Some(node_id) = self.pos_map.get(&pos) else {
            debug!("could not find node at pos {}", pos);
            return;
        };

        debug!("Node {:?}: {:#?}", node_id, self.nodes[*node_id]);
    }

    fn reset<W: World>(&mut self, world: &mut W, io_only: bool) {
        self.scheduler.reset(world, &self.blocks);

        let nodes = std::mem::take(&mut self.nodes);

        for (i, node) in nodes.into_inner().iter().enumerate() {
            for (pos, block) in self.blocks[i].iter().copied() {
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

        self.forward_links.clear();
        self.pos_map.clear();
        self.noteblock_info.clear();
        self.events.clear();
    }

    fn on_use_block(&mut self, pos: BlockPos) {
        let node_id = self.pos_map[&pos];
        let node = &self.nodes[node_id];
        match node.ty {
            NodeType::Button => {
                if node.powered {
                    return;
                }
                self.schedule_tick(node_id, 10, TickPriority::Normal);
                self.set_node(node_id, true, 15);
            }
            NodeType::Lever => {
                self.set_node(node_id, !node.powered, bool_to_ss(!node.powered));
            }
            _ => warn!("Tried to use a {:?} redpiler node", node.ty),
        }
    }

    fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        let node_id = self.pos_map[&pos];
        let node = &self.nodes[node_id];
        match node.ty {
            NodeType::PressurePlate => {
                self.set_node(node_id, powered, bool_to_ss(powered));
            }
            _ => warn!("Tried to set pressure plate state for a {:?}", node.ty),
        }
    }

    fn tick(&mut self) {
        let mut queues = self.scheduler.queues_this_tick();
        
        // let mut ticks = 0;
        for node_id in queues.drain_iter() {
            // ticks += 1;
            self.tick_node(node_id);
        }

        self.scheduler.end_tick(queues);

        // ticks
    }

    fn flush<W: World>(&mut self, world: &mut W, io_only: bool) {
        for event in self.events.drain(..) {
            match event {
                Event::NoteBlockPlay { noteblock_id } => {
                    let (positions, instrument, note) = &self.noteblock_info[noteblock_id as usize];
                    for pos in positions.iter().copied() {
                        noteblock::play_note(world, pos, *instrument, *note);
                    }
                }
            }
        }
        for (i, node) in self.nodes.inner_mut().iter_mut().enumerate() {
            if !node.changed || (io_only && !node.is_io) {
                continue;
            }
            node.changed = false;
            for (pos, block) in &mut self.blocks[i] {
                if let Some(powered) = block_powered_mut(block) {
                    *powered = node.powered
                }
                if let Block::RedstoneWire { wire, .. } = block {
                    wire.power = node.output_power
                };
                if let Block::RedstoneRepeater { repeater } = block {
                    repeater.locked = node.locked;
                }
                world.set_block(*pos, *block);
            }
        }
    }

    fn compile(
        &mut self,
        graph: CompileGraph,
        ticks: &[TickEntry],
        options: &CompilerOptions,
        monitor: Arc<TaskMonitor>,
        analysis_infos: &AnalysisInfos
    ) {
        compile::compile(self, graph, ticks, options, monitor, analysis_infos);
    }

    fn has_pending_ticks(&self) -> bool {
        self.scheduler.has_pending_ticks()
    }
}

/// Set node for use in `update`. None of the nodes here have usable output power,
/// so this function does not set that.
fn set_node(node: &mut Node, powered: bool) {
    node.powered = powered;
    node.changed = true;
}

fn set_node_locked(node: &mut Node, locked: bool) {
    node.locked = locked;
    node.changed = true;
}

fn schedule_tick(
    scheduler: &mut TickScheduler,
    node_id: NodeId,
    node: &mut Node,
    delay: usize,
    priority: TickPriority,
) {
    node.pending_tick = true;
    scheduler.schedule_tick(node_id, delay, priority);
}

fn get_bool_input(node: &Node) -> bool {
    // During compilation its ensured all signal strength buckets add up to 255
    // So if and only if the zero bucket contains 255 is the input zero
    node.default_inputs.ss_counts[0] != 255
}

fn get_bool_side(node: &Node) -> bool {
    node.side_inputs.ss_counts[0] != 255
}

fn last_index_positive(array: &[u8; 16]) -> u32 {
    // Note: this might be slower on big-endian systems
    let value = u128::from_le_bytes(*array);
    if value == 0 {
        0
    } else {
        15 - (value.leading_zeros() >> 3)
    }
}

fn get_all_input(node: &Node) -> (u8, u8) {
    let input_power = last_index_positive(&node.default_inputs.ss_counts) as u8;

    let side_input_power = last_index_positive(&node.side_inputs.ss_counts) as u8;

    (input_power, side_input_power)
}

// This function is optimized for input values from 0 to 15 and does not work correctly outside that
// range
pub fn calculate_comparator_output(
    mode: ComparatorMode,
    input_strength: u8,
    power_on_sides: u8,
) -> u8 {
    let difference = input_strength.wrapping_sub(power_on_sides);
    if difference <= 15 {
        match mode {
            ComparatorMode::Compare => input_strength,
            ComparatorMode::Subtract => difference,
        }
    } else {
        0
    }
}

impl fmt::Display for DirectState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "digraph {{")?;
        for (id, node) in self.nodes.inner().iter().enumerate() {
            if matches!(node.ty, NodeType::Wire) {
                continue;
            }
            let label = match node.ty {
                NodeType::Repeater { delay, .. } => format!("Repeater({})", delay),
                NodeType::Torch => "Torch".to_string(),
                NodeType::Comparator { mode, .. } => format!(
                    "Comparator({})",
                    match mode {
                        ComparatorMode::Compare => "Cmp",
                        ComparatorMode::Subtract => "Sub",
                    }
                ),
                NodeType::Lamp => "Lamp".to_string(),
                NodeType::Button => "Button".to_string(),
                NodeType::Lever => "Lever".to_string(),
                NodeType::PressurePlate => "PressurePlate".to_string(),
                NodeType::Trapdoor => "Trapdoor".to_string(),
                NodeType::Wire => "Wire".to_string(),
                NodeType::Constant => format!("Constant({})", node.output_power),
                NodeType::NoteBlock { .. } => "NoteBlock".to_string(),
            };
            let pos = if self.blocks[id].len() > 0 {
                let mut string = String::new();
                for (pos, _) in self.blocks[id].iter() {
                    write!(&mut string, "{}, {}, {}; ", pos.x, pos.y, pos.z)?;
                }
                string
            } else {
                "No Pos".to_string()
            };
            writeln!(f, "    n{} [ label = \"{}\\n({})\" ];", id, label, pos)?;
            for link in &self.forward_links[node.fwd_link_begin..node.fwd_link_end] {
                let out_index = link.node().index();
                let distance = link.ss();
                let color = if link.side() { ",color=\"blue\"" } else { "" };
                writeln!(
                    f,
                    "    n{} -> n{} [ label = \"{}\"{} ];",
                    id, out_index, distance, color
                )?;
            }
        }
        writeln!(f, "}}")
    }
}
