use std::io::Bytes;
use std::iter::Copied;

#[derive(Debug, Clone, Copy)]
pub struct AigLit(u32);

impl AigLit {
    pub const FALSE: AigLit = AigLit::c(false);
    pub const TRUE: AigLit = AigLit::c(true);


    pub const fn c(sign: bool) -> Self {
        Self::new(0, sign)
    }

    const fn new(index: u32, sign: bool) -> Self {
        Self((index << 1) | sign as u32)
    }

    pub const fn sign(&self) -> bool {
        self.0 & 1 != 0
    }


    pub const fn index(&self) -> u32 {
        self.0 >> 1
    }

    pub const fn num(&self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
struct And(AigLit, AigLit);
struct Output(u32);

struct Latch(Output, AigLit);

struct Aiger {
    input_count: u32,
    latch_count: u32,
    ands: Vec<And>,
    outputs: Vec<AigLit>,
}
impl Aiger {
    pub fn new() -> Self {
        Self { input_count: 0, ands: vec![And(AigLit::FALSE, AigLit::FALSE)], outputs: Vec::new(), latch_count: 0 }
    }

    pub fn get_input(&self, index: u32) -> AigLit {
        debug_assert!(index <= self.input_count);
        AigLit::new(index, false)
    }

    pub fn input(&mut self) -> AigLit {
        debug_assert_eq!(self.ands.len(), 0);
        self.input_count += 1;
        self.ands.push(And(AigLit::FALSE, AigLit::FALSE));
        AigLit::new(self.input_count, false)
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

    pub fn parse(bytes: &[u8]) -> Self {
        let mut parser = Parser::new(bytes);

        let magic = parser.text();
        assert_eq!(magic, "aig".as_bytes());

        let total = parser.num().unwrap();
        let pi_count = parser.num().unwrap();
        let latch_count = parser.num().unwrap();
        let po_count = parser.num().unwrap();
        let gate_count = parser.num().unwrap();
        
        assert_eq!(total, pi_count + latch_count + po_count + gate_count);

        parser.skip_white();

        let input_count = pi_count + latch_count;

        let mut aig = Self {
            input_count,
            ands: Vec::with_capacity((input_count + gate_count) as usize),
            outputs: Vec::with_capacity((latch_count + po_count) as usize),
            latch_count,
        };

        unsafe {
            aig.ands.set_len(1 + input_count as usize);
        }

        for _ in 0..latch_count+po_count {
            let next_state = parser.num().unwrap();
            aig.outputs.push(AigLit(next_state));
        }

        for i in input_count..input_count+gate_count {
            let delta0 = parser.var_int().unwrap();
            let delta1 = parser.var_int().unwrap();
            let rhs0 = i - delta0;
            let rhs1 = rhs0 - delta1;
            aig.ands.push(And(AigLit(rhs0), AigLit(rhs1)));
        }

        aig
    }

    pub fn serialize<W: std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        fn write_var_int<W: std::io::Write>(mut x: u32, w: &mut W) -> std::io::Result<()>  {
            while x > 0 {
                w.write(&[(x & 127) as u8 | if x >= 128 {128} else {0}])?;
                x >>= 7;
            }
            Ok(())
        }

        let pi_count = self.input_count - self.latch_count;
        let latch_count = self.latch_count;
        let po_count = self.outputs.len() as u32 - self.latch_count;
        let gate_count = self.ands.len() as u32;
        let total = pi_count + latch_count + po_count + gate_count;

        writeln!(w, "aig {} {} {} {} {}", total, pi_count, latch_count, po_count, gate_count)?;

        for output in self.outputs.iter().copied() {
            writeln!(w, "{}", output.num())?;
        }

        for (i, And(rhs0, rhs1)) in self.ands.iter().copied().enumerate() {
            let lhs = self.input_count + i as u32;
            let delta0 = lhs - rhs0.num();
            let delta1 = rhs0.num() - rhs1.num();

            write_var_int(delta0, w)?;
            write_var_int(delta1, w)?;
        }

        Ok(())
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
        self.skip_spaces();
        let start = self.pos();
        self.skip_while(|c| c > ' ' as u8);
        self.lex(start)
    }

    fn num(&mut self) -> Option<u32> {
        self.skip_spaces();
        let mut num = 0;
        let mut matched = false;
        while let Some(c) = self.next_if(|c| c.is_ascii_digit())  {
            num = num * 10 + (c as u32 - '0' as u32);
            matched = true;
        }
        matched.then_some(num)
    }

    fn var_int(&mut self) -> Option<u32> {
        if self.iter.len() == 0 {
            return None;
        }

        let mut x = 0;

        while let Some(c) = self.next() {
            x = (x << 7) | (c & 127) as u32;
            if c > 127 {
                break;
            }
        }

        Some(x)
    }
}