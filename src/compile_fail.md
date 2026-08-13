# Compile-time contracts

These examples are expected to be rejected by the compiler. They protect the
ownership and capability boundaries without depending on rustc's diagnostic
wording.

Pinned values require the pin-aware branded borrow:

```compile_fail,E0277
use std::marker::PhantomPinned;
use o3::cell::brand;

struct Pinned(PhantomPinned);

brand::Token::scope(|mut token| {
    let cell = brand::Value::new(Pinned(PhantomPinned));
    let _ = cell.borrow_mut(&mut token);
});
```

A checked mutable borrow cannot escape its callback:

```compile_fail
use o3::cell::Checked;

fn escape(cell: &Checked<u8>) -> &mut u8 {
    cell.with_mut(|value| value)
}
```

A quota cycle cannot reset while an earlier reservation can still refund:

```compile_fail
use o3::mem::quota::{Lease, Ledger};

let mut ledger = Ledger::new(1);
let lease = Lease::<()>::reserve_all(&ledger);
ledger.reset(1);
drop(lease);
```

Unchecked queue insertion remains an explicit caller obligation:

```compile_fail,E0133
use o3::queue;

let queue = queue::Fifo::with_capacity(1);
queue::raw::Fifo::push_back_unchecked(&queue, 1_u8);
```

A raw hash entry cannot outlive the map borrow that proves its storage:

```compile_fail,E0515
use o3::collections::fixed::hash;

fn escape<'a>() -> hash::Entry<'a, u8> {
    let mut map = hash::Map::from_plan(hash::Plan::fixed::<1>());
    unsafe { hash::raw::Map::entry_unchecked(&mut map, 1, |_| false) }
}
```

An unchecked vacancy keeps its queue exclusively borrowed:

```compile_fail,E0515
use o3::collections::queue::fixed;

fn escape<'a>() -> fixed::Vacant<'a, u8> {
    let mut queue = fixed::Fifo::with_capacity(1);
    unsafe { fixed::raw::Fifo::vacant_entry_unchecked(&mut queue) }
}
```

An indexed queue vacancy retains its exclusive queue borrow:

```compile_fail,E0515
use o3::collections::queue::slot;

fn escape<'a>() -> slot::Vacant<'a, u8> {
    let mut queue = slot::Fifo::with_capacity(1);
    queue.vacant_entry(0).unwrap()
}
```

Shared queue mutation cannot expose a borrowed value:

```compile_fail,E0599
use o3::collections::queue::slot;

let queue = slot::Cell::with_capacity(1);
queue.push_back(0, 1_u8).unwrap();
let _ = queue.front();
```

Typed slab handles cannot be fabricated from public parts:

```compile_fail,E0624
use o3::collections::slab::key::{Handle, Parts};

let parts = Parts::new(0, 1).unwrap();
let _ = Handle::<()>::from_parts(parts);
```

Keys from distinct tag domains cannot be mixed:

```compile_fail,E0308
use o3::collections::slab::{Exclusive, key::Handle};

struct Read;
struct Write;

fn remove(slab: &mut Exclusive<u8, Write>, key: Handle<Read>) {
    slab.remove(key);
}
```

A lease cannot outlive its slab:

```compile_fail,E0515
use o3::collections::slab::{Capacity, lease};

fn escape() -> lease::Lease<'static, u8> {
    let slab = lease::Pool::with_capacity(Capacity::new(1));
    slab.vacant_entry().unwrap().insert(1)
}
```

A recycling lease retains the pool that owns its reusable seed:

```compile_fail,E0515
use o3::collections::slab::{Capacity, recycle};

struct Value;

impl recycle::Recycle for Value {
    type Seed = ();

    fn into_seed(self) {}
}

fn escape() -> recycle::Lease<'static, Value> {
    let pool = recycle::Pool::with_capacity(Capacity::new(1), || ());
    pool.vacant_entry().unwrap().insert_with(|()| Value)
}
```

A borrowed pinned recycling lease cannot escape its pool:

```compile_fail,E0515
use std::pin::Pin;
use o3::collections::{fixed::pinned::recycle, slab::Capacity};

struct Value;

impl recycle::Recycle for Value {
    fn recycle(self: Pin<&mut Self>) {}
}

fn escape() -> recycle::Lease<'static, Value> {
    let pool = recycle::Pool::with_capacity(Capacity::new(1), |_| Value);
    pool.reserve().unwrap().commit()
}
```

An owner-backed pinned recycling lease retains its exact owner domain:

```compile_fail
use std::pin::Pin;
use o3::collections::fixed::pinned::recycle;

struct Value;

impl recycle::Recycle for Value {
    fn recycle(self: Pin<&mut Self>) {}
}

fn shorten<'long: 'short, 'short>(
    lease: recycle::Lease<'long, Value>,
) -> recycle::Lease<'short, Value> {
    lease
}
```

Pinned recycling leases cannot cross their owning thread:

```compile_fail,E0277
use std::pin::Pin;
use o3::collections::fixed::pinned::recycle;

struct Value;

impl recycle::Recycle for Value {
    fn recycle(self: Pin<&mut Self>) {}
}

fn require_send<T: Send>() {}

require_send::<recycle::Lease<'static, Value>>();
```

Initialized pool storage cannot expose an uninitialized writer:

```compile_fail,E0599
use o3::buffer;

let pool = buffer::pool::Pool::<buffer::pool::state::Initialized>::try_new(1, 8).unwrap();
let mut lease = pool.try_acquire().unwrap();
let _ = lease.spare_writer();
```

Uninitialized pool storage cannot expose initialized spare bytes:

```compile_fail,E0599
use o3::buffer;

let pool = buffer::pool::Pool::<buffer::pool::state::Uninitialized>::try_new(1, 8).unwrap();
let mut lease = pool.try_acquire().unwrap();
let _ = lease.spare_mut();
```

Runtime-exact and compile-time-fixed owners remain distinct policy types:

```compile_fail,E0308
use o3::buffer::{BLOCK_CAPACITY, storage::Owned};

let exact = Owned::try_with_capacity(16).unwrap();
let _: Owned<BLOCK_CAPACITY> = exact;
```

A validated prefix keeps its target exclusively borrowed until commit:

```compile_fail,E0506
use o3::buffer::{PrefixConsumer, PrefixLength, PrefixProof};

struct Cursor { len: usize }

impl PrefixLength for Cursor {
    fn prefix_len(&self) -> usize { self.len }
}

impl PrefixConsumer for Cursor {
    fn consume_validated_prefix(&mut self, proof: PrefixProof) {
        self.len -= proof.amount();
    }
}

let mut cursor = Cursor { len: 8 };
let prefix = cursor.try_consume_prefix(4).unwrap();
cursor.len = 2;
prefix.commit();
```

Brand and region permissions are distinct domains:

```compile_fail,E0308
use o3::cell::{brand, region};

brand::Token::scope(|mut token| {
    let cell = region::Value::new(0_u8);
    *cell.borrow_mut(&mut token) = 1;
});
```

Tokens can be issued only by their generative scope:

```compile_fail,E0624
use o3::cell::region;

let _ = region::Token::new();
```

Cells from one generative brand cannot be accessed through another:

```compile_fail,E0521
use o3::cell::brand;

brand::Token::scope(|mut first| {
    brand::Token::scope(|mut second| {
        let cell = brand::Value::new(0_u8);
        let _ = cell.borrow_mut(&mut first);
        let _ = cell.borrow_mut(&mut second);
    });
});
```

Generation limits must be nonzero:

```compile_fail,E0080
use o3::collections::slab::key::Generation;

const _: Generation<0> = Generation::MIN;
```

Pinned slab keys retain their tag domain:

```compile_fail,E0308
use o3::collections::slab::{key::Handle, pinned};

struct Read;
struct Write;

fn remove(pool: &mut pinned::Pool<u8, Write>, key: Handle<Read>) {
    pool.remove(key);
}
```
