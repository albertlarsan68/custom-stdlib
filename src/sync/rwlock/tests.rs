use crate::sync::atomic::{AtomicUsize, Ordering};
use crate::sync::mpsc::channel;
use crate::sync::{Arc, RwLock, RwLockReadGuard};
use crate::thread;
use rand::Rng;

#[derive(Eq, PartialEq, Debug)]
struct NonCopy(i32);

#[test]
fn smoke() { //FIXME
    let l = RwLock::new(());
    drop(l.read());
    drop(l.write());
    drop((l.read(), l.read()));
    drop(l.write());
}

#[test]
fn frob() { //FIXME
    const N: u32 = 10;
    const M: usize = if cfg!(miri) { 100 } else { 1000 };

    let r = Arc::new(RwLock::new(()));

    let (tx, rx) = channel::<()>();
    for _ in 0..N {
        let tx = tx.clone();
        let r = r.clone();
        thread::spawn(move || {
            let mut rng = rand::thread_rng();
            for _ in 0..M {
                if rng.gen_bool(1.0 / (N as f64)) {
                    drop(r.write());
                } else {
                    drop(r.read());
                }
            }
            drop(tx);
        });
    }
    drop(tx);
    let _ = rx.recv();
}

#[test]
fn test_rw_arc() { //FIXME
    let arc = Arc::new(RwLock::new(0));
    let arc2 = arc.clone();
    let (tx, rx) = channel();

    println!("Created");
    
    thread::spawn(move || {
        let mut lock = arc2.write();
        for _ in 0..10 {
            let tmp = *lock;
            *lock = -1;
            thread::yield_now();
            *lock = tmp + 1;
        }
        tx.send(()).unwrap();
        println!("Writer finished");
    });
    
    println!("Writer spawned");
    
    // Readers try to catch the writer in the act
    let mut children = Vec::new();
    for i in 0..5 {
        let arc3 = arc.clone();
        children.push(thread::Builder::new().name(format!("{i}")).spawn(move || {
            let lock = arc3.read();
            assert!(*lock >= 0);
        }).unwrap());
        println!("Children {i} spawned");
    }

    println!("Readers spawned");

    // Wait for children to pass their asserts
    for (i, r) in children.into_iter().enumerate() {
        println!("Joining reader {i}");
        assert!(r.join().is_ok());
        println!("Reader {i} joined");
        println!("{:?}", arc.lock.state.lock().unwrap());
    }

    println!("Readers finished");
    
    // Wait for writer to finish
    rx.recv().unwrap();
    println!("Writer joined");
    let lock = arc.read();
    assert_eq!(*lock, 10);
}

#[test]
fn test_rw_arc_access_in_unwind() {
    let arc = Arc::new(RwLock::new(1));
    let arc2 = arc.clone();
    let _ = thread::spawn(move || -> () {
        struct Unwinder {
            i: Arc<RwLock<isize>>,
        }
        impl Drop for Unwinder {
            fn drop(&mut self) {
                let mut lock = self.i.write();
                *lock += 1;
            }
        }
        let _u = Unwinder { i: arc2 };
        panic!();
    })
    .join();
    let lock = arc.read();
    assert_eq!(*lock, 2);
}

#[test]
fn test_rwlock_unsized() {
    let rw: &RwLock<[i32]> = &RwLock::new([1, 2, 3]);
    {
        let b = &mut *rw.write();
        b[0] = 4;
        b[2] = 5;
    }
    let comp: &[i32] = &[4, 2, 5];
    assert_eq!(&*rw.read(), comp);
}

#[test]
fn test_rwlock_try_write() {
    let lock = RwLock::new(0isize);
    let read_guard = lock.read();

    let write_result = lock.try_write();
    match write_result {
        None => {}
        Some(_) => panic!("try_write should not succeed while read_guard is in scope"),
    }

    drop(read_guard);
}

#[test]
fn test_into_inner() {
    let m = RwLock::new(NonCopy(10));
    assert_eq!(m.into_inner(), NonCopy(10));
}

#[test]
fn test_into_inner_drop() {
    struct Foo(Arc<AtomicUsize>);
    impl Drop for Foo {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let num_drops = Arc::new(AtomicUsize::new(0));
    let m = RwLock::new(Foo(num_drops.clone()));
    assert_eq!(num_drops.load(Ordering::SeqCst), 0);
    {
        let _inner = m.into_inner();
        assert_eq!(num_drops.load(Ordering::SeqCst), 0);
    }
    assert_eq!(num_drops.load(Ordering::SeqCst), 1);
}

#[test]
fn test_get_mut() {
    let mut m = RwLock::new(NonCopy(10));
    *m.get_mut() = NonCopy(20);
    assert_eq!(m.into_inner(), NonCopy(20));
}

#[test]
fn test_read_guard_covariance() {
    fn do_stuff<'a>(_: RwLockReadGuard<'_, &'a i32>, _: &'a i32) {}
    let j: i32 = 5;
    let lock = RwLock::new(&j);
    {
        let i = 6;
        do_stuff(lock.read(), &i);
    }
    drop(lock);
}
