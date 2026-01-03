use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

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

impl <T, const SIZE: usize> Sender<[T; SIZE]> {
    pub fn get(&mut self) -> &mut T {
        let write = self.write;
        let next_write = (write + 1) % SIZE;
        if next_write == self.read {
            loop {
                self.read = self.queue.read.load(Ordering::Acquire);
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

impl <T, const SIZE: usize> Receiver<[T; SIZE]> {
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

    pub fn try_get<'a>(&'a mut self) -> Option<&'a T> {
        let read = self.read;

        if read == self.write {
            self.write = self.queue.write.load(Ordering::Acquire);
            if self.write == read {
                return None;
            }
        }

        Some(&self.queue.values[read])
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
    
    let (sum, dt1) =
    {
        let (mut sender, mut receiver) = channel_using::<[Vec<_>; _]>(
            [(); QUEUE_SIZE].map(|_| Default::default())
        );
        let data: Vec<_> = (0..TO_SEND).map(|v| [v, v+1, v+2]).collect();
    
        let a = std::thread::spawn(move || {
            let start = Instant::now();
            let first;
            let sum = black_box({
                let mut sum = 0u64;
                let d = &data[0];
                sum += d.iter().sum::<u64>();
                let s = sender.get();
                s.clear();
                s.push(*d);
                sender.advance();
                first = Instant::now();

                for d in &data[1..] {
                    sum += d.iter().sum::<u64>();
                    let s = sender.get();
                    s.clear();
                    s.push(*d);
                    sender.advance();
                }
                sum
            });
            let dt = Instant::now() - first;
            println!("write {:?} {:?} {} {:e}", first - start, dt, sum, (TO_SEND - 1) as f64 / dt.as_secs_f64());

            return first;
        });
    
        let b = std::thread::spawn(move || {
            let start = Instant::now();
            let first;
            let sum = black_box({
                let mut sum: u64 = 0;
                let slice = receiver.get();
                first = Instant::now();
                for d in slice.iter() {
                    sum += d.iter().copied().sum::<u64>();
                }
                receiver.advance();
                for _ in 1..TO_SEND {
                    for d in receiver.get().iter() {
                        sum += d.iter().copied().sum::<u64>();
                    }
                    receiver.advance();
                }
                sum
            });
            let end = Instant::now();
            let dt = end - first;
            println!("read {:?} {:?} {} {:e}", first - start, dt, sum, (TO_SEND - 1) as f64 / dt.as_secs_f64());

            return (first, sum, dt);
        });
    
        let a = a.join().unwrap();
        let (b, sum, dt) = b.join().unwrap();

        println!("{:?}", b - a);

        (sum, dt)
    };

    println!();
    println!("std");

    let (sum2, dt2) =
    {
        let (sender, receiver) = std::sync::mpsc::sync_channel(QUEUE_SIZE);
        let data: Vec<_> = (0..TO_SEND).map(|v| [v, v+1, v+2]).collect();
    
        let a = std::thread::spawn(move || {

            let first;
            let start = Instant::now();
            let sum = black_box({
                let mut sum: u64 = 0;
                let d = &data[0];
                sum += d.iter().sum::<u64>();
                sender.send(vec![*d]).unwrap();
                first = Instant::now();

                for d in &data[1..] {
                    sum += d.iter().sum::<u64>();
                    sender.send(vec![*d]).unwrap();
                }
                sum
            });
            let end = Instant::now();
            let dt = end - first;
            println!("write {:?} {:?} {} {:e}", first - start, dt, sum, (TO_SEND - 1) as f64 / dt.as_secs_f64());

            return first;
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
            let end = Instant::now();
            let dt = end - first;
            println!("read {:?} {:?} {} {:e}", first-start, dt, sum, (TO_SEND - 1) as f64 / dt.as_secs_f64());

            return (first, sum, dt);
        });
    
        let a = a.join().unwrap();
        let (b, sum, dt) = b.join().unwrap();

        println!("{:?}", b - a);

        (sum, dt)
    };

    assert_eq!(sum, sum2);
    assert!(dt1 * 5 < dt2);
}