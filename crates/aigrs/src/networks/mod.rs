use std::ops::Not;

pub mod petaig;
pub mod aiger;
pub mod andtree;
pub mod testnetwork;


pub trait Network {
    type Node;
    type Sig;
}

pub trait CreateNew: Network {
    fn new() -> Self;
}

pub trait CreatePi: Network {
    fn create_pi(&mut self) -> Self::Sig;
}

pub trait CreatePo: Network {
    fn create_po(&mut self, signal: Self::Sig) -> Self::Sig;
}

pub trait CreateAnd: Network {
    fn create_and(&mut self, a: Self::Sig, b: Self::Sig) -> Self::Sig;
}

pub trait CreateOr: Network {
    fn create_or(&mut self, a: Self::Sig, b: Self::Sig) -> Self::Sig;
}

pub trait CreateLatch: Network {
    fn create_latch(&mut self) -> (Self::Node, Self::Sig);
}