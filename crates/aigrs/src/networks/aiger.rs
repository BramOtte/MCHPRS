use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::io::Bytes;
use std::iter::Copied;
use std::num::NonZero;
use std::ops::{BitXor, Not};
use std::rc::Rc;
use super::Network;

use petgraph::visit::{EdgeRef, IntoEdgesDirected, IntoNeighborsDirected, NodeIndexable};
use petgraph::Direction::{Incoming, Outgoing};

use super::petaig;

#[derive(Debug, Clone, Copy)]
pub struct AigLit(u32);

impl AigLit {
    pub const FALSE: AigLit = AigLit::c(false);
    pub const TRUE: AigLit = AigLit::c(true);


    pub const fn c(sign: bool) -> Self {
        Self::new(0, sign)
    }

    pub const fn new(index: usize, sign: bool) -> Self {
        Self(((index as u32) << 1) | sign as u32)
    }

    pub const fn sign(&self) -> bool {
        self.0 & 1 != 0
    }


    pub const fn index(&self) -> usize {
        (self.0 >> 1) as usize
    }

    pub const fn num(&self) -> usize {
        self.0 as usize
    }

    pub const fn xor(self, sign: bool) -> Self {
        Self::new(self.index(), self.sign() ^ sign)
    }
}

impl Not for AigLit {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self::new(self.index(), !self.sign())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct And(pub AigLit, pub AigLit);
pub struct Output(u32);

struct Latch(Output, AigLit);

pub struct AigerHeader {
    max_index: usize,
    pi_count: usize,
    latch_count: usize,
    po_count: usize,
    and_count: usize,
}

impl AigerHeader {
    pub fn parse(bytes: &[u8]) -> Result<Self, AigerParseError<'static>>  {
        Self::p(&mut Parser::new(bytes))
    }

    fn p(parser: &mut Parser) -> Result<Self, AigerParseError<'static>> {
        let magic = parser.text();
        assert_eq!(magic, "aig".as_bytes());

        let max_index = parser.num()?;
        let pi_count = parser.num()?;
        let latch_count = parser.num()?;
        let po_count = parser.num()?;
        let and_count = parser.num()?;

        if max_index != pi_count + latch_count + and_count {
            return Err(AigerParseError::new(
                parser.pos(),
                format!("{max_index} != {pi_count} + {latch_count} + {and_count}")
            ));
        }

        Ok(AigerHeader {
            max_index,
            pi_count,
            latch_count,
            po_count,
            and_count,
        })
    }
}

/*
[
zero
inputs
latches
]
latches
outputs
*/
pub struct Aiger {
    pub start_latches: usize,
    pub start_gates: usize,
    pub ands: Vec<And>,
    pub outputs: Vec<AigLit>,
}
impl Aiger {
    pub fn ci_count(&self) -> usize {
        self.start_gates - 1
    }
    pub fn pi_count(&self) -> usize {
        self.start_latches - 1
    }
    pub fn co_count(&self) -> usize {
        self.outputs.len()
    }
    pub fn po_count(&self) -> usize {
        self.co_count() - self.latch_count()
    }
    pub fn latch_count(&self) -> usize {
        self.start_gates - self.start_latches
    }
    pub fn and_count(&self) -> usize {
        self.ands.len() - self.ci_count() - 1
    }

    pub fn set_latch_count(&mut self, latch_count: usize) {
        assert!(latch_count <= self.ci_count());
        assert!(latch_count <= self.co_count());

        self.start_latches = self.start_gates - latch_count;
    }

    pub fn new() -> Self {
        Self { start_latches: 1, ands: vec![And(AigLit::FALSE, AigLit::FALSE)], outputs: Vec::new(), start_gates: 1 }
    }

    pub fn get_input(&self, index: usize) -> AigLit {
        debug_assert!(index as usize <= self.start_latches);
        AigLit::new(index, false)
    }

    pub fn input(&mut self) -> AigLit {
        debug_assert_eq!(self.start_latches, self.start_gates);
        self.start_latches += 1;
        self.start_gates += 1;
        self.ands.push(And(AigLit::FALSE, AigLit::FALSE));
        AigLit::new(self.start_latches, false)
    }

    pub fn and(&mut self, a: AigLit, b: AigLit) -> AigLit {
        let index = self.ands.len() as u32;
        self.ands.push(And(a, b));
        AigLit(index)
    }

    pub fn output(&mut self, lit: AigLit) -> Output {
        let index = self.outputs.len() as u32;
        self.outputs.push(lit);
        Output(index)
    }

    pub fn iter_pis(&self) -> impl Iterator<Item=AigLit> {
        (1..self.start_latches).map(|i| AigLit::new(i, false))
    }
    pub fn iter_cis(&self) -> impl Iterator<Item=AigLit> {
        (1..self.start_gates).map(|i| AigLit::new(i, false))
    }
    pub fn iter_latches(&self) -> impl Iterator<Item=AigLit> {
        (self.start_latches..self.start_gates).map(|i| AigLit::new(i, false))
    }

    pub fn iter_ands(&self) -> impl Iterator<Item=AigLit> {
        (self.start_gates..self.ands.len()).map(|i| AigLit::new(i, false))
    }
    pub fn iter_and_nodes<'a>(&'a self) -> impl Iterator<Item=usize> {
        self.start_gates..self.ands.len()
    }

    pub fn parse_comb(bytes: &[u8]) -> Result<(Self, usize), AigerParseError> {
        let (mut graph, index) = Self::parse(bytes)?;
        graph.set_latch_count(0);
        Ok((graph, index))
    }

    pub fn parse(bytes: &[u8]) -> Result<(Self, usize), AigerParseError> {
        let mut parser = Parser::new(bytes);

        let AigerHeader { max_index: _, pi_count, latch_count, po_count, and_count } = AigerHeader::p(&mut parser)?;

        let ci_count = pi_count + latch_count;

        let mut aig = Self {
            start_latches: pi_count + 1,
            start_gates: ci_count + 1,
            ands: Vec::with_capacity(ci_count + and_count + 1),
            outputs: Vec::with_capacity(latch_count + po_count),
        };

        unsafe {
            aig.ands.set_len(1 + ci_count);
        }

        for _ in 0..latch_count+po_count {
            parser.skip_white();
            let next_state = parser.num()?;
            aig.outputs.push(AigLit(next_state as u32));
        }

        parser.next();

        for i in 1+ci_count..1+ci_count+and_count {
            let delta0 = parser.var_int()?;
            let delta1 = parser.var_int()?;
            let lhs = i*2;

            let rhs0 = lhs - delta0;
            let rhs1 = rhs0 - delta1;
            aig.ands.push(And(AigLit(rhs0 as u32), AigLit(rhs1 as u32)));
        }

        Ok((aig, parser.pos()))
    }

    pub fn serialize<W: std::io::Write>(&self, w: &mut W, comb: bool) -> std::io::Result<()> {
        fn write_var_int<W: std::io::Write>(mut x: usize, w: &mut W) -> std::io::Result<()>  {
            while x > 0 {
                w.write(&[(x & 127) as u8 | if x >= 128 {128} else {0}])?;
                x >>= 7;
            }
            Ok(())
        }

        let total = self.pi_count() + self.latch_count() + self.and_count();

        if comb {
            writeln!(w, "aig {} {} {} {} {}", total, self.ci_count(), 0, self.co_count(), self.and_count())?;
        } else {
            writeln!(w, "aig {} {} {} {} {}", total, self.pi_count(), self.latch_count(), self.po_count(), self.and_count())?;
        }

        for output in self.outputs.iter().copied() {
            writeln!(w, "{}", output.num())?;
        }
        
        for lhs in self.iter_ands() {
            let And(mut rhs0, mut rhs1) = self.ands[lhs.index()];
            if rhs0.num() < rhs1.num() {
                std::mem::swap(&mut rhs0, &mut rhs1);
            }
            let delta0 = lhs.num() - rhs0.num();
            let delta1 = rhs0.num() - rhs1.num();

            write_var_int(delta0, w)?;
            write_var_int(delta1, w)?;
        }

        Ok(())
    }

    pub fn forward_links(&self) -> Vec<Vec<AigLit>> {
        let mut forward = vec![Vec::new(); self.ands.len()];

        for gate in self.iter_ands() {
            let And(a, b) = self.ands[gate.index()];
            forward[a.index()].push(gate.xor(a.sign()));
            forward[b.index()].push(gate.xor(b.sign()));
        }
        for (i, output) in self.outputs.iter().copied().enumerate() {
            forward[output.index()].push(AigLit::new(self.ands.len() + i, output.sign()));
        }

        forward
    }
}

pub struct AigerParseError<'a> {
    index: usize,
    message: Cow<'a, str>
}
impl <'a> AigerParseError<'a> {
    pub fn new(pos: usize, message: String) -> Self {
        Self { index: pos, message: Cow::Owned(message) }
    }
    pub fn ueof(pos: usize) -> Self {
        Self { index: pos, message: Cow::Borrowed("unexpected input") }
    }
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl <'a> Debug for AigerParseError<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}: {}", self.index(), self.message())
    }
}

impl <'a> Display for AigerParseError<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}: {}", self.index(), self.message())
    }
}


struct Parser<'a> {
    bytes: &'a [u8],
    iter: Copied<std::slice::Iter<'a, u8>>,
}

impl <'a> Parser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, iter: bytes.iter().copied() }
    }
    fn pos(&self) -> usize {
        self.bytes.len() - self.iter.len()
    }
    fn next(&mut self) -> Option<u8> {
        self.iter.next()
    }
    fn peak(&mut self) -> Option<u8> {
        self.iter.clone().next()
    }
    fn lex(&self, pos: usize) -> &[u8] {
        &self.bytes[pos..self.pos()]
    }

    fn skip_spaces(&mut self) {
        self.skip_while(|c| c == ' ' as u8 )
    }
    fn skip_white(&mut self) {
        self.skip_while(|c| c <= ' ' as u8 )
    }    
    
    fn skip_while<F: Fn(u8) -> bool>(&mut self, f: F) {
        while let Some(p) = self.peak() {
            if !f(p){
                break;
            }
            self.next();
        }
    }

    fn next_if<F: Fn(u8) -> bool>(&mut self, f: F) -> Option<u8> {
        let Some(p) = self.peak() else {
            return None;
        };

        if !f(p){
            return None;
        }

        return self.next();
    }

    fn text(&mut self) -> &[u8] {
        let start = self.pos();
        self.skip_while(|c| c > ' ' as u8);
        self.lex(start)
    }

    fn num(&mut self) -> Result<usize, AigerParseError<'static>> {
        self.skip_spaces();
        let mut num = 0;
        let mut matched = false;
        while let Some(c) = self.next_if(|c| c.is_ascii_digit())  {
            num = num * 10 + (c as usize - '0' as usize);
            matched = true;
        }
        if matched {
            Ok(num)
        } else {
            Err(self.uef())
        }
    }

    fn uef(&self) -> AigerParseError<'static> {
        AigerParseError::ueof(self.pos())
    }

    fn var_int(&mut self) -> Result<usize, AigerParseError<'static>> {
        if self.iter.len() == 0 {
            return Err(self.uef());
        }

        let mut x = 0;
        let mut i = 0;
        while let Some(c) = self.next() {
            x |= ((c & 127) as usize) << (i * 7);
            i += 1;
            if c < 128 {
                break;
            }
        }

        Ok(x)
    }
}