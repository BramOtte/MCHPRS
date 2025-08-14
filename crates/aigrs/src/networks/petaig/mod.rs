use std::os::unix::process;
use std::time::Instant;
use std::{u32, usize};

use petgraph::Direction;
use petgraph::stable_graph::EdgeReference;
use petgraph::visit::{EdgeRef, IntoEdgesDirected, IntoNodeReferences, NodeIndexable};
use petgraph::Direction::{Incoming, Outgoing};

use super::aiger::{Aiger};
use super::{aiger, Network};

type PAig = petgraph::stable_graph::StableDiGraph<AigNodeTy, bool, u32>;
type AigIndex = petgraph::stable_graph::NodeIndex<u32>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AigNodeTy {
    And, Input, Output, Latch, LocalInput, False
}


#[derive(Debug, Clone, Copy)]
pub struct AigLit(AigIndex, bool);

impl AigLit {
    pub fn index(&self) -> AigIndex {
        self.0
    }
    pub fn sign(&self) -> bool {
        self.1
    }

    pub fn xor(self, sign: bool) -> Self {
        Self(self.0, self.1 ^ sign)
    }
}

impl std::ops::Not for AigLit {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(self.0, !self.1)
    }
}


#[derive(Debug)]
pub struct NextState(AigIndex);

#[derive(Debug, Clone, Copy)]
pub struct Node(AigIndex);

impl Node {
    pub const fn lit(&self) -> AigLit {
        AigLit(self.0, false)
    }
    pub const fn with_sign(&self, sign: bool) -> AigLit {
        AigLit(self.0, sign)
    }
}

impl <'a> Into<AigLit> for &'a Node {
    fn into(self) -> AigLit {
        AigLit(self.0, false)
    }
}

pub trait AigAdd {
    fn add(self, aig: &mut Aig) -> AigLit;
}

impl AigAdd for AigLit {
    fn add(self, _aig: &mut Aig) -> AigLit {
        self
    }
}

pub struct Aig {
    pub g: PAig,
    f: AigIndex,
}

#[derive(Debug, Default)]
pub struct AigSize {
    pub pi: usize,
    pub po: usize,
    pub ands: usize,
    pub latches: usize,
}


impl Network for Aig {
    type Sig = AigLit;
    type Node = Node;
}

impl Aig {
    pub fn new() -> Self {
        let mut g = PAig::new();

        let f = g.add_node(AigNodeTy::False);

        Self { g, f }
    }

    pub fn size(&self) -> AigSize {
        let mut size = AigSize::default();
        for node in self.g.node_weights() {
            match node {
                AigNodeTy::And => size.ands += 1,
                AigNodeTy::Input => size.pi += 1,
                AigNodeTy::Output => size.po += 1,
                AigNodeTy::Latch => size.latches += 1,
                AigNodeTy::LocalInput => {},
                AigNodeTy::False => {},
            }
        }
        size
    }

    pub fn c(&self, sign: bool) -> AigLit {
        AigLit(self.f, sign)
    }

    pub fn f(&self) -> AigLit {
        self.c(false)
    }

    pub fn t(&self) -> AigLit {
        self.c(true)
    }

    pub fn push<T: AigAdd>(&mut self, gate: T) -> AigLit {
        gate.add(self)
    }

    pub fn input(&mut self) -> AigLit {
        AigLit(self.g.add_node(AigNodeTy::Input), false)
    }

    pub fn output(&mut self, lit: AigLit) {
        let output = self.g.add_node(AigNodeTy::Output);
        self.g.add_edge(lit.0, output, lit.1);
    }

    pub fn local_input(&mut self) -> Node {
        let input = self.g.add_node(AigNodeTy::LocalInput);
        Node(input)
    }

    pub fn latch(&mut self) -> (NextState, AigLit) {
        let node = self.g.add_node(AigNodeTy::Latch);
        (NextState(node), AigLit(node, false))
    }

    pub fn latch2(&mut self, lit: AigLit) -> AigLit {
        let (next_state, state) = self.latch();
        self.connect_drain(next_state, lit);
        state
    }

    pub fn connect_drain(&mut self, drain: NextState, lit: AigLit) {
        self.g.add_edge(lit.0, drain.0, lit.1);
    }

    pub fn replace_node(&mut self, old: Node, new: AigLit) {
        let outputs = self.g.edges_directed(old.0, Outgoing)
        .map(|edge| {
                AigLit(edge.target(), *edge.weight())
            }).collect::<Vec<_>>();

        for output in outputs {
            self.g.add_edge(new.0, output.0, new.1 ^ output.1);
        }

        self.g.remove_node(old.0);
    }

    fn replace_internal(&mut self, old: petgraph::prelude::NodeIndex, new: AigLit) {
        self.replace_node(Node(old), new);
    }

    pub fn andx(&mut self, a: AigLit, b: AigLit, inv: bool) -> AigLit {
        let and = self.g.add_node(AigNodeTy::And);
        self.g.add_edge(a.0, and, a.1);
        self.g.add_edge(b.0, and, b.1);
        return AigLit(and, inv);
    }

    pub fn and(&mut self, a: AigLit, b: AigLit) -> AigLit {
        self.andx(a, b, false)
    }

    pub fn mux(&mut self, mux: AigLit, t: AigLit, f: AigLit) -> AigLit {
        let t = self.and(mux, t);
        let f = self.and(!mux, f);
        self.or(t, f)
    }

    pub fn or(&mut self, a: AigLit, b: AigLit) -> AigLit {
        return !self.and(!a, !b);
    }

    pub fn ors(&mut self, inputs: &[AigLit]) -> AigLit {
        if inputs.len() == 0 {
            return self.f();
        }
        if inputs.len() == 1 {
            return inputs[0];
        }

        let a= self.ors(&inputs[..inputs.len()/2]);
        let b = self.ors(&inputs[inputs.len()/2..]);

        return self.or(a, b);
    }

    pub fn ands(&mut self, inputs: &[AigLit]) -> AigLit {
        if inputs.len() == 0 {
            return self.t();
        }
        if inputs.len() == 1 {
            return inputs[0];
        }

        let a= self.ands(&inputs[..inputs.len()/2]);
        let b = self.ands(&inputs[inputs.len()/2..]);

        return self.and(a, b);
    }

    pub fn gc(&mut self) {
        let mut changed = true;
        let mut i = 0;

        while changed && i < 1_000_000_000 {
            i += self.g.node_bound();
            // println!("bound {} {}", i, self.g.node_bound());
            let start = Instant::now();


            let mut j = 0;
            changed = false;

            for id in 0..self.g.node_bound() {
                let id = petgraph::stable_graph::node_index(id);
                if !self.g.contains_node(id) {
                    continue;
                }
                if self.g[id] != AigNodeTy::And && self.g[id] != AigNodeTy::Latch {
                    continue;
                }

                if self.g.edges_directed(id, Direction::Outgoing).next().is_none() {
                    self.g.remove_node(id);
                    changed = true;
                    j += 1;
                    continue;
                }

                let mut inputs = self.g.edges_directed(id, Direction::Incoming);
                let mut a = inputs.next().unwrap();

                if a.source() == id {
                    continue;
                }

                if self.g[id] == AigNodeTy::Latch {
                    assert!(inputs.next().is_none());
                    if a.source() == self.f {
                        self.replace_internal(id, AigLit(a.source(), *a.weight()));
                        changed = true;
                        j += 1;
                        continue;
                    }
                }

                if self.g[id] != AigNodeTy::And {
                    continue;
                }
                let mut b = inputs.next().unwrap();
                assert!(inputs.next().is_none());

                if b.source() == id {
                    continue;
                }

                if b.source() == self.f {
                    std::mem::swap(&mut a, &mut b);
                }



                if a.source() == self.f {
                    if *a.weight() {
                        self.replace_internal(id, AigLit(b.source(), *b.weight()));
                    } else {
                        self.replace_internal(id, self.f());
                    }
                    changed = true;
                    j += 1;
                    continue;
                }

                if a.source() == b.source(){
                    if a.weight() == b.weight() {
                        self.replace_internal(id, AigLit(a.source(), *a.weight()));
                    } else {
                        self.replace_internal(id, self.f());
                    }
                    changed = true;
                    j += 1;
                    continue;
                }
            }
            let dt = start.elapsed();
            println!("should remove {} in {:?}", j, dt);
        }
    }

    pub fn to_aiger(&self) -> Aiger {
        let mut inputs = Vec::<AigIndex>::with_capacity(self.g.node_count());
        let mut latches = Vec::<AigIndex>::new();
        let mut outputs = Vec::<AigIndex>::new();
        let mut map = vec![0; self.g.node_bound()];
        // std::collections::HashMap::<AigIndex, u32>::new();
        
        inputs.push(self.f);
        
        for (id, &node) in self.g.node_references() {
            match node {
                AigNodeTy::And | AigNodeTy::False => {},
                AigNodeTy::Input | AigNodeTy::LocalInput => {
                    inputs.push(id);
                }
                AigNodeTy::Output => {
                    outputs.push(id);
                },
                AigNodeTy::Latch => {
                    latches.push(id);
                },
            }
        }
        let input_count = inputs.len();
        let latch_count = latches.len();
        
        let mut ands: Vec<aiger::And> = vec![aiger::And(aiger::AigLit::FALSE, aiger::AigLit::FALSE); input_count+latch_count];
        
        {
            let mut queue = inputs;
            queue.extend_from_slice(&latches);
            
            let mut processed = 0;
            const NO_LIT: aiger::AigLit = aiger::AigLit(u32::MAX);
            let mut input_litterals = vec![ NO_LIT; self.g.node_bound()];

            while processed < queue.len() {
                let node = queue[processed];
                
                map[node.index()] = processed as u32;
                
                for edge in self.g.edges_directed(node, Outgoing) {
                    if self.g[edge.target()] != AigNodeTy::And {
                        continue;
                    }
                    let target = edge.target();
                    let sign = *edge.weight();

                    let lit = aiger::AigLit::new(processed, sign);

                    let left = input_litterals[target.index()];
                    if left == NO_LIT {
                        input_litterals[target.index()] = lit;
                        continue;
                    }
                    queue.push(target);
                    
                    ands.push(aiger::And(left, lit));
                }

                processed += 1;
            }
        }

        let outputs = latches.into_iter().chain(outputs).map(|output| {
            let mut inputs = self.g.edges_directed(output, Incoming);
            let edge = inputs.next().unwrap();
            assert_eq!(inputs.next(), None);
            let sign = *edge.weight();
            let source = edge.source();
            let source = map[source.index()];
            
            aiger::AigLit::new(source as usize, sign)
        })
            .collect();

        let mut aig = Aiger {
            ands,
            outputs,
            start_latches: input_count,
            start_gates: input_count + latch_count,
        };

        aig
    }

    pub fn serialize<W: std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        self.to_aiger().serialize(w, false)
    }

}


fn write_var_int<W: std::io::Write>(mut x: usize, w: &mut W) -> std::io::Result<()>  {
    while x > 0 {
        w.write(&[(x & 127) as u8 | if x >= 128 {128} else {0}])?;
        x >>= 7;
    }
    Ok(())
}