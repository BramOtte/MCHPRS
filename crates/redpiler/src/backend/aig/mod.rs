mod passes;

use std::{fs::File, io::Write, process::Command, sync::Arc};

use aigrs::networks::aiger::{Aiger, And};
use mchprs_blocks::{blocks::Block, BlockPos};
use mchprs_world::{TickEntry, World};
use passes::contruct::PetAigData;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use rayon::prelude::*;

use crate::{block_powered_mut, compile_graph::{CompileGraph, CompileLink, CompileNode, NodeState, NodeType}, CompilerOptions, TaskMonitor};

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
    layers: Vec<u32>,
    state: StateB,
    pos_to_input: FxHashMap<BlockPos, u32>,
    input_to_pos: Vec<BlockPos>,
    output_to_pos: Vec<BlockPos>,
}

impl Default for AigBackend {
    fn default() -> Self {
        let aig = Aiger::new(0, 0, 0, 0);
        let state = StateB::new(&aig);

        Self { aig, state, output_to_pos: Default::default(), pos_to_input: Default::default(), input_to_pos: Default::default(), layers: Vec::new() }
    }
}

impl AigBackend {}

#[test]
fn test() {
    fn nod(ty: crate::compile_graph::NodeType, powered: bool) -> CompileNode {
        let mut nod = CompileNode {
            ty: ty.clone(),
            block: None,
            state: NodeState::simple(powered),
            is_input: false,
            is_output: false,
            annotations: Default::default(),
        };
        match ty {
            NodeType::Lever => nod.is_input = true,
            NodeType::Lamp => nod.is_output = true,
            _ => {}
        }
        nod
    }
    
    let mut graph = CompileGraph::new();
    
    let a = graph.add_node(nod(NodeType::Lever, false));
    let b = graph.add_node(nod(NodeType::Lever, false));
    // let c = graph.add_node(nod(NodeType::Lever));

    let an = graph.add_node(nod(NodeType::Torch, true));
    let bn = graph.add_node(nod(NodeType::Torch, true));

    let ab = graph.add_node(nod(NodeType::Torch, false));

    let o = graph.add_node(nod(NodeType::Lamp, false));

    graph.add_edge(a, an, CompileLink::default(0));
    graph.add_edge(b, bn, CompileLink::default(0));
    
    graph.add_edge(an, ab, CompileLink::default(0));
    graph.add_edge(bn, ab, CompileLink::default(0));

    graph.add_edge(ab, o, CompileLink::default(0));


    let ticks = Vec::new();
    let options = CompilerOptions::default();
    let monitor = Arc::new(TaskMonitor::default());

    let mut backend = AigBackend::default();
    backend.compile(graph, ticks, &options, monitor);
    println!("{:?}", backend.aig.outputs);
    
    for _ in 0..3 {
        backend.tick();
        println!("{:?}", backend.state.states);
        // println!("{:?}", backend.state.pos(&backend.aig));
    }
    backend.state.states[1] = true;
    backend.state.states[2] = false;
    println!("true true");


    for _ in 0..3 {
        backend.tick();
        println!("{:?}", backend.state.states);
        // println!("{:?}", backend.state.pos(&backend.aig));
    }
}

impl JITBackend for AigBackend {
    fn compile(
        &mut self,
        graph: CompileGraph,
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        monitor: Arc<TaskMonitor>,
    ) {
        let PetAigData {
            graph,
            input_to_pos,
            pos_to_input,
            output_to_pos,
            input_state,
            output_state,
        } = passes::contruct::ConstructAig::default().compile(graph, ticks, options, monitor);
        let aig = graph.to_aiger();
        let state = StateB::new(&aig);


        self.aig = aig;
        self.state = state;
        self.output_to_pos = output_to_pos;
        self.pos_to_input = pos_to_input;
        self.input_to_pos = input_to_pos;

        println!("start");
        let mut file = File::create("ok.aig").unwrap();
        self.aig.serialize(&mut file, false).unwrap();
        file.flush().unwrap();
        println!("end");

        // Command::new("")
    }

    fn tick(&mut self) {
        self.state.update_gates(&self.aig);
        self.state.update_latches(&self.aig);
    }

    fn on_use_block(&mut self, pos: BlockPos) {
        println!("Use block {:?}", pos);
        let Some(&input) = self.pos_to_input.get(&pos) else {
            println!("Failed to use block {:?}", pos);
            return;
        };

        self.state.states[1 + input as usize] ^= true;

        println!("{:?}", self.state.states);
    }

    fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        // todo!()
    }

    fn flush<W: World>(&mut self, world: &mut W, io_only: bool) {
        println!("FLUSH");
        // for (i, output) in self.state {

        for (i, input) in self.state.pis(&self.aig).iter().copied().enumerate() {
            let pos = self.input_to_pos[i];
            let mut block = world.get_block(pos);
            // println!("input{} {} {:?} {:?}", i, input, pos, block);

            if let Some(powered) = block_powered_mut(&mut block) {
                *powered = input;
            }

            world.set_block(pos, block);
        }

        for (i, output) in self.state.pos(&self.aig).enumerate() {
            let pos = self.output_to_pos[i];
            let mut block = world.get_block(pos);

            // println!("output{} {} {:?} {:?}", i, output, pos, block);


            if let Some(powered) = block_powered_mut(&mut block) {
                *powered = output;
            }

            world.set_block(pos, block);
        }
        // todo!()
        
    }

    fn reset<W: World>(&mut self, world: &mut W, io_only: bool) {
        // todo!()
    }

    fn has_pending_ticks(&self) -> bool {
        // todo!()
        true
    }

    #[doc = " Inspect block for debugging"]
    fn inspect(&mut self, pos: BlockPos) {
        // todo!()
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

    pub fn pos<'a>(&'a self, g: &'a Aiger) -> impl Iterator<Item = bool> + 'a {
        g.outputs.iter().skip(g.latch_count()).copied().map(|output| {
            // println!("output {} {}", output.index(), output.sign());
            self.states[output.index()] ^ output.sign()
        })
    }

    pub fn par_update_gates(&mut self, g: &Aiger, layers: &[u32]) {
        // let states = 
        for layer in layers.windows(2) {
            let start = layer[0] as usize;
            let end = layer[1] as usize;

            let (input, output) = self.states.split_at_mut(start);

            (&mut output[..end-start]).into_par_iter().enumerate().for_each(|(i, state)| {
                let And(rhs0, rhs1) = g.ands[i];
                *state =
                    (input[rhs0.index()] ^ rhs0.sign())
                    & (input[rhs1.index()] ^ rhs1.sign());                
            });
        }
    }

    pub fn update_gates(&mut self, g: &Aiger) {
        for i in g.iter_and_nodes() {
            let And(rhs0, rhs1) = g.ands[i];
            // println!("({:?}, {}) ({:?}, {}) -> {}", rhs0.index(), rhs0.sign(), rhs1.index(), rhs1.sign(), i);
            self.states[i] = 
                (self.states[rhs0.index()] ^ rhs0.sign())
                & (self.states[rhs1.index()] ^ rhs1.sign());
        }
    }
    // pub fn par_update_latches(&mut self, g: &Aiger) {
    //     let (input, )
    // }

    pub fn update_latches(&mut self, g: &Aiger) {
        for i in 0..g.latch_count() {
            let output = g.outputs[i];
            // println!("output {} {}", output.index(), output.sign());
            self.states[i + g.start_latches] = self.states[output.index()] ^ output.sign();
        }
    }
}