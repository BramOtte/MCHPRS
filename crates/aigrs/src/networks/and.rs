struct Ands {
    state: Vec<u64>,
    next_state: Vec<u64>,
    ands: Vec<BigAnd>,
    start_end: usize,

}

struct BigAnd {
    left: u32,
    right: u32,
}

impl Ands {
    fn out_fn(&self) {

    }
}