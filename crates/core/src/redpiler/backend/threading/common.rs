use mchprs_blocks::blocks::ComparatorMode;
use std::fmt::Debug;
use std::num::NonZeroU8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct NonMaxU8(NonZeroU8);

impl NonMaxU8 {
    pub fn new(n: u8) -> Option<Self> {
        Some(Self(NonZeroU8::new(n.wrapping_add(1))?))
    }

    pub fn get(self) -> u8 {
        self.0.get() - 1
    }
}

pub(super) fn calculate_comparator_output(
    mode: ComparatorMode,
    input_strength: u8,
    power_on_sides: u8,
) -> u8 {
    match mode {
        ComparatorMode::Compare => {
            if input_strength >= power_on_sides {
                input_strength
            } else {
                0
            }
        }
        ComparatorMode::Subtract => input_strength.saturating_sub(power_on_sides),
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct NodeId(u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// Safety: index must be within bounds of nodes array
    pub unsafe fn from_index(index: usize) -> NodeId {
        NodeId(index as u32)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NodeIdWithData {
    data: u32,
}

impl NodeIdWithData {
    pub fn new(node_id: NodeId) -> Self {
        debug_assert!(node_id.index() < (1 << 27));
        Self {
            data: (node_id.index() as u32) << 5,
        }
    }

    pub fn new_with_data(node_id: NodeId, internal_tick: bool, ss: u8) -> Self {
        debug_assert!(node_id.index() < (1 << 27));
        debug_assert!(ss < (1 << 4));
        Self {
            data: (node_id.index() as u32) << 5
                | if internal_tick { 1 << 4 } else { 0 }
                | ss as u32,
        }
    }

    pub fn node(self) -> NodeId {
        unsafe {
            // safety: ForwardLink is contructed using a NodeId
            NodeId::from_index((self.data >> 5) as usize)
        }
    }

    pub fn internal_tick(self) -> bool {
        self.data & (1 << 4) != 0
    }

    pub fn ss(self) -> u8 {
        (self.data & 0b1111) as u8
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ForwardLink {
    data: u32,
}

impl ForwardLink {
    pub fn new(id: NodeId, side: bool, ss: u8) -> Self {
        assert!(id.index() < (1 << 27));
        assert!(ss < (1 << 4));
        Self {
            data: (id.index() as u32) << 5 | if side { 1 << 4 } else { 0 } | ss as u32,
        }
    }

    pub fn node(self) -> NodeId {
        unsafe {
            // safety: ForwardLink is contructed using a NodeId
            NodeId::from_index((self.data >> 5) as usize)
        }
    }

    pub fn side(self) -> bool {
        self.data & (1 << 4) != 0
    }

    pub fn ss(self) -> u8 {
        (self.data & 0b1111) as u8
    }
}
