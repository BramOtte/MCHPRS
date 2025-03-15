use super::*;

struct Test {
    signal_count: usize,
}

#[derive(Debug)]
struct TestNode(usize);

#[derive(Debug, Clone, Copy)]
struct TestSignal(usize, bool);

impl Not for TestSignal {
    type Output = TestSignal;

    fn not(self) -> Self::Output {
        TestSignal(self.0, !self.1)
    }
}

impl Network for Test {
    type Node = TestNode;

    type Sig = TestSignal;
}

impl CreateAnd for Test {
    fn create_and(&mut self, a: Self::Sig, b: Self::Sig) -> Self::Sig {
        self.signal_count += 1;
        println!("{:?} = And({:?}, {:?})", self.signal_count, a, b);
        TestSignal(self.signal_count, false)
    }
}

impl CreateOr for Test {
    fn create_or(&mut self, a: Self::Sig, b: Self::Sig) -> Self::Sig {
        println!("{:?} = Or({:?}, {:?})", self.signal_count, a, b);
        !self.create_and(!a, !b)
    }
}