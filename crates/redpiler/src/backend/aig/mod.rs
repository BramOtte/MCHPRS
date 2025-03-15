mod passes;

use std::sync::Arc;

use aigrs::networks::aiger::{Aiger, And};
use mchprs_blocks::BlockPos;
use mchprs_world::{TickEntry, World};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::{compile_graph::CompileGraph, CompilerOptions, TaskMonitor};

use super::JITBackend;

// #[repr(C)]
// struct SmallBox<T, const S: usize> {
//     size: u32,
//     data: SmallBoxData<T, S>,
// }

// union SmallBoxData<T, const S: usize> {
//     inline: std::mem::ManuallyDrop<[T; S]>,
//     heap: *mut T,
// }

// struct Link {

// }
struct OpLink {
    node: u32,
    delay: u8,
    ss: u8,
    inverted: bool,
    side: bool,
}

struct Node {
    updates: SmallVec<[OpLink; 8]>,
    updated: bool,
    timeout: usize,
}

// struct Comp {
//     inputs:
// }

pub struct AigBackend {
    aig: aigrs::networks::aiger::Aiger,
    state: StateB,
    node_to_pos: Vec<BlockPos>,
    pos_to_node: FxHashMap<BlockPos, u32>,
}

impl AigBackend {}

impl JITBackend for AigBackend {
    fn compile(
        &mut self,
        graph: CompileGraph,
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        monitor: Arc<TaskMonitor>,
    ) {
        let aig = passes::contruct::ConstructAig::default().compile(graph, ticks, options, monitor);
        let aig = aig.to_aiger();
        // let state = StateB::new(&aig);
        todo!()
    }

    fn tick(&mut self) {
        todo!()
    }

    fn on_use_block(&mut self, pos: BlockPos) {
        todo!()
    }

    fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        todo!()
    }

    fn flush<W: World>(&mut self, world: &mut W, io_only: bool) {
        todo!()
    }

    fn reset<W: World>(&mut self, world: &mut W, io_only: bool) {
        todo!()
    }

    fn has_pending_ticks(&self) -> bool {
        todo!()
    }

    #[doc = " Inspect block for debugging"]
    fn inspect(&mut self, pos: BlockPos) {
        todo!()
    }
}


pub struct StateB {
    pub states: Vec<bool>,
}

impl StateB {
    pub fn new(g: &Aiger) -> Self {
        Self { states: vec![false; g.ci_count()+g.and_count()+1] }
    }
    pub fn pis(&mut self, g: &Aiger) -> &mut [bool] {
        &mut self.states[1..1+g.pi_count()]
    }

    pub fn pos(&mut self, g: &Aiger) -> Vec<bool> {
        g.outputs.iter().copied().map(|output| {
            self.states[output.index()] ^ output.sign()
        }).collect()
    }

    pub fn step(&mut self, g: &Aiger) {
        for i in g.iter_and_nodes() {
            let And(rhs0, rhs1) = g.ands[i];
            self.states[i] = 
                (self.states[rhs0.index()] ^ rhs0.sign())
                & (self.states[rhs1.index()] ^ rhs1.sign());
        }
    }
    pub fn update_latches(&mut self, g: &Aiger) {
        for i in 0..g.latch_count() {
            let output = g.outputs[i];
            self.states[i+1] = self.states[output.index()] ^ output.sign();
        }
    }
}