use std::usize;

use petgraph::{graph, Direction};
use petgraph::stable_graph::EdgeReference;
use petgraph::visit::{EdgeRef, IntoEdgesDirected, IntoNodeReferences, NodeIndexable, NodeRef};
use petgraph::Direction::{Incoming, Outgoing};

use super::andtree::And;
use super::Network;




type PAig = petgraph::stable_graph::StableDiGraph<AigNodeTy, bool, u32>;
type AigIndex = petgraph::stable_graph::NodeIndex<u32>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AigNodeTy {
    And, Input, Output, Latch, LocalInput
}


#[derive(Debug, Clone, Copy)]
pub struct AigLit(AigIndex, bool);

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

impl Network for Aig {
    type Sig = AigLit;
    type Node = Node;
}

impl Aig {
    pub fn new() -> Self {
        let mut g = PAig::new();

        let f = g.add_node(AigNodeTy::Input);

        Self { g, f }
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

    pub fn replace_input(&mut self, input: Node, lit: AigLit) {
        let outputs = self.g.edges_directed(input.0, Outgoing)
        .map(|edge| {
                AigLit(edge.target(), *edge.weight())
            }).collect::<Vec<_>>();
        
        for output in outputs {
            self.g.add_edge(lit.0, output.0, lit.1 ^ output.1);
        }
        
        self.g.remove_node(input.0);
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
        while changed {
            changed = false;

            for id in 0..self.g.node_bound() {
                let id = petgraph::stable_graph::node_index(id);
                if !self.g.contains_node(id) {
                    continue;
                }
                if self.g[id] != AigNodeTy::And {
                    continue;
                }
                
                if self.g.edges_directed(id, Direction::Outgoing).next().is_none() {
                    self.g.remove_node(id);
                    changed = true;
                    continue;;
                }

                let mut inputs = self.g.edges_directed(id, Direction::Incoming);
                let a = inputs.next().unwrap();
                let b = inputs.next().unwrap();

                if a.source() == b.source() {
                    let outputs =  self.g.edges_directed(id, Direction::Outgoing)
                        .map(|edge| (edge.target().id(), *edge.weight()))
                        .collect::<Vec<_>>();

                    let equal_weights = a.weight() == b.weight();
                    if equal_weights {
                        let src = a.source();
                        let weight = *a.weight();
                        for (output, output_weight) in outputs {
                            self.g.add_edge(src, output, weight ^ output_weight);
                        }
                    } else {
                        for (output, output_weight) in outputs {
                            self.g.add_edge(self.f, output, output_weight);
                        }
                    }

                    self.g.remove_node(id);
                    changed = true;
                    continue;;
                }

            }
        }


    }

    pub fn serialize<W: std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
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
                AigNodeTy::Input | AigNodeTy::LocalInput => {
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

        writeln!(w, "aig {} {} {} {} {}", inputs.len()-1+latches.len()+gates.len(), inputs.len()-1, latches.len(), outputs.len(), gates.len())?;

        let get_num_n = |id: AigIndex| {
            let &(node, index) = map.get(&id).unwrap();
            let offset = match node {
                AigNodeTy::Input | AigNodeTy::LocalInput => 0,
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
            println!("{} {} {} {}, {} {}", lhs/2, lhs, rhs0, rhs1, delta0, delta1);

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