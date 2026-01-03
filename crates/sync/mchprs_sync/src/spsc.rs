use std::{ops::Deref, sync::{Arc, atomic::{AtomicUsize, Ordering}}};

use crossbeam_utils::CachePadded;

pub fn channel<T: Default>() -> (Sender<T>, Receiver<T>) {
    let queue = Arc::new(ConcurrentQueue::default());
    (Sender { queue: queue.clone(), write: 0, read: 0}, Receiver { queue, write: 0, read: 0 })
}

pub fn channel_using<T>(values: T) -> (Sender<T>, Receiver<T>) {
    let queue = Arc::new(ConcurrentQueue {
        values,
        read: Default::default(),
        write: Default::default(),
    });
    (Sender { queue: queue.clone(), write: 0, read: 0}, Receiver { queue, write: 0, read: 0 })
}

#[derive(Default)]
struct ConcurrentQueue<T> {
    values: T,
    read: CachePadded<AtomicUsize>,
    write: CachePadded<AtomicUsize>,
}

pub struct Sender<T> {
    queue: Arc<ConcurrentQueue<T>>,
    write: usize,
    read: usize,
}

impl <T, const SIZE: usize> Sender<[CachePadded<T>; SIZE]> {
    pub fn get(&mut self) -> &mut T {
        let write = self.write;
        let next_write = (write + 1) % SIZE;
        if next_write == self.read {
            loop {
                self.read = self.queue.read.load(Ordering::Relaxed);
                if self.read != next_write {
                    break;
                }
                std::thread::yield_now();
            }
        }

        unsafe {
            &mut *(make_mut(self.queue.values.as_ptr())).add(write)
        }
    }

    pub fn advance(&mut self) {
        let write = self.write;
        self.write = (write + 1) % SIZE;
        self.queue.write.store(self.write, Ordering::Release);
    }
}


pub struct Receiver<T> {
    queue: Arc<ConcurrentQueue<T>>,
    write: usize,
    read: usize,
}

impl <T, const SIZE: usize> Receiver<[CachePadded<T>; SIZE]> {
    pub fn get<'a>(&'a mut self) -> &'a T {
        let read = self.read;

        if read == self.write {
            loop {
                self.write = self.queue.write.load(Ordering::Acquire);
                if self.write != read {
                    break;
                }
                std::thread::yield_now();
            }
        }

        &self.queue.values[read]
    }

    pub fn advance(&mut self) {
        let read = self.read;
        self.read = (read + 1) % SIZE;
        self.queue.read.store(self.read, Ordering::Release);
    }
}


unsafe fn make_mut<T>(ptr: *const T) -> *mut T {
    ptr as *mut T
}




#[test]
fn test_channel() {
    use std::{hint::black_box, time::Instant};

    const TO_SEND: u64 = 10_000_000;
    const QUEUE_SIZE: usize = 256;

    println!();
    println!("get and advance");
    {
        let (mut sender, mut receiver) = channel_using::<[CachePadded<Vec<_>>; _]>(
            [(); QUEUE_SIZE].map(|_| Default::default())
        );
    
        let a = std::thread::spawn(move || {
            let data: Vec<_> = (0..TO_SEND).map(|v| [v, v+1, v+2]).collect();
            
            let start = Instant::now();
            let sum = black_box({
                let mut sum = 0u64;
                for d in data {
                    sum += d.iter().sum::<u64>();
                    let s = sender.get();
                    s.clear();
                    s.push(d);
                    sender.advance();
                }
                sum
            });
            let dt = Instant::now() - start;
            println!("write {:?} {} {:e}", dt, sum, TO_SEND as f64 / dt.as_secs_f64());
        });
    
        let b = std::thread::spawn(move || {
            let start = Instant::now();
            let mut first = start;
            let sum = black_box({
                let mut sum: u64 = 0;
                let slice = receiver.get();
                first = Instant::now();
                for d in slice {
                    sum += d.iter().copied().sum::<u64>();
                }
                receiver.advance();
                for _ in 1..TO_SEND {
                    for d in receiver.get() {
                        sum += d.iter().copied().sum::<u64>();
                    }
                    receiver.advance();
                }
                sum
            });
            let dt = Instant::now() - first;
            println!("read {:?} {} {:e}", dt, sum, (TO_SEND - 1) as f64 / dt.as_secs_f64());
        });
    
        a.join().unwrap();
        b.join().unwrap();
    }

    println!();
    println!("std");
    {
        let (sender, receiver) = std::sync::mpsc::sync_channel(QUEUE_SIZE);
    
        let a = std::thread::spawn(move || {
            let data: Vec<_> = (0..TO_SEND).map(|v| [v, v+1, v+2]).collect();

    
            let start = Instant::now();
            let sum = black_box({
                let mut sum: u64 = 0;
                for d in data {
                    sum += d.iter().sum::<u64>();
                    sender.send(vec![d]).unwrap();
                }
                sum
            });
            let dt = Instant::now() - start;
            println!("write {:?} {} {:e}", dt, sum, TO_SEND as f64 / dt.as_secs_f64());
        });
    
        let b = std::thread::spawn(move || {
            let start = Instant::now();
            let mut first = start;
            let sum = black_box({
                let mut sum: u64 = 0;
                let v = receiver.recv().unwrap();
                first = Instant::now();
                for d in v.iter() {
                    sum += d.iter().copied().sum::<u64>();
                }
                for _ in 1..TO_SEND {
                    let v = receiver.recv().unwrap();
                    for d in v.iter() {
                        sum += d.iter().copied().sum::<u64>();
                    }
                }
                sum
            });
            let dt = Instant::now() - first;
            println!("read {:?} {} {:e}", dt, sum, (TO_SEND - 1) as f64 / dt.as_secs_f64());
        });
    
        a.join().unwrap();
        b.join().unwrap();
    }
}