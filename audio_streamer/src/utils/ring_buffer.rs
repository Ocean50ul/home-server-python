use std::marker::PhantomData;
use std::cell::Cell;
use crate::utils::sync::{cell, Arc, AtomicUsize, Ordering};

struct RingBufferCoreF32<const N: usize> {
    buffer: [cell::UnsafeCell<f32>; N],
    head: AtomicUsize,
    tail: AtomicUsize
}

unsafe impl<const N: usize> Send for RingBufferCoreF32<N> {}
unsafe impl<const N: usize> Sync for RingBufferCoreF32<N> {}

impl<const N: usize> RingBufferCoreF32<N> {
    fn new() -> Self {
        Self {
            buffer: std::array::from_fn(|_| cell::UnsafeCell::new(0.0)),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0)
        }
    }
}

/// The producer handle. Can only push.
pub struct Producer<const N: usize> {
    core: Arc<RingBufferCoreF32<N>>,
    _send_but_not_sync: PhantomData<Cell<()>>
}

impl<const N: usize> Producer<N> {
    fn available_space(&self, head: &usize, tail: &usize) -> usize {
        if head >= tail {
            return N - head + tail - 1;
        } else {
            return tail - head - 1;
        }
    }

    pub fn push_slice(&mut self, data: &[f32]) -> usize {
        let mut head_i = self.core.head.load(Ordering::Relaxed);
        let tail_i = self.core.tail.load(Ordering::Acquire);

        let amount_to_push = self.available_space(&head_i, &tail_i).min(data.len());

        for i in 0..amount_to_push {
            
            self.core.buffer[head_i].with_mut(|ptr| {
                unsafe { *ptr = data[i] }
            });

            head_i = (head_i + 1) & (N - 1);
        }

        self.core.head.store(head_i, Ordering::Release);

        return amount_to_push;
    }

    pub fn push(&mut self, data: f32) -> bool {
        let head_i = self.core.head.load(Ordering::Relaxed);
        let tail_i = self.core.tail.load(Ordering::Acquire);

        // check if the buffer is full
        if (head_i + 1) & (N - 1) == tail_i {
            return false;
        }

         self.core.buffer[head_i].with_mut(|ptr| {
            unsafe { *ptr = data; }
        });

        self.core.head.store((head_i + 1) & (N - 1), Ordering::Release);

        return true;
    }
}

/// The consumer handle. Can only pop.
pub struct Consumer<const N: usize> {
    core: Arc<RingBufferCoreF32<N>>,
    _send_but_not_sync: PhantomData<Cell<()>>
}

impl<const N: usize> Consumer<N> {
    pub fn pop(&mut self) -> Option<f32> {
        let head_i = self.core.head.load(Ordering::Acquire);
        let tail_i = self.core.tail.load(Ordering::Relaxed);

        if head_i == tail_i {
            return None;
        }

        let data = self.core.buffer[tail_i].with_mut(|ptr| {
            unsafe { *ptr }
        });

        self.core.tail.store((tail_i + 1) & (N - 1), Ordering::Release);

        return Some(data);
    }

    fn available_data(&self, head: &usize, tail: &usize) -> usize {
        if head >= tail {
            return head - tail;
        } else {
            return N - tail + head;
        }
    }

    pub fn pop_slice(&mut self, output: &mut [f32]) -> usize {
        let head_i = self.core.head.load(Ordering::Acquire);
        let mut tail_i = self.core.tail.load(Ordering::Relaxed);

        let amount_to_pop = self.available_data(&head_i, &tail_i).min(output.len());

        for i in 0..amount_to_pop {

            output[i] = self.core.buffer[tail_i].with_mut(|ptr| {
                unsafe { *ptr }
            });

            tail_i = (tail_i + 1) & (N - 1);
        }

        self.core.tail.store(tail_i, Ordering::Release);

        return amount_to_pop;
    }
}

/// Creates a new SPSC ring buffer with a usable capacity of N.
pub fn ring_buffer<const N: usize>() -> (Producer<N>, Consumer<N>) {
    const { assert!(N.is_power_of_two(), "RingBuffer capacity must be the power of two.") };
    let core = Arc::new(RingBufferCoreF32::new());
    (
        Producer { core: Arc::clone(&core), _send_but_not_sync: PhantomData },
        Consumer { core, _send_but_not_sync: PhantomData },
    )
}


#[cfg(test)]
mod tests {
    use super::*;
    use ctor::ctor;

    #[ctor]
    fn announce_test_mode() {
        #[cfg(loom)]
        {
            use std::env;
            println!("\n\n=============== RUNNING LOOM CONCURRENCY TESTS ===============");
            println!("Do <Remove-Item env:RUSTFLAGS> in order to run standard tests.");

            match env::var("LOOM_MAX_PREEMPTIONS") {
                Ok(max_p) => { println!("\nMax preemptions is {}. Do <Remove-Item env:LOOM_MAX_PREEMPTIONS> in order to run tests in exhaustive mode.", max_p) },
                Err(_) => { println!("\nMode: EXHAUSTIVE, this may take a while...\nSet <$env:LOOM_MAX_PREEMPTIONS=3> to do loom tests in non-exhaustive mode.\nThe lower max_preemptions the faster tests are, and the chances of skipping the bug are higher.") }
            }
        }
        
        #[cfg(not(loom))]
        {
                println!("\n\n=============== RUNNING STANDARD TESTS =======================");
                println!("Set <$env:RUSTFLAGS=\"--cfg loom\"> in order to run loom tests.");
        }

        println!("==============================================================");
    }

    #[cfg(not(loom))]
    mod standard_tests {
        use super::*;
        use crate::utils::sync::{thread, mpsc};
        
        const CAPACITY: usize = 128;

        #[test]
        fn test_st_push_and_pop() {
            let (mut producer, mut consumer) = ring_buffer::<CAPACITY>();
            let item_to_push = 42.0f32;

            producer.push(item_to_push);
            let popped = consumer.pop();

            assert!(popped.is_some());
            assert_eq!(popped.unwrap(), item_to_push);
        }

        #[test]
        fn test_st_push_full() {
            let (mut producer, mut _consumer) = ring_buffer::<CAPACITY>();
            
            for i in 0..CAPACITY - 1 {
                producer.push(i as f32);
            }

            let pushed_to_full = producer.push(4269f32);
            assert!(!pushed_to_full);
        }

        #[test]
        fn test_st_pop_empty() {
            let (mut _producer, mut consumer) = ring_buffer::<CAPACITY>();
            
            let nothing = consumer.pop();
            assert!(nothing.is_none());
        }

        #[test]
        fn test_st_push_and_pop_slices() {
            let (mut producer, mut consumer) = ring_buffer::<CAPACITY>();
            let slice_to_push = [42.0f32, 69.0f32];

            producer.push_slice(&slice_to_push);

            let mut popped = [0.0f32, 0.0f32];
            consumer.pop_slice(&mut popped);

            assert_eq!(popped, slice_to_push);
        }

        #[test]
        fn test_st_push_slice_full() {
            let (mut producer, mut _consumer) = ring_buffer::<CAPACITY>();
            let slice_to_push = (0..CAPACITY - 1)
                .map(|i| i as f32)
                .collect::<Vec<_>>();

            let pushed_amount = producer.push_slice(&slice_to_push);
            let pushed_to_full_amount = producer.push_slice(&[1.0f32, 2.0f32]);

            assert_eq!(pushed_amount, CAPACITY - 1);
            assert_eq!(pushed_to_full_amount, 0usize);
        }

        #[test]
        fn test_st_pop_slice_empty() {
            let (mut _producer, mut consumer) = ring_buffer::<CAPACITY>();
            let mut popping_buffer = vec![-1.0f32, -1.0f32];

            consumer.pop_slice(&mut popping_buffer);

            assert_eq!(popping_buffer, vec![-1.0f32, -1.0f32]);
        }

        #[test]
        fn test_mt_push_and_pop() {
            let (mut producer, mut consumer) = ring_buffer::<CAPACITY>();
            let item = 42.0f32;
            let (tx, rx) = mpsc::channel();

            let producer_handle = thread::spawn(move || {
                producer.push(item);
                tx.send(()).expect("Failed to send a thing through a channel thingy, uWu.");
            });

            let consumer_handle = thread::spawn(move || {
                rx.recv().expect("Failed to recv a thing through a channel thingy, uWu.");
                consumer.pop()
            });

            producer_handle.join().expect("Failed to join producer thread.");
            let popped_data = consumer_handle.join().expect("Failed to join consumer thread!");

            assert!(popped_data.is_some());
            assert_eq!(popped_data.unwrap(), item);
        }

        #[test]
        fn test_mt_push_full() {
            let (mut producer, mut _consumer) = ring_buffer::<CAPACITY>();

            let producer_handle = thread::spawn(move || {
                for i in 0..CAPACITY - 1 {
                    producer.push(i as f32);
                }

                producer.push(4269.0f32)
            });

            let push_result = producer_handle.join().expect("Failed to join producer thread.");
            assert!(!push_result);

        }

        #[test]
        fn test_mt_pop_empty() {
            let (mut _producer, mut consumer) = ring_buffer::<CAPACITY>();

            let consumer_handle = thread::spawn(move || {
                consumer.pop()
            });

            let popped_data = consumer_handle.join().expect("Failed to join consumer thread!");
            assert!(popped_data.is_none());        
        }

        #[test]
        fn test_mt_push_and_pop_slice() {
            let (mut producer, mut consumer) = ring_buffer::<CAPACITY>();
            let items = [42.42f32, 69.69f32];

            let (tx, rx) = mpsc::channel();
            let mut popped = [0.0, 0.0];

            thread::scope(|s| {
                s.spawn(move || {
                    producer.push_slice(&items);
                    tx.send(()).expect("Failed to send a thing through a channel thingy, uWu.");
                });

                let popped = &mut popped;

                s.spawn(move || {
                    rx.recv().expect("Failed to recv a thing through a channel thingy, uWu.");
                    consumer.pop_slice(popped);
                });
            });

            assert_eq!(popped, items);
        }

        #[test]
        fn test_mt_push_slice_full() {
            let (mut producer, mut _consumer) = ring_buffer::<CAPACITY>();
            let items = (0..CAPACITY - 1)
                .map(|i| i as f32)
                .collect::<Vec<_>>();

            let producer_handle = thread::spawn(move || {
                (
                    producer.push_slice(&items),
                    producer.push_slice(&[1.0, 2.0])
                )
            });

            let (push_1_result, push_2_result) = producer_handle.join().expect("Failed to join producer's handle!");

            assert_eq!(push_1_result, CAPACITY - 1);
            assert_eq!(push_2_result, 0usize);

        }

        #[test]
        fn test_mt_pop_slice_empty() {
            let (mut _producer, mut consumer) = ring_buffer::<CAPACITY>();
            let meme_n1 = 69.69;
            let meme_n2 = 42.42;

            let mut popped = [meme_n1, meme_n2];

            thread::scope(|s| {
                s.spawn(|| {
                    consumer.pop_slice(&mut popped);
                });
            });

            assert!(popped.contains(&meme_n1));
            assert!(popped.contains(&meme_n2));
        }
    }

    #[cfg(loom)]
    mod loom_tests {
        use super::*;
        use crate::utils::sync::{thread, mpsc};
        use std::io::{stdout, Write};

        #[test]
        fn test_singles_with_monotonic_counter() {
            // let branch_counter = std::sync::atomic::AtomicU64::new(0);

            loom::model(|| {
                const N: usize = 2;
                let amount = 4;

                let (mut producer, mut consumer) = ring_buffer::<N>();

                let producer_handle = thread::spawn(move || {
                    let mut counter = 0;

                    for _ in 0..amount {
                        
                        // keep pushing until we succeed
                        while !producer.push(counter as f32) {
                            // yeild if the push has returned false
                            thread::yield_now();
                        }
                        counter += 1;
                    }

                    counter
                });

                let consumer_handle = thread::spawn(move || {
                    let mut monotonic_counter = 0;

                    // keep popping until there is data
                    while monotonic_counter < amount {
                        if let Some(popped_data) = consumer.pop() {

                            assert_eq!(popped_data, monotonic_counter as f32);
                            monotonic_counter += 1;
                        
                        } else {
                            // yield if pop has returned nothing
                            thread::yield_now(); 
                        }
                    }

                    monotonic_counter
                });

                let (counter, expected_counter) = (
                    producer_handle.join().expect("error"),
                    consumer_handle.join().expect("error")
                );

                assert_eq!(counter, expected_counter);
                assert_eq!(counter, amount);

                // let branch_num = branch_counter.fetch_add(1, Ordering::Relaxed);
                // if branch_num % 1000 == 0 {
                //     print!("\rLoom branches explored: {}", branch_num);
                //     stdout().flush().unwrap();
                // }
            });

            // println!("\nLoom exploration complete.");
        }

        #[test]
        fn test_slices_with_monotonic_counter() {
            // let branch_counter = std::sync::atomic::AtomicU64::new(0);

            loom::model(move || {
                const N: usize = 4;

                let (mut producer, mut consumer) = ring_buffer::<N>();

                let producer_handle = thread::spawn(move || {
                    let data = vec![0.0f32, 1.0f32, 2.0f32, 3.0f32, 4.0f32, 5.0f32];
                    let mut pushed_counter = 0usize;

                    while pushed_counter < data.len() {
                        let pushed_amount = producer.push_slice(&data[pushed_counter..]);

                        if pushed_amount == 0 {
                            thread::yield_now();
                        }

                        pushed_counter += pushed_amount;
                    }

                    pushed_counter

                });

                let consumer_handle = thread::spawn(move || {
                    let mut popped_items: Vec<f32> = Vec::with_capacity(6);
                    let mut monotonic_counter = 0usize;

                    while popped_items.len() < 6 {
                        let mut popped_buffer = vec![-1.0f32; N - 1];
                        let popped_amount = consumer.pop_slice(&mut popped_buffer);

                        if popped_amount == 0 {
                            thread::yield_now();
                            continue;
                        }

                        for i in 0..popped_amount {
                            let popped_item = popped_buffer[i];

                            assert_eq!(popped_item, monotonic_counter as f32);
                            monotonic_counter += 1;
                        }

                        popped_items.extend(&popped_buffer[..popped_amount]);
                    }

                    popped_items
                });

                let (pushed_counter, popped_items) = (
                    producer_handle.join().expect("error"),
                    consumer_handle.join().expect("error")
                );

                assert_eq!(pushed_counter, popped_items.len());
                assert_eq!(popped_items, vec![0.0f32, 1.0f32, 2.0f32, 3.0f32, 4.0f32, 5.0f32]);

                // let branch_num = branch_counter.fetch_add(1, Ordering::Relaxed);
                // if branch_num % 1000 == 0 {
                //     print!("\rLoom branches explored: {}", branch_num);
                //     stdout().flush().unwrap();
                // }
            });

            // println!("\nLoom exploration complete.");
        }

        #[test]
        fn test_boundry_singles() {
            // let branch_counter = std::sync::atomic::AtomicU64::new(0);

            loom::model(move || {
                const N: usize = 4;
                const HAMMER_TIMES: usize = 10;

                let (mut producer, mut consumer) = ring_buffer::<N>();
                producer.push_slice(&[0.0f32, 1.0f32, 2.0f32]);

                let (space_tx, space_rx) = mpsc::channel();
                let (data_tx, data_rx) = mpsc::channel();

                let produce_handle = thread::spawn(move || {
                    for i in 3..HAMMER_TIMES + 3 {
                        // receive the message - buffer has one empty slot, we need to make it full
                        // break if one of the loops has finished
                        if space_rx.recv().is_err() { break; }
                        assert!(producer.push(i as f32));

                        // send the message - buffer full again
                        // break if one of the loops has finished
                        if data_tx.send(()).is_err() { break; }
                    }
                });

                let consumer_handle = thread::spawn(move || {
                    for i in 0..HAMMER_TIMES {

                        let popped = consumer.pop();
                        assert!(popped.is_some());
                        assert_eq!(i as f32, popped.unwrap());

                        // send the message - buffer has one empty slot
                        // break if one of the loops has finished
                        if space_tx.send(()).is_err() { break; }

                        // receive the message - buffer is full, we can pop one item
                        // break if one of the loops has finished
                        if data_rx.recv().is_err() { break; }
                    }

                });

                produce_handle.join().unwrap();
                consumer_handle.join().unwrap();

                // let branch_num = branch_counter.fetch_add(1, Ordering::Relaxed);
                
                // print!("\rLoom branches explored: {}", branch_num);
                // stdout().flush().unwrap();
                
            });

            println!("\nLoom exploration complete.");

        }
    }

}