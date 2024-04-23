use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub struct RingBuffer<T> {
    write: Inner<T>,
    read: Inner<T>,
    _marker: PhantomData<T>,
}

#[repr(align(64))]
struct Inner<T> {
    data_ptr: *mut T,
    capacity: usize,
    idx: AtomicUsize,
}

pub struct Sender<T> {
    buffer: Arc<RingBuffer<T>>,
    read_idx_cached: usize,
    uncommited: usize,
}

pub struct Receiver<T> {
    buffer: Arc<RingBuffer<T>>,
    write_idx_cached: usize,
    uncommited: usize,
}

unsafe impl<T: Send> Send for Sender<T> {}
unsafe impl<T: Send> Send for Receiver<T> {}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> (Sender<T>, Receiver<T>) {
        assert!(capacity > 0 && capacity <= (usize::MAX >> 1));
        let capacity = capacity.next_power_of_two();
        let data_ptr = ManuallyDrop::new(Vec::with_capacity(capacity)).as_mut_ptr();

        let buffer = Arc::new(Self {
            write: Inner {
                data_ptr,
                capacity,
                idx: AtomicUsize::new(0),
            },
            read: Inner {
                data_ptr,
                capacity,
                idx: AtomicUsize::new(0),
            },
            _marker: PhantomData,
        });
        let sender = Sender {
            buffer: buffer.clone(),
            read_idx_cached: 0,
            uncommited: 0,
        };
        let receiver = Receiver {
            buffer,
            write_idx_cached: 0,
            uncommited: 0,
        };
        (sender, receiver)
    }
}

impl<T> Sender<T> {
    pub fn send(&mut self, value: T) {
        let capacity = self.buffer.write.capacity;
        let write_idx = self.buffer.write.idx.load(Ordering::Relaxed);

        if write_idx == self.read_idx_cached + capacity {
            self.read_idx_cached = self.buffer.read.idx.load(Ordering::Acquire);

            while write_idx == self.read_idx_cached + capacity {
                wait();
                self.read_idx_cached = self.buffer.read.idx.load(Ordering::Acquire);
            }
        }

        unsafe {
            self.buffer
                .write
                .data_ptr
                .add(write_idx & (capacity - 1))
                .write(value);
        }
        self.buffer
            .write
            .idx
            .store(write_idx + 1, Ordering::Release);
    }

    pub fn begin_commit(&mut self, reserve: usize) {
        let capacity = self.buffer.write.capacity;
        let write_idx = self.buffer.write.idx.load(Ordering::Relaxed);

        if write_idx + reserve >= self.read_idx_cached + capacity {
            self.read_idx_cached = self.buffer.read.idx.load(Ordering::Acquire);

            while write_idx + reserve >= self.read_idx_cached + capacity {
                wait();
                self.read_idx_cached = self.buffer.read.idx.load(Ordering::Acquire);
            }
        }
    }

    pub fn send_unsafe(&mut self, value: T) {
        let capacity = self.buffer.write.capacity;
        let write_idx = self.buffer.write.idx.load(Ordering::Relaxed);

        debug_assert!(write_idx + self.uncommited < self.read_idx_cached + capacity);
        unsafe {
            self.buffer
                .write
                .data_ptr
                .add((write_idx + self.uncommited) & (capacity - 1))
                .write(value);
        }
        self.uncommited += 1;
    }

    pub fn end_commit(&mut self) {
        let write_idx = self.buffer.write.idx.load(Ordering::Relaxed);

        self.buffer
            .write
            .idx
            .store(write_idx + self.uncommited, Ordering::Release);
        self.uncommited = 0;
    }
}

impl<T> Receiver<T> {
    pub fn recv(&mut self) -> T {
        let capacity = self.buffer.read.capacity;
        let read_idx = self.buffer.read.idx.load(Ordering::Relaxed);

        if read_idx == self.write_idx_cached {
            self.write_idx_cached = self.buffer.write.idx.load(Ordering::Acquire);

            while read_idx == self.write_idx_cached {
                wait();
                self.write_idx_cached = self.buffer.write.idx.load(Ordering::Acquire);
            }
        }

        let value = unsafe {
            self.buffer
                .read
                .data_ptr
                .add(read_idx & (capacity - 1))
                .read()
        };
        self.buffer.read.idx.store(read_idx + 1, Ordering::Release);
        value
    }

    pub fn begin_commit(&mut self, reserve: usize) {
        let read_idx = self.buffer.read.idx.load(Ordering::Relaxed);

        if read_idx + reserve >= self.write_idx_cached {
            self.write_idx_cached = self.buffer.write.idx.load(Ordering::Acquire);

            while read_idx + reserve >= self.write_idx_cached {
                wait();
                self.write_idx_cached = self.buffer.write.idx.load(Ordering::Acquire);
            }
        }
    }

    pub fn recv_unsafe(&mut self) -> T {
        let capacity = self.buffer.read.capacity;
        let read_idx = self.buffer.read.idx.load(Ordering::Relaxed);

        debug_assert!(read_idx + self.uncommited < self.write_idx_cached);

        let value = unsafe {
            self.buffer
                .read
                .data_ptr
                .add((read_idx + self.uncommited) & (capacity - 1))
                .read()
        };
        self.uncommited += 1;
        value
    }

    pub fn end_commit(&mut self) {
        let read_idx = self.buffer.read.idx.load(Ordering::Relaxed);

        self.buffer
            .read
            .idx
            .store(read_idx + self.uncommited, Ordering::Release);
        self.uncommited = 0;
    }
}

fn wait() {
    std::hint::spin_loop();
}

impl<T> Drop for RingBuffer<T> {
    fn drop(&mut self) {
        // TODO: Drop all remaing values
        unsafe {
            Vec::from_raw_parts(self.read.data_ptr, 0, self.read.capacity);
        }
    }
}
