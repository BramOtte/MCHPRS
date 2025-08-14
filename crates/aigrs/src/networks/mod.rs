use std::ops::Not;

pub mod petaig;
pub mod aiger;
pub trait Network {
    type Node;
    type Sig: Copy + Not;
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

pub trait CreateOrs: Network {
    fn create_ors<T: ExactSizeIterator<Item=Self::Sig>>(&mut self, inputs: T) -> Self::Sig;
}

pub trait CreateLatch: Network {
    fn create_latch(&mut self) -> (Self::Node, Self::Sig);
    fn connect_latch(&mut self, latch: Self::Node);
}