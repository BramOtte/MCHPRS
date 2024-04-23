//! # [`IdentifyNodes`]
//!
//! This pass populates the graph with nodes using the input given in [`CompilerInput`].
//! This pass is *mandatory*. Without it, the graph will never be populated.
//!
//! If `optimize` is set in [`CompilerOptions`], redstone wires will not be added to the graph.
//!
//! There are no requirements for this pass.

use super::Pass;
use crate::redpiler::compile_graph::{
    Annotations, CompileGraph, CompileNode, LinkType, NodeIdx, NodeState, NodeType
};
use crate::redpiler::{BackendVariant, CompilerInput, CompilerOptions};
use crate::redstone::{self, comparator, noteblock};
use crate::redstone::comparator::get_far_input;
use crate::world::{for_each_block_optimized, World};
use itertools::Itertools;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockDirection, BlockFace, BlockPos};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::Value;
use petgraph::Direction;
use std::str::FromStr;
use tracing::warn;

pub struct IdentifyNodes;

impl<W: World> Pass<W> for IdentifyNodes {
    fn run_pass(
        &self,
        graph: &mut CompileGraph,
        options: &CompilerOptions,
        input: &CompilerInput<'_, W>,
    ) {
        let ignore_wires = options.optimize;
        let plot = input.world;

        let mut first_pass = FxHashMap::default();
        let mut second_pass = FxHashSet::default();

        let (first_pos, second_pos) = input.bounds;

        for_each_block_optimized(plot, first_pos, second_pos, |pos| {
            for_pos(
                graph,
                &mut first_pass,
                &mut second_pass,
                ignore_wires,
                plot,
                pos,
            );
        });

        for pos in second_pass {
            apply_annotations(graph, options, &first_pass, plot, pos);
        }
    }

    fn should_run(&self, _: &CompilerOptions) -> bool {
        // Mandatory
        true
    }

    fn status_message(&self) -> &'static str {
        "Identifying nodes"
    }
}

fn for_pos<W: World>(
    graph: &mut CompileGraph,
    first_pass: &mut FxHashMap<BlockPos, NodeIdx>,
    second_pass: &mut FxHashSet<BlockPos>,
    ignore_wires: bool,
    world: &W,
    pos: BlockPos,
) {
    let id = world.get_block_raw(pos);
    let block = Block::from_id(id);

    if matches!(block, Block::Sign { .. } | Block::WallSign { .. }) {
        second_pass.insert(pos);
        return;
    }

    let Some((ty, state)) = identify_block(block, pos, world) else {
        return;
    };

    let is_input = matches!(
        ty,
        NodeType::Button | NodeType::Lever | NodeType::PressurePlate
    );
    let is_output = matches!(
        ty,
        NodeType::Trapdoor | NodeType::Lamp | NodeType::NoteBlock { .. }
    );
    // || matches!(block, Block::RedstoneWire { wire } if wire::is_dot(wire));

    if ignore_wires && ty == NodeType::Wire && !(is_input | is_output) {
        return;
    }

    let node_idx = graph.add_node(CompileNode {
        ty,
        block: Some((pos, id)),
        state,

        is_input,
        is_output,
        annotations: Annotations::default(),
    });
    first_pass.insert(pos, node_idx);
}

fn identify_block<W: World>(
    block: Block,
    pos: BlockPos,
    world: &W,
) -> Option<(NodeType, NodeState)> {
    let (ty, state) = match block {
        Block::RedstoneRepeater { repeater } => (
            NodeType::Repeater {
                delay: repeater.delay,
                facing_diode: redstone::is_diode(
                    world.get_block(pos.offset(repeater.facing.opposite().block_face())),
                ),
            },
            NodeState::repeater(repeater.powered, repeater.locked),
        ),
        Block::RedstoneComparator { comparator } => (
            NodeType::Comparator {
                mode: comparator.mode,
                far_input: get_far_input(world, pos, comparator.facing),
                facing_diode: redstone::is_diode(
                    world.get_block(pos.offset(comparator.facing.opposite().block_face())),
                ),
            },
            NodeState::comparator(
                comparator.powered,
                if let Some(BlockEntity::Comparator { output_strength }) =
                    world.get_block_entity(pos)
                {
                    *output_strength
                } else {
                    0
                },
            ),
        ),
        Block::RedstoneTorch { lit, .. } | Block::RedstoneWallTorch { lit, .. } => {
            (NodeType::Torch, NodeState::simple(lit))
        }
        Block::RedstoneWire { wire } => (NodeType::Wire, NodeState::ss(wire.power)),
        Block::StoneButton { button } => (NodeType::Button, NodeState::simple(button.powered)),
        Block::RedstoneLamp { lit } => (NodeType::Lamp, NodeState::simple(lit)),
        Block::Lever { lever } => (NodeType::Lever, NodeState::simple(lever.powered)),
        Block::StonePressurePlate { powered } => {
            (NodeType::PressurePlate, NodeState::simple(powered))
        }
        Block::IronTrapdoor { powered, .. } => (NodeType::Trapdoor, NodeState::simple(powered)),
        Block::RedstoneBlock {} => (NodeType::Constant, NodeState::ss(15)),
        Block::NoteBlock {
            instrument: _,
            note,
            powered,
        } if noteblock::is_noteblock_unblocked(world, pos) => {
            let instrument = noteblock::get_noteblock_instrument(world, pos);
            (
                NodeType::NoteBlock { instrument, note },
                NodeState::simple(powered),
            )
        }
        block if comparator::has_override(block) => (
            NodeType::Constant,
            NodeState::ss(comparator::get_override(block, world, pos)),
        ),
        _ => return None,
    };
    Some((ty, state))
}

fn apply_annotations<W: World>(
    graph: &mut CompileGraph,
    options: &CompilerOptions,
    first_pass: &FxHashMap<BlockPos, NodeIdx>,
    world: &W,
    pos: BlockPos,
) {
    let block = world.get_block(pos);
    let annotations = parse_sign_annotations(world.get_block_entity(pos));
    if annotations.is_empty() {
        return;
    }

    let targets = match block {
        Block::Sign { rotation, .. } => {
            if let Some(facing) = BlockDirection::from_rotation(rotation) {
                let behind = pos.offset(facing.opposite().block_face());
                vec![behind]
            } else {
                warn!("Found sign with annotations, but bad rotation at {}", pos);
                return;
            }
        }
        Block::WallSign { facing, .. } => {
            let behind = pos.offset(facing.opposite().block_face());
            vec![
                behind,
                behind.offset(BlockFace::Top),
                behind.offset(BlockFace::Bottom),
            ]
        }
        _ => panic!("Block unimplemented for second pass"),
    };

    let target = targets.iter().flat_map(|pos| first_pass.get(pos)).next();
    if let Some(&node_idx) = target {
        for annotation in annotations {
            let result = annotation.apply(graph, node_idx, options);
            if let Err(msg) = result {
                warn!("{} at {}", msg, pos);
            }
        }
    } else {
        warn!("Could not find component for annotation at {}", pos);
    }
}

fn parse_sign_annotations(entity: Option<&BlockEntity>) -> Vec<NodeAnnotation> {
    if let Some(BlockEntity::Sign(sign)) = entity {
        sign.rows
            .iter()
            .flat_map(|row| serde_json::from_str(row))
            .flat_map(|json: Value| {
                NodeAnnotation::from_str(json.as_object()?.get("text")?.as_str()?).ok()
            })
            .collect_vec()
    } else {
        vec![]
    }
}

pub enum NodeAnnotation {
    Separate { delay: u32 },
}

impl NodeAnnotation {
    fn apply(
        self,
        graph: &mut CompileGraph,
        node_idx: NodeIdx,
        options: &CompilerOptions,
    ) -> Result<(), String> {
        match self {
            NodeAnnotation::Separate { delay } => {
                let has_side_input = graph
                    .edges_directed(node_idx, Direction::Incoming)
                    .any(|edge| edge.weight().ty == LinkType::Side);
                if has_side_input {
                    return Err(format!(
                        "Separate annotation does not allow for side inputs"
                    ));
                }
                let node = &mut graph[node_idx];
                if !matches!(node.ty, NodeType::Comparator { .. }) {
                    return Err(format!(
                        "Separate annotation only allowed on comparators, not {:?}",
                        node.ty
                    ));
                }
                if node.annotations.separate != None {
                    return Err(format!("Separate annotation was already present"));
                }
                if options.backend_variant == BackendVariant::Threading {
                    node.annotations.separate = Some(delay);
                }
                Ok(())
            }
        }
    }
}

impl FromStr for NodeAnnotation {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().to_ascii_lowercase();
        if s.starts_with('[') && s.ends_with(']') {
            let parts = s[1..s.len() - 1].split(' ').collect_vec();
            match parts.as_slice() {
                ["separate", delay_str] => delay_str
                    .parse::<u32>()
                    .map(|delay| NodeAnnotation::Separate { delay })
                    .map_err(|_| ()),
                _ => Err(()),
            }
        } else {
            Err(())
        }
    }
}
