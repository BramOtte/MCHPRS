// The backend will use rayon to loop over the to be ticked nodes in parallel
// All the nodes that share outputs are ticked in the same thread to protect their signal strength buckets
// These groups of nodes that share outputs each have their own tick queue
// When source and target nodes are scheduled to tick at the same time the target node is ticked in the same thread as the source node to protect the tick queue
// I am not entirely sure how to put the 2 sepperate tick queues in the right order perhaps we could keep an ordering number in each tick queue entry and join them back together like you would in merge sort


mod compile;
mod node;
mod tick;
mod update;

use super::JITBackend;
use crate::compile_graph::CompileGraph;
use crate::task_monitor::TaskMonitor;
use crate::{block_powered_mut, CompilerOptions};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ComparatorMode, Instrument};
use mchprs_blocks::BlockPos;
use mchprs_redstone::{bool_to_ss, noteblock};
use mchprs_world::World;
use mchprs_world::{TickEntry, TickPriority};
use node::{Group, Node, NodeId, NodeType, Nodes};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::{fmt, mem};
use tracing::{debug, warn};
use rayon::prelude::*;

#[derive(Default, Clone)]
struct Queues([Vec<NodeId>; TickScheduler::NUM_PRIORITIES]);

impl Queues {
    fn drain_iter(&mut self) -> impl Iterator<Item = NodeId> + '_ {
        let [q0, q1, q2, q3] = &mut self.0;
        let [q0, q1, q2, q3] = [q0, q1, q2, q3].map(|q| q.drain(..));
        q0.chain(q1).chain(q2).chain(q3)
    }
}

#[derive(Default)]
struct TickScheduler {
    queues_deque: [Queues; Self::NUM_QUEUES],
}

impl TickScheduler {
    const NUM_PRIORITIES: usize = 4;
    const NUM_QUEUES: usize = 16;

    fn reset<W: World>(&mut self, tick: usize, world: &mut W, blocks: &[Option<(BlockPos, Block)>]) {
        for (idx, queues) in self.queues_deque.iter().enumerate() {
            let delay = if tick >= idx {
                idx + Self::NUM_QUEUES
            } else {
                idx
            } - tick;
            for (entries, priority) in queues.0.iter().zip(Self::priorities()) {
                for node in entries {
                    let Some((pos, _)) = blocks[node.index()] else {
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

    fn schedule_tick(&mut self, tick: usize, node: NodeId, delay: usize, priority: TickPriority) -> u8 {
        let tick = (tick + delay) % Self::NUM_QUEUES;
        self.queues_deque[tick].0[priority as usize].push(node);
        tick as u8
    }

    fn queues_this_tick(&mut self, tick: usize) -> Queues {
        mem::take(&mut self.queues_deque[tick])
    }

    fn end_tick(&mut self, tick: usize, mut queues: Queues) {
        for queue in &mut queues.0 {
            queue.clear();
        }
        self.queues_deque[tick] = queues;
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

#[derive(Default)]
struct Groups {
    pub groups: Vec<Group>,
    // ticks: [Vec<u32>; TickScheduler::NUM_QUEUES],
    tick: usize,
}

impl Groups {
    fn schedule_tick(&mut self, group: u32, node: NodeId, delay: usize, priority: TickPriority) -> u8 {
        let group = &mut self.groups[group as usize];
        group.scheduler.schedule_tick(self.tick, node, delay, priority)
    }

    fn queues_this_tick(&mut self, group: u32) -> Queues {
        let group = &mut self.groups[group as usize];
        group.scheduler.queues_this_tick(self.tick)
    }

    fn end_tick(&mut self, group: u32, queues: Queues) {
        let group = &mut self.groups[group as usize];
        group.scheduler.end_tick(self.tick, queues);
    }

    fn has_pending_ticks(&self) {
        for group in self.groups.iter() {
            group.scheduler.has_pending_ticks();
        }
    }
    
    fn has_next_tick(&self, group: u32) -> bool {
        let tick = (self.tick + 1) % TickScheduler::NUM_QUEUES;
        self.groups[group as usize].scheduler.queues_deque[tick].0.iter().any(|queue| queue.len() > 0)
    }

    fn reset<W: World>(&mut self, world: &mut W, blocks: &[Option<(BlockPos, Block)>]) {
        for group in self.groups.iter_mut() {
            group.scheduler.reset(self.tick, world, blocks);
        }
        self.tick = 0;
    }
}

enum Event {
    NoteBlockPlay { noteblock_id: u16 },
}

#[derive(Default)]
pub struct ThreadedBackend {
    nodes: Nodes,
    groups: Groups,
    blocks: Vec<Option<(BlockPos, Block)>>,
    pos_map: FxHashMap<BlockPos, NodeId>,
    events: Vec<Event>,
    noteblock_info: Vec<(BlockPos, Instrument, u32)>,
}

unsafe impl Sync for ThreadedBackend {}

impl ThreadedBackend {
    fn schedule_tick(&mut self, group: u32, node_id: NodeId, delay: usize, priority: TickPriority) {
        self.groups.schedule_tick(group, node_id, delay, priority);
    }

    fn set_node(&mut self, priority: TickPriority, node_id: NodeId, powered: bool, new_power: u8) {
        let node = &mut self.nodes[node_id];
        let old_power = node.output_power;

        node.changed = true;
        node.powered = powered;
        node.output_power = new_power;
        for i in 0..node.updates.len() {
            let node = &self.nodes[node_id];

            // Unhappy path if input and output tick together
            let tick_output = node.pending_tick == self.groups.tick as u8;
            if tick_output && (node.pending_tick_priority as u8) < (priority as u8) {
                self.tick_node(node.pending_tick_priority, node.group_id, node_id);
            }

            let node = &self.nodes[node_id];

            let update_link = unsafe { *node.updates.get_unchecked(i) };
            let side = update_link.side();
            let distance = update_link.ss();
            let update = update_link.node();

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

            // Safety: signal strength is never larger than 15
            unsafe {
                *inputs.ss_counts.get_unchecked_mut(old_power as usize) -= 1;
                *inputs.ss_counts.get_unchecked_mut(new_power as usize) += 1;
            }

            update::update_node(
                &mut self.groups,
                &mut self.events,
                &mut self.nodes,
                update,
            );

            let node = &self.nodes[node_id];

            if tick_output && (node.pending_tick_priority as u8) >= (priority as u8) {
                self.tick_node(node.pending_tick_priority, node.group_id, node_id);
            }
        }
    }
}

impl JITBackend for ThreadedBackend {
    fn inspect(&mut self, pos: BlockPos) {
        let Some(node_id) = self.pos_map.get(&pos) else {
            debug!("could not find node at pos {}", pos);
            return;
        };

        debug!("Node {:?}: {:#?}", node_id, self.nodes[*node_id]);
    }

    fn reset<W: World>(&mut self, world: &mut W, io_only: bool) {
        self.groups.reset(world, &self.blocks);

        let nodes = std::mem::take(&mut self.nodes);

        for (i, node) in nodes.into_inner().iter().enumerate() {
            let Some((pos, block)) = self.blocks[i] else {
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
                self.schedule_tick(node.group_id, node_id, 10, TickPriority::Normal);
                self.set_node(TickPriority::Highest, node_id, true, 15);
            }
            NodeType::Lever => {
                self.set_node(TickPriority::Highest, node_id, !node.powered, bool_to_ss(!node.powered));
            }
            _ => warn!("Tried to use a {:?} redpiler node", node.ty),
        }
    }

    fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        let node_id = self.pos_map[&pos];
        let node = &self.nodes[node_id];
        match node.ty {
            NodeType::PressurePlate => {
                self.set_node(TickPriority::Highest, node_id, powered, bool_to_ss(powered));
            }
            _ => warn!("Tried to set pressure plate state for a {:?}", node.ty),
        }
    }

    fn tick(&mut self) {
        self.groups.tick = (self.groups.tick + 1) % TickScheduler::NUM_QUEUES;
        let current_tick = self.groups.tick;
        let next_tick = current_tick % TickScheduler::NUM_QUEUES;

        let backend = self as *mut Self as usize;
        (0..self.groups.groups.len()).into_par_iter().for_each(|group_id| {
            // FIXME: Very nasty unsafe here
            let backend = unsafe {
                &mut *(backend as *mut Self)   
            };
            let mut queues = backend.groups.queues_this_tick(group_id as u32);

            for priority in TickScheduler::priorities() {
                for node_id in queues.0[priority as usize].drain(..) {
                    if let Some(input_group) = backend.nodes[node_id].input_group_id {
                        // Unhappy path if input and output tick together
                        if backend.groups.groups[input_group as usize].tick[current_tick % 2] {
                            continue;
                        }
                    }

                    backend.tick_node(priority, group_id as u32, node_id);
                }
            }

            let group = &mut backend.groups.groups[group_id];
            group.scheduler.end_tick(current_tick, queues);
            group.tick[(current_tick + 1) % 2] = 
                group
                .scheduler
                .queues_deque[next_tick].0
                .iter()
                .any(|queue| queue.len() > 0);
        });

        
    }

    fn flush<W: World>(&mut self, world: &mut W, io_only: bool) {
        for event in self.events.drain(..) {
            match event {
                Event::NoteBlockPlay { noteblock_id } => {
                    let (pos, instrument, note) = self.noteblock_info[noteblock_id as usize];
                    noteblock::play_note(world, pos, instrument, note);
                }
            }
        }
        for (i, node) in self.nodes.inner_mut().iter_mut().enumerate() {
            let Some((pos, block)) = &mut self.blocks[i] else {
                continue;
            };
            if node.changed && (!io_only || node.is_io) {
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
            node.changed = false;
        }
    }

    fn compile(
        &mut self,
        graph: CompileGraph,
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        monitor: Arc<TaskMonitor>,
    ) {
        compile::compile(self, graph, ticks, options, monitor);
    }

    fn has_pending_ticks(&self) -> bool {
        self.groups.groups.iter().any(|group| group.scheduler.has_pending_ticks())
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
    scheduler: &mut Groups,
    group_id: u32,
    node_id: NodeId,
    node: &mut Node,
    delay: usize,
    priority: TickPriority,
) {
    node.pending_tick = scheduler.schedule_tick(group_id, node_id, delay, priority) as u8;
    node.pending_tick_priority = priority;
}

const BOOL_INPUT_MASK: u128 = u128::from_ne_bytes([
    0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
]);

fn get_bool_input(node: &Node) -> bool {
    u128::from_le_bytes(node.default_inputs.ss_counts) & BOOL_INPUT_MASK != 0
}

fn get_bool_side(node: &Node) -> bool {
    u128::from_le_bytes(node.side_inputs.ss_counts) & BOOL_INPUT_MASK != 0
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

// This function is optimized for input values from 0 to 15 and does not work correctly outside that range
fn calculate_comparator_output(mode: ComparatorMode, input_strength: u8, power_on_sides: u8) -> u8 {
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

impl fmt::Display for ThreadedBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "digraph {{")?;
        for (id, node) in self.nodes.inner().iter().enumerate() {
            if matches!(node.ty, NodeType::Wire) {
                continue;
            }
            let label = match node.ty {
                NodeType::Repeater { delay, .. } => format!("Repeater({})", delay),
                NodeType::Torch => format!("Torch"),
                NodeType::Comparator { mode, .. } => format!(
                    "Comparator({})",
                    match mode {
                        ComparatorMode::Compare => "Cmp",
                        ComparatorMode::Subtract => "Sub",
                    }
                ),
                NodeType::Lamp => format!("Lamp"),
                NodeType::Button => format!("Button"),
                NodeType::Lever => format!("Lever"),
                NodeType::PressurePlate => format!("PressurePlate"),
                NodeType::Trapdoor => format!("Trapdoor"),
                NodeType::Wire => format!("Wire"),
                NodeType::Constant => format!("Constant({})", node.output_power),
                NodeType::NoteBlock { .. } => format!("NoteBlock"),
            };
            let pos = if let Some((pos, _)) = self.blocks[id] {
                format!("{}, {}, {}", pos.x, pos.y, pos.z)
            } else {
                "No Pos".to_string()
            };
            writeln!(f, "    n{} [ label = \"{}\\n({})\" ];", id, label, pos)?;
            for link in node.updates.iter() {
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
