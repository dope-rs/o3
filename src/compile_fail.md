# Compile-time contracts

These examples are expected to be rejected by the compiler. They protect the
ownership and capability boundaries without depending on rustc's diagnostic
wording.

Pinned values require the pin-aware branded borrow:

```compile_fail,E0277
use std::marker::PhantomPinned;
use o3::cell::branded::{Brand, BrandToken};

struct Pinned(PhantomPinned);

BrandToken::scope(|mut token| {
    let cell = Brand::new(Pinned(PhantomPinned));
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

Slab keys cannot be fabricated from public parts:

```compile_fail,E0624
use o3::collections::slab::key::{Key, Parts};

let parts = Parts::new(0, 1).unwrap();
let _ = Key::<()>::from_parts(parts);
```

Keys from distinct tag domains cannot be mixed:

```compile_fail,E0308
use o3::collections::slab::{Slab, key::Key};

struct Read;
struct Write;

fn remove(slab: &mut Slab<u8, Write>, key: Key<Read>) {
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
use o3::cell::branded::{BrandToken, Region};

BrandToken::scope(|mut token| {
    let cell = Region::new(0_u8);
    *cell.borrow_mut(&mut token) = 1;
});
```

Cells from one generative brand cannot be accessed through another:

```compile_fail,E0521
use o3::cell::branded::{Brand, BrandToken};

BrandToken::scope(|mut first| {
    BrandToken::scope(|mut second| {
        let cell = Brand::new(0_u8);
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
use o3::collections::slab::{key::Key, pin};

struct Read;
struct Write;

fn remove(pool: &mut pin::Pool<u8, Write>, key: Key<Read>) {
    pool.remove(key);
}
```
