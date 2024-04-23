use mchprs_blocks::blocks::{ComparatorMode, Instrument};
use mchprs_blocks::BlockPos;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use rustc_hash::FxHashSet;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt::Display;

pub type NodeIdx = NodeIndex;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeType {
    Repeater {
        delay: u8,
        facing_diode: bool,
    },
    Torch,
    Comparator {
        mode: ComparatorMode,
        far_input: Option<u8>,
        facing_diode: bool,
    },
    Lamp,
    Button,
    Lever,
    PressurePlate,
    Trapdoor,
    Wire,
    Constant,
    NoteBlock {
        instrument: Instrument,
        note: u32,
    },
    ComparatorLine {
        states: Box<[u8]>,
    },
    ExternalInput,
    ExternalOutput {
        target_idx: NodeIdx,
        delay: u32,
    },
}

#[derive(Debug, Clone, Default)]
pub struct NodeState {
    pub powered: bool,
    pub repeater_locked: bool,
    pub output_strength: u8,
}

impl NodeState {
    pub fn simple(powered: bool) -> NodeState {
        NodeState {
            powered,
            output_strength: if powered { 15 } else { 0 },
            ..Default::default()
        }
    }

    pub fn repeater(powered: bool, locked: bool) -> NodeState {
        NodeState {
            powered,
            repeater_locked: locked,
            output_strength: if powered { 15 } else { 0 },
        }
    }

    pub fn ss(ss: u8) -> NodeState {
        NodeState {
            output_strength: ss,
            ..Default::default()
        }
    }

    pub fn comparator(powered: bool, ss: u8) -> NodeState {
        NodeState {
            powered,
            output_strength: ss,
            ..Default::default()
        }
    }
}

#[derive(Debug, Default)]
pub struct Annotations {
    pub separate: Option<u32>,
}

#[derive(Debug)]
pub struct CompileNode {
    pub ty: NodeType,
    pub block: Option<(BlockPos, u32)>,
    pub state: NodeState,

    pub is_input: bool,
    pub is_output: bool,
    pub annotations: Annotations,
}

impl CompileNode {
    pub fn is_removable(&self) -> bool {
        !self.is_input && !self.is_output && self.annotations.separate == None
    }

    pub fn is_io_and_flushable(&self) -> bool {
        (self.is_input || self.is_output) && self.block.is_some()
    }

    pub fn can_be_ticked(&self) -> bool {
        matches!(
            self.ty,
            NodeType::Repeater { .. }
                | NodeType::Comparator { .. }
                | NodeType::Torch
                | NodeType::Button
                | NodeType::Lamp
                | NodeType::ComparatorLine { .. }
        )
    }

    pub fn is_analog(&self) -> bool {
        matches!(
            self.ty,
            NodeType::Comparator { .. }
                | NodeType::Wire
                | NodeType::ExternalOutput { .. }
                | NodeType::ComparatorLine { .. }
        )
    }
}

impl Display for CompileNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self.ty {
                NodeType::Repeater { delay, .. } => format!("Repeater({})", delay),
                NodeType::Torch => format!("Torch"),
                NodeType::Comparator { mode, .. } =>
                    format!("Comparator({})", match mode {ComparatorMode::Compare=>"Cmp",ComparatorMode::Subtract=>"Sub",}),
                NodeType::Lamp => format!("Lamp"),
                NodeType::Button => format!("Button"),
                NodeType::Lever => format!("Lever"),
                NodeType::PressurePlate => format!("PressurePlate"),
                NodeType::Trapdoor => format!("Trapdoor"),
                NodeType::Wire => format!("Wire"),
                NodeType::Constant => format!("Constant"),
                NodeType::ExternalInput => format!("ExternalInput"),
                NodeType::ExternalOutput { .. } => format!("ExternalOutput"),
                NodeType::ComparatorLine { states } => format!("ComparatorLine({})", states.len()),
                NodeType::NoteBlock { instrument, note } => todo!(),
            }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkType {
    Default,
    Side,
}

#[derive(Debug)]
pub struct CompileLink {
    pub ty: LinkType,
    pub ss: u8,
}

impl CompileLink {
    pub fn new(ty: LinkType, ss: u8) -> CompileLink {
        CompileLink { ty, ss }
    }

    pub fn default(ss: u8) -> CompileLink {
        CompileLink {
            ty: LinkType::Default,
            ss,
        }
    }

    pub fn side(ss: u8) -> CompileLink {
        CompileLink {
            ty: LinkType::Side,
            ss,
        }
    }
}

impl Display for CompileLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}",
            match self.ty {
                LinkType::Default => "",
                LinkType::Side => "S",
            },
            self.ss
        )
    }
}

pub type CompileGraph = StableGraph<CompileNode, CompileLink>;

pub fn weakly_connected_components(graph: &CompileGraph) -> Vec<Vec<NodeIdx>> {
    let mut visited = FxHashSet::with_capacity_and_hasher(graph.node_count(), Default::default());
    let mut components = vec![];

    for node in graph.node_indices() {
        if !visited.contains(&node) {
            visited.insert(node);

            let mut component = vec![node];
            let mut index = 0;
            while component.len() > index {
                for neighbor in graph.neighbors_undirected(component[index]) {
                    if !visited.contains(&neighbor) {
                        visited.insert(neighbor);
                        component.push(neighbor);
                    }
                }
                index += 1;
            }
            components.push(component);
        }
    }
    components
}

// Merge groups into `k` bins using a greedy algorithm for the multiway number partitioning problem.
// TODO: In the future a higher quality algorithm should be used here.
pub fn multiway_number_partitioning(mut groups: Vec<Vec<NodeIdx>>, k: usize) -> Vec<Vec<NodeIdx>> {
    if groups.len() <= k {
        return groups;
    }
    groups.sort_by_key(|group| Reverse(group.len()));

    let mut bins: BinaryHeap<(Reverse<usize>, Vec<NodeIdx>)> = BinaryHeap::with_capacity(k);
    for _ in 0..k {
        bins.push((Reverse(0), vec![]));
    }

    for group in groups {
        let (_, mut bin) = bins.pop().unwrap();
        bin.extend(group);
        bins.push((Reverse(bin.len()), bin));
    }
    bins.into_iter().map(|(_, bin)| bin).collect()
}
