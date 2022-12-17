#![feature(new_uninit)]
#![feature(int_roundings)]
#![feature(once_cell)]
#![feature(strict_provenance)]
#![feature(adt_const_params)]
#![feature(arbitrary_self_types)]
#![feature(const_result_drop)]
#![feature(const_option)]
#![feature(strict_provenance_atomic_ptr)]
#![feature(core_intrinsics)]
#![feature(const_option_ext)]
#![feature(inline_const)]
#![feature(thin_box)]
#![feature(coerce_unsized)]
#![feature(ptr_metadata)]
#![feature(generic_const_exprs)]

mod linked_list;
mod rustlings;
mod doubly_linked_list;
mod swap_arc;
mod fair_mutex;
mod unfair_mutex;
mod fair_mutex_minimal;
mod unfair_mutex_minimal;
mod sharable_lock;
mod atomic_stack;
mod inlinable_ptr;
mod stable_inlinable_ptr;
// mod inline_dyn;

use std::arch::asm;
use std::cmp::Ordering;
use std::{io, mem, thread};
use std::hint::black_box;
use std::io::{Error, ErrorKind, Read, Write};
use std::mem::{ManuallyDrop, transmute};
use std::sync::{Arc, atomic, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use parking_lot::Mutex;
use rand::{Rng, thread_rng};
use serde::{Deserialize, Serialize};
use crate::doubly_linked_list::{AtomicDoublyLinkedList, NodeKind};
use crate::fair_mutex::FairMutex;
use crate::fair_mutex_minimal::TICKET_MASK;
use crate::linked_list::LinkedList;
use crate::rustlings::test_main;
use crate::unfair_mutex::UnfairMutex;

fn main() {
    /*let mut fake_mutex = fair_mutex_minimal::FairMutex::new(30);
    let guard = black_box(fair_mutex_minimal::lock_cool_name(&fake_mutex));
    black_box(fair_mutex_minimal::drop_cool_name(&guard));
    black_box(fair_mutex_minimal::lock_slow_cool_name(&fake_mutex, fake_mutex.state.load(atomic::Ordering::Acquire) & TICKET_MASK));*/
    /*let stack_pointer: u64;
    let stack_base_pointer: u64;
    let stack_pointer_2: u64;
    unsafe {
        asm!(
        "mov rax, 0",
        "push rax",
        "mov {}, rsp",
        "mov {}, rbp",
        "pop rax",
        "mov {}, rsp",
        out(reg) stack_pointer,
        out(reg) stack_base_pointer,
        out(reg) stack_pointer_2,
        out("rax") _,
        );
    }
    println!("stack pointer: {}", stack_pointer);
    println!("stack base pointer: {}", stack_base_pointer);
    println!("stack 2 pointer: {}", stack_pointer_2);*/

    /*
    let mut tmp = Vec::new();
    loop {
        let rand = thread_rng().gen_range('A'..'Z');
        println!("Buchstabe: `{}`", rand);
        io::stdin().read_to_end(&mut tmp);
    }
    */

    /*
    let rand = thread_rng().gen_range('A'..'Z');
    println!("Buchstabe: `{}`", rand);*/


    // let test = std::arch::x86_64::__cpuid()
    /*unsafe {
        asm!("nop");
    }*/
    //let mut test = vec![];
    // for (x = 0; x < LIMIT; x -= 2) {
    /*for x in 0..LIMIT {
        test.push(x);
    }*/
    /*let result = test.iter_mut()/*.filter_map(|x| {
        if x % 2 == 0 {
            Some(x)
        } else {
            None
        }
    })*/
        .filter(|x| x % 2 == 0)
        .rev().skip(2).enumerate().filter(|(number, id)| number % 10 == 0 && id % 2 == 0)
        .map(|(number, id)| (number as u128) & (*id as u128)).collect::<Vec<u128>>();
    let first = input("Insert the first number: ".to_string()).unwrap().parse::<u64>().unwrap();
    let second = input("Insert the second number: ".to_string()).unwrap().parse::<u64>().unwrap();*/
    // println!("{}", eq(first, second));
    /*let mut linked_list = LinkedList::default();
    linked_list.insert(56).insert(34);

    test_main();*/

    /*let tmp: Arc<SwapArcIntermediate<i32, Arc<i32>, 0>> = SwapArcIntermediate::new(Arc::new(52));
    let mut threads = vec![];
    tmp.update(Arc::new(31));
    println!("{}", tmp.load());
    for _ in 0..20/*5*//*1*/ {
        let tmp = tmp.clone();
        threads.push(thread::spawn(move || {
            for x in 0..10/*200*/ {
                println!("{}", tmp.load());
                if x % 5 == 0 {
                    println!("completed push: {x}");
                }
                // thread::sleep(Duration::from_millis(1000));
            }
        }));
    }
    for _ in 0..20/*5*//*1*/ {
        // let send = send.clone();
        let tmp = tmp.clone();
        threads.push(thread::spawn(move || {
            // let send = send.clone();
            for x in 0..10/*200*/ {
                /*
                thread::sleep(Duration::from_millis(500));
                println!("{:?}", list.remove_head());
                thread::sleep(Duration::from_millis(500));*/
                // send.send(list.remove_head()).unwrap();
                tmp.update(Arc::new(rand::random()));

                if x % 5 == 0 {
                    println!("completed removals: {x}");
                }
            }
        }));
    }
    threads.into_iter().for_each(|thread| thread.join().unwrap());*/

    // loop {}
    /*
    let doubly_linked_list: Arc<AtomicDoublyLinkedList<i32, { NodeKind::Bound }>> = AtomicDoublyLinkedList::new();
    /*let mut threads = vec![];
    // let (mut send, mut recv) = mpsc::channel();
    // FIXME: with only adder threads, a normal execution finishes, but a miri execution (at least seems to) loop infinitely - but this could also just be because of worse
    // FIXME: throughput on miri's side

    // FIXME: a "battle" (1 adder and 1 remover thread) finishes on both miri and without miri - that is for 200 additions/removals, for 500 on the other hand
    // FIXME: a normal run will complete, though printing that the list isn't empty, which shouldn't be the case. - when doing 500 additions/removals in miri,
    // FIXME: the correct result gets computed

    // FIXME: everything works up to 20 pusher and 10 remover threads at least and up to 2000 iters per thread at least
    // FIXME: (30.10.22 | normal execution finishes cleanly while miri gets stuck in an infinite loop at some point in time)
    for _ in 0..10/*20*//*20*//*5*//*1*/ {
        let list = doubly_linked_list.clone();
        threads.push(thread::spawn(move || {
            for x in 0..50/*2000*//*25*//*50*//*200*//*0*//*200*/ {
                list.push_head(x);
                if x % 5 == 0 {
                    println!("completed push: {x}");
                }
                // thread::sleep(Duration::from_millis(1000));
            }
        }));
    }
    thread::sleep(Duration::from_secs(1));
    /*for _ in 0..10/*20*//*5*//*1*/ {
        // let send = send.clone();
        let list = doubly_linked_list.clone();
        threads.push(thread::spawn(move || {
            // let send = send.clone();
            for x in 0..2000/*200*/ {
                /*
                thread::sleep(Duration::from_millis(500));
                println!("{:?}", list.remove_head());
                thread::sleep(Duration::from_millis(500));*/
                // send.send(list.remove_head()).unwrap();
                /*mem::forget(*/list.remove_head()/*)*/;
                if x % 5 == 0 {
                    println!("completed removals: {x}");
                }
            }
        }));
    }*/
    /*mem::forget(doubly_linked_list.push_head(0)); // this line alone in combination with Unbound leads to an infinite cycle - for some reason the program doesn't end but list empty still gets printed
    mem::forget(doubly_linked_list.push_head(1));
    mem::forget(doubly_linked_list.push_head(2));
    mem::forget(doubly_linked_list.push_head(3));
    println!("PUSHED!");*/
    // mem::forget(doubly_linked_list.remove_head());
    /*
    /*mem::forget(*/doubly_linked_list.remove_head()/*)*/; // if this is the only call, this works
    println!("REMOVED 1");
    /*mem::forget(*/doubly_linked_list.remove_head()/*)*/; // FIXME: this causes a data race, so we just forget the return value!
    // thread::sleep(Duration::from_millis(20000));
   //  threads.into_iter().for_each(|thread| thread.join().unwrap());
    */
    // threads.into_iter().for_each(|thread| thread.join().unwrap());
    threads.into_iter().for_each(|thread| thread.join().unwrap());*/
    /*doubly_linked_list.remove_head();
    doubly_linked_list.remove_head();
    doubly_linked_list.remove_head();
    doubly_linked_list.remove_head();*/
    doubly_linked_list.push_head(52);
    println!("list empty: {}", doubly_linked_list.is_empty());
    if doubly_linked_list.is_empty() {
        println!("Aggressive push/pop testsuite passed!");
    }
    */
    // println!("test: {:?}", test);
    // loop {}

    /*let mutex = Arc::new(/*parking_lot::Mutex*//*FairMutex*//*parking_lot::*/fair_mutex_minimal::FairMutex/*unfair_mutex_minimal::UnfairMutex*//*FairMutex*/::new(20));
    let mut threads = vec![];
    let start = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    for _ in 0..1/*2*//*2*//*8*//*20*//*5*//*1*/ {
        let tmp = mutex.clone();
        threads.push(thread::spawn(move || {
            let rand = rand::random();
            for _ in 0..200000000/*200*/ {
                let mut l1 = tmp.lock();
                // println!("val: {}", &*l1);
                black_box(&*l1);
                *l1 = rand;
                // thread::sleep(Duration::from_millis(1000));
            }
        }));
    }
    threads.into_iter().for_each(|thread| thread.join().unwrap());
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    println!("test took: {}ms", time - start);*/
}

/*
fn eq(x: u64, y: u64) -> bool {
    /*
    let res: u64;
    unsafe {
        asm!("sub {0}, {1}",
        inlateout(reg) x => res,
        in(reg) y);
    }
    res == 0 // transform resulting u64 to bool
    */
    let res: u64;
    unsafe {
        // x = 1 - {0}
        // y = x - (x * overflow) // either 0 or 1 (so this is covering the {0} condition
        asm!(
        "sub {2}, {2}", // clear 2. register
        "sub {0}, {1}", // compare both numbers
        "seto {2}",     // set overflow to 2. register
        "sub {3}, {3}", // clear 3. register
        "mov {3}, 1",    // TODO: Somehow remove the mov and replace it with sub (there are multiple possible approaches)
        "sub {5}, {5}",
        "sub {3}, {0}", // x = 1 - {0}
        "seto {5}",
        "sub {4}, {4}",
        "mov {4}, {3}", // TODO: Reverse and use sub instead of mov
        "mul {4}, {5}", // (x * overflow)
        "sub {6}, {6}",
        "mov {6}, {4}", // TODO: Reverse and use sub instead of mov
        // TODO: Fix this

        inout(reg) x => res,
        in(reg) y);
    }
    unsafe { transmute::<u8, bool>(res as u8) } // transform resulting u64 to bool
}*/

// sub x y address

/*
fn cmp(x: u64, y: u64) -> Ordering {
    let res: u64;
    unsafe {
        // less    | 0
        // equal   | 1
        // greater | 2
        asm!(
        "sub {2}, {2}", // clear the 2. register | // FIXME: Is this required?
        // do the initial comparing
        "sub {0}, {1}", // compare the two things
        "seto {2}",     // set the third register to the overflow
        "sub {1}, {1}", // clear 1. register as we don't need it anymore and can reuse it
        // "mul {2}, 2",
        // bring the output in a usable form and unify it
           // reverse the overflow bits
        "sub {1}, {2}", // reverse the bits of the 2. register
        "sub {1}, 1",   // this MAY also be required to reverse the bits
            /*
        // multiply the overflow by 2
        "sub {3}, {3}", // clear the 3. register
        "sub {3}, {1}",
        "sub {3}, {1}",
        "sub {4}, {4}", // clear the 4. register
        "sub {4}, {3}", // reverse the bits of the 3. register
        "sub {4}, 1",   // this MAY also be required to reverse the bits
        */
        // "sub {}",
        // TODO: the next things to do are: if {0} is 0 or {2} is 1, set it to 1, else set it to 0
        // x = 1 - {0}
        // y = x - (x * overflow) // either 0 or 1 (so this is covering the {0} condition
        // r = y + {2}


        inout(reg) x => _,
        in(reg) y,
        // out(reg) _,
        );
    }
    match res {
        0 => Ordering::Less,
        1 => Ordering::Equal,
        2 => Ordering::Greater,
        _ => unreachable!(),
    }
}*/

pub fn input(text: String) -> io::Result<String> {
    print!("{}", text);
    io::stdout().flush()?; // because print! doesn't flush
    let mut input = String::new();
    if io::stdin().read_line(&mut input)? == 0 {
        return Err(Error::new(
            ErrorKind::UnexpectedEof,
            "EOF while reading a line",
        ));
    }
    if input.ends_with('\n') {
        input.pop();
        if input.ends_with('\r') {
            input.pop();
        }
    }
    Ok(input)
}

/*
trait TestTrait {
    fn id(&self) -> usize;
}

struct TestStruct {
    id: usize,
}

impl TestTrait for TestStruct {
    fn id(&self) -> usize {
        self.id
    }
}

struct Builder<T: TestTrait> {
    with_id: Option<T>,
    some_num: usize,
}

impl<T: TestTrait> Builder<T> {
    fn new() -> Self {
        Self {
            with_id: None,
            some_num: 0
        }
    }

    fn with_id(mut self, with_id: T) -> Self {
        self.with_id = Some(with_id);
        self
    }

    fn some_num(mut self, some_num: usize) -> Self {
        self.some_num = some_num;
        self
    }

    async fn build(self) -> BuildResult {
        BuildResult::new(self).await
    }
}

struct BuildResult {
    id: usize,
    some_num: usize,
}

impl BuildResult {
    async fn new<T: TestTrait>(builder: Builder<T>) -> Self {
        Self {
            id: builder.with_id.unwrap().id(),
            some_num: builder.some_num,
        }
    }
}

async fn test() -> BuildResult {
    let with_id = TestStruct {
        id: 0,
    };
    Builder::new().some_num(2).with_id(with_id).build()
}*/
