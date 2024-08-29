use super::networks::petaig;
use super::networks::Network;

type Netw = petaig::Aig;
type Signal = petaig::AigLit;

pub fn xor(network: &mut Netw, inputs: [Signal; 2]) -> Signal {
    let two = network.and(inputs[0], inputs[0]);
    let zero = network.and(!inputs[0], !inputs[0]);
    network.and(!two, !zero)
}

pub fn half_add(network: &mut Netw, inputs: [Signal; 2]) -> [Signal; 2] {
    let carry = network.and(inputs[0], inputs[1]);
    let zero = network.and(!inputs[0], !inputs[1]);
    let one = network.and(!carry, !zero);

    [one, carry]
}

pub fn full_add(network: &mut Netw, inputs: [Signal; 3]) -> [Signal; 2] {
    let [one, carry1] = half_add(network, [inputs[0], inputs[1]]);
    let [one, carry2] = half_add(network, [one, inputs[1]]);
    let carry = xor(network, [carry1, carry2]);

    [one, carry]
}

pub fn const_add<const N: usize>(network: &mut Netw, a: [Signal; N], b: [Signal; N]) -> ([Signal; N], Signal) {
    assert!(a.len() > 0);

    let mut outputs = unsafe { std::mem::MaybeUninit::<[Signal; N]>::uninit().assume_init() };
    let [one, mut carry] = half_add(network, [a[0], b[0]]);
    outputs[0] = one;
    
    for (i, (a, b)) in a.iter().copied().zip(b.iter().copied()).enumerate().skip(1) {
        let [one, carry2] = full_add(network, [carry, a, b]);
        outputs[i] = one;
        carry = carry2
    }

    (outputs, carry)
}

pub fn const_sub<const N: usize>(network: &mut Netw, a: [Signal; N], b: [Signal; N]) -> ([Signal; N], Signal) {
    let (outputs, carry) = const_add(network, a, b.map(|lit| !lit));
    (outputs.map(|lit| !lit), !carry)
}

pub fn max<const N: usize>(network: &mut Netw, a: [Signal; N], b: [Signal; N]) -> [Signal; N] {
    let (_, carry) = const_sub(network, a, b);

    const_mux(network, carry, b, a)
}

pub fn max_tree<const N: usize>(network: &mut Netw, inputs: &[[Signal; N]]) -> [Signal; N] {
    if inputs.len() == 0 {
        return [network.f(); N];
    }
    if inputs.len() == 1 {
        return inputs[0];
    }
    let left = max_tree(network, &inputs[..inputs.len()/2]);
    let right = max_tree(network, &inputs[inputs.len()/2..]);

    max(network, left, right)
}


pub fn min<const N: usize>(network: &mut Netw, a: [Signal; N], b: [Signal; N]) -> [Signal; N] {
    let (_, carry) = const_sub(network, a, b);

    const_mux(network, carry, a, b)
}

pub fn min_tree<const N: usize>(network: &mut Netw, inputs: &[[Signal; N]]) -> [Signal; N] {
    if inputs.len() == 0 {
        return [network.f(); N];
    }
    if inputs.len() == 1 {
        return inputs[0];
    }
    let left = min_tree(network, &inputs[..inputs.len()/2]);
    let right = min_tree(network, &inputs[inputs.len()/2..]);

    min(network, left, right)
}

pub fn const_word<const N: usize>(network: &mut Netw, mut x: usize) -> [Signal; N] {
    let mut i = N;
    [(); N].map(|_| {
        let x = network.c(x & (1 << i) != 0);
        i -= 1;
        x
    })
}

pub fn const_mux<const N: usize>(network: &mut Netw, mux: Signal, a: [Signal; N], b: [Signal; N]) -> [Signal; N] {
    let mut i = 0;
    [(); N].map(|_| {
        let output = network.mux(mux, a[i], b[i]);
        i += 1;
        output
    })
}