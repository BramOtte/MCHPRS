#[derive(Debug, Clone, Copy)]
pub struct AigLit {
    data: u32
}

impl AigLit {
    pub const FALSE: Self = Self::c(false);
    pub const TRUE: Self = Self::c(true);


    pub const fn c(sign: bool) -> Self {
        Self::new(0, sign, false)
    }
    
    const fn new(index: usize, sign: bool, and: bool) -> Self {
        Self { data: ((index as u32) << 2) | ((sign as u32) << 1) | and as u32 }
    }

    pub const fn is_and(&self) -> bool {
        self.data & 1 == 0
    }

    pub const fn is_const(&self) -> bool {
        (self.data >> 2) == 0
    }

    pub const fn sign(&self) -> bool {
        self.data & 2 != 0
    }
    pub const fn index(&self) -> usize {
        (self.data >> 2) as usize
    }

    fn set_index(&mut self, index: usize) {
        *self = Self::new(index, self.sign(), self.is_and());
    }

    const fn num(&self) -> u32 {
        self.data >> 1
    }
}

impl std::ops::Not for AigLit {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self {data: self.data ^ 2}
    }
}

pub struct And(pub AigLit, pub AigLit);
struct Latch {
    next_state: AigLit,
    state: u32,
}

pub struct LatchRef(u32);

struct Aig {
    input_count: u32,
    outputs: Vec<AigLit>,
    and_gates: Vec<And>,
    latches: Vec<Latch>,
    input_after_latch: bool,
}

impl Aig {
    pub const FALSE: AigLit = AigLit::FALSE;
    pub const TRUE: AigLit = AigLit::TRUE;
    
    pub const fn c(&self, sign: bool) -> AigLit {
        AigLit::c(sign)
    }
    
    pub fn input(&mut self) -> AigLit {
        self.input_count += 1;

        self.input_after_latch = self.latches.len() > 0;

        AigLit::new(self.input_count as usize, false, false)
    }
    pub fn output(&mut self, lit: AigLit) {
        self.outputs.push(lit)
    }

    pub fn and(&mut self, a: AigLit, b: AigLit) -> AigLit {
        self.and_gates.push(And(a, b));
        AigLit::new(self.and_gates.len(), false, true)
    }

    pub fn latch(&mut self) -> (LatchRef, AigLit) {
        self.input_count += 1;

        let state = AigLit::new(self.outputs.len(), false, false);
        let next_state = self.input_count as u32;


        (LatchRef(next_state), state)
    }

    pub fn latch2(&mut self, lit: AigLit) -> AigLit {
        let (next_state, state) = self.latch();
        self.latch_next_state(next_state, lit);
        state
    }

    pub fn latch_next_state(&mut self, latch: LatchRef, next_state: AigLit) {
        self.outputs.push(next_state);
        self.latches.push(Latch { next_state, state: latch.0 })
    }

    fn create_lut(&self) {
        type U = u64;
        let mut data: Vec<U> = vec![0; 1 + self.input_count as usize + self.and_gates.len()];
        let mut i = self.input_count as usize + 1;
        for &And(left, right) in self.and_gates.iter() {
            let left = data[self.index(left)] ^ (left.sign() as U * U::MAX);
            let right = data[self.index(right)] ^ (right.sign() as U * U::MAX);
            
            data[i] = left & right;

            i +=1;
        }
    }

    fn order_inputs(&mut self) {
        // if self.input_after_latch {
        //     return;
        // }
        // let mut map = Vec::with_capacity(self.input_count as usize);
        
        // fn adjust(map: &[u32], lit: &mut AigLit) {
        //     if lit.is_and() {
        //         return;
        //     }

        //     *lit = AigLit::new(map[lit.index()] as usize, lit.sign(), false);
        // }
        
        // let first_latch = self.input_count - self.latches.len() as u32;
        // let mut start = 0;
        // for (offset, latch) in self.latches.iter_mut().enumerate() {
        //     for i in start..latch.state {
        //         map[i as usize] = i + offset as u32;
        //     }

            

        //     start = latch.state;
        // }
        // // let mut latches = self.latches.iter();
        // // let mut Some(latch) = latches.next()

        // // for i in 0..self.input_count {
        // // }

        // self.input_after_latch = false;
    }

    fn index(&self, lit: AigLit) -> usize {
        lit.index() + (lit.is_and() as usize * self.input_count as usize)
    }

    fn num(&self, lit: AigLit) -> u32 {
        if lit.is_and() {
            lit.num() + self.input_count*2
        } else {
            lit.num()
        }
    }

    pub fn to_dot<W: std::io::Write>(&self) -> std::io::Result<()> {
        assert!(!self.input_after_latch);

        Ok(())
    }
}