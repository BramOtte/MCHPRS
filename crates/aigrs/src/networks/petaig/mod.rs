use std::time::Instant;
use std::usize;

use petgraph::{graph, Direction};
use petgraph::stable_graph::EdgeReference;
use petgraph::visit::{EdgeRef, IntoEdgesDirected, IntoNodeReferences, NodeIndexable, NodeRef};
use petgraph::Direction::{Incoming, Outgoing};

use crate::networks::aiger;

use super::aiger::Aiger;
use super::andtree::And;
use super::Network;




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
}

impl std::ops::Not for AigLit {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(self.0, !self.1)
    }
}


#[derive(Debug)]
pub struct NextState(AigIndex);

#[derive(Debug, Clone)]
pub struct Node(AigIndex);

impl Node {
    pub const fn lit(&self) -> AigLit {
        AigLit(self.0, false)
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

impl <A, B> AigAdd for And<A, B>
    where A: AigAdd, B: AigAdd
{
    fn add(self, aig: &mut Aig) -> AigLit {
        let a = self.lhs.add(aig);
        let b = self.rhs.add(aig);
        aig.andx(a, b, self.inv)
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
            println!("bound {} {}", i, self.g.node_bound());
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


    pub fn to_aiger(&self) -> aiger::Aiger {
        use petgraph::prelude::*;
        use aiger::{And, AigLit};

        let size = self.size();

        let ci = size.pi + size.latches;

        let mut aig = aiger::Aiger {
            start_latches: size.pi + 1,
            start_gates: ci + 1,
            ands: Vec::with_capacity(ci + size.ands + 1),
            outputs: Vec::with_capacity(size.latches + size.po),
        };

        // unsafe  {
        //     aig.ands.set_len(ci + 1);
        // }
        aig.ands.push(And(AigLit::FALSE, AigLit::FALSE));

        let mut visited: Vec<AigLit> = vec![AigLit::FALSE; self.g.node_bound()];
        let mut input_count: Vec<u32> = vec![0; self.g.node_bound()];

        let mut queue: Vec<NodeIndex> = self.g.node_indices()
            .filter(|&node| matches!(self.g[node], AigNodeTy::Input))
            .chain(
                self.g.node_indices()
                    .filter(|&node| matches!(self.g[node], AigNodeTy::Latch))
            )
            .collect::<Vec<_>>();
        let mut next_queue: Vec<NodeIndex> = Vec::new();

        let mut outputs: Vec<NodeIndex> = Vec::new();

        while queue.len() > 0 {
            for node in queue.drain(..) {
                
                //  = aig.ands.len() as u32;

                let mut inputs = self.g.edges_directed(node, Incoming);
                match self.g[node] {
                    AigNodeTy::And => {
                        let a = inputs.next().unwrap();
                        let b = inputs.next().unwrap();

                        let a = AigLit::new(a.source().index(), *a.weight());
                        let b = AigLit::new(b.source().index(), *b.weight());
                        
                        visited[node.index() as usize] = aig.and(a, b);
                    },
                    AigNodeTy::Input => {
                        visited[node.index() as usize] = aig.input();
                    },
                    AigNodeTy::Latch => {
                        visited[node.index() as usize] = aig.input();
                        aig.start_latches -= 1;
                        // aig.set_latch_count(latch_count);
                    },
                    AigNodeTy::Output => {
                        outputs.push(node);
                    },
                    AigNodeTy::LocalInput => panic!("Should not contain local inputs please run gc"),
                    AigNodeTy::False => {},
                }

                for output in self.g.neighbors_directed(node, Outgoing) {
                    input_count[output.index()] += 1;
                    let count = if matches!(self.g[output], AigNodeTy::And) {
                        2
                    } else {
                        1
                    };
                    if count == input_count[output.index()] {
                        next_queue.push(output);
                    }
                    assert!(count <= input_count[output.index()]);
                }
            }

            std::mem::swap(&mut queue, &mut next_queue);
        }
        // let mut input_c = 0;
        // let mut latch_c = aig.start_latches;
        // let mut and_c = aig.start_gates;

        // for node in input.g.node_indices() {
        //     match input.g[node] {
        //         AigNodeTy::And => {
        //             let mut inputs = input.g.edges_directed(node, Incoming);
        //             let a = inputs.next().unwrap();
        //             let b = inputs.next().unwrap();

        //             let a = AigLit::new(a.source().index(), *a.weight());
        //             let b = AigLit::new(b.source().index(), *b.weight());


        //             aig.and(a, b);
        //             visited[node.index()] = and_c as u32;
        //             and_c += 1
        //         },
        //         AigNodeTy::Input => {
        //             visited[node.index()] = input_c;
        //             input_c += 1;
        //         },
        //         AigNodeTy::Output => aig.outputs.push(AigLit::new(node.index(), false)),
        //         AigNodeTy::Latch => todo!(),
        //         AigNodeTy::LocalInput => todo!(),
        //         AigNodeTy::False => todo!(),
        //     }
        // }

        aig
    }

    pub fn to_aig(&self) -> Aiger {
        let mut inputs = Vec::<AigIndex>::new();
        let mut latches = Vec::<AigIndex>::new();
        let mut outputs = Vec::<AigIndex>::new();
        let mut gates = Vec::<AigIndex>::new();
        
        let mut map = std::collections::HashMap::<AigIndex, (AigNodeTy, usize)>::new();

        for (id, &node) in self.g.node_references() {
            match node {
                AigNodeTy::And => {
                    gates.push(id);
                },
                AigNodeTy::Input | AigNodeTy::LocalInput | AigNodeTy::False => {
                    map.insert(id, (node, inputs.len()));
                    inputs.push(id);
                }
                AigNodeTy::Output => {
                    map.insert(id, (node, outputs.len()));
                    outputs.push(id);
                },
                AigNodeTy::Latch => {
                    map.insert(id, (node, latches.len()));
                    latches.push(id);
                },
            }
        }

        {
            let mut stack = 
                inputs.iter().copied()
                .chain(
                    latches.iter().copied()
                )    
                .map(|node| (node, 1))
                .collect::<Vec<_>>();

            let mut depth_map = vec![0; self.g.node_bound()];

            while let Some((node, depth)) = stack.pop() {
                for neighbor in self.g.neighbors_directed(node, Outgoing) {
                    if self.g[neighbor] != AigNodeTy::And {
                        continue;
                    }
                    if depth_map[neighbor.index()] >= depth {
                        continue;
                    }
                    depth_map[neighbor.index()] = depth;
                    stack.push((neighbor, depth+1));
                }
            }

            // TODO: use breadth first search so we don't need to sort 
            gates.sort_by_key(|&node| depth_map[node.index()]);
        }

        todo!()
    }

    pub fn serialize<W: std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        let mut inputs = Vec::<AigIndex>::new();
        let mut latches = Vec::<AigIndex>::new();
        let mut outputs = Vec::<AigIndex>::new();
        let mut gates = Vec::<AigIndex>::new();
        
        let mut map = std::collections::HashMap::<AigIndex, (AigNodeTy, usize)>::new();

        for (id, &node) in self.g.node_references() {
            match node {
                AigNodeTy::And => {
                    gates.push(id);
                },
                AigNodeTy::Input | AigNodeTy::LocalInput | AigNodeTy::False => {
                    map.insert(id, (node, inputs.len()));
                    inputs.push(id);
                }
                AigNodeTy::Output => {
                    map.insert(id, (node, outputs.len()));
                    outputs.push(id);
                },
                AigNodeTy::Latch => {
                    map.insert(id, (node, latches.len()));
                    latches.push(id);
                },
            }
        }
        
        {
            let mut stack = 
                inputs.iter().copied()
                .chain(
                    latches.iter().copied()
                )    
                .map(|node| (node, 1))
                .collect::<Vec<_>>();

            let mut depth_map = vec![0; self.g.node_bound()];

            while let Some((node, depth)) = stack.pop() {
                for neighbor in self.g.neighbors_directed(node, Outgoing) {
                    if self.g[neighbor] != AigNodeTy::And {
                        continue;
                    }
                    if depth_map[neighbor.index()] >= depth {
                        continue;
                    }
                    depth_map[neighbor.index()] = depth;
                    stack.push((neighbor, depth+1));
                }
            }

            gates.sort_by_key(|&node| depth_map[node.index()]);
        }


        for (index, gate) in gates.iter().copied().enumerate() {
            map.insert(gate, (AigNodeTy::And, index));
        }

        // println!("{:#?}\n\n{:#?}\n\n{:#?}\n\n{:#?}", inputs, latches, gates, outputs);

        writeln!(w, "aig {} {} {} {} {}", inputs.len()-1+latches.len()+gates.len(), inputs.len()-1, latches.len(), outputs.len(), gates.len())?;

        let get_num_n = |id: AigIndex| {
            let &(node, index) = map.get(&id).unwrap();
            let offset = match node {
                AigNodeTy::Input | AigNodeTy::LocalInput | AigNodeTy::False => 0,
                AigNodeTy::Latch => inputs.len(),
                AigNodeTy::And => inputs.len() + latches.len(),
                AigNodeTy::Output => panic!("output has no literal"),
            };
            (offset+index)*2
        };

        let get_num = |edge: EdgeReference<'_, bool, u32>| {
            get_num_n(edge.source()) + *edge.weight() as usize
        };

        
        for (i, node) in latches.iter().copied().chain(outputs.iter().copied()).enumerate() {
            let mut edges = self.g.edges_directed(node, Incoming);
            let Some(edge) = edges.next() else {
                eprintln!("missing edge");
                continue;
            };
            if edges.next().is_some() {
                eprintln!("too many edges");
                continue;    
            }
            writeln!(w, "{}", get_num(edge))?;
        }

        for node in gates.iter().copied() {
            let mut edges = self.g.edges_directed(node, Incoming);
            let Some(rhs0) = edges.next() else {
                eprintln!("AND with 0 inputs");
                continue;
            };
            
            let Some(rhs1) = edges.next() else {
                eprintln!("AND with 1 input");
                continue;
            };
            if edges.next().is_some() {
                eprintln!("AND with 3 input");
                continue;
            }

            let lhs = get_num_n(node);
            let mut rhs0 = get_num(rhs0);
            let mut rhs1 = get_num(rhs1);
            if rhs0 < rhs1 {
                std::mem::swap(&mut rhs0, &mut rhs1);
            }

            assert!(lhs > rhs0);

            let delta0 = lhs - rhs0;
            let delta1 = rhs0 - rhs1;

            write_var_int(delta0, w)?;
            write_var_int(delta1, w)?;
            // println!("{} {} {} {}, {} {}", lhs/2, lhs, rhs0, rhs1, delta0, delta1);
        }
        
        Ok(())
    }
 
}


fn write_var_int<W: std::io::Write>(mut x: usize, w: &mut W) -> std::io::Result<()>  {
    while x > 0 {
        w.write(&[(x & 127) as u8 | if x >= 128 {128} else {0}])?;
        x >>= 7;
    }
    Ok(())
}