use o3::{
    depot::{
        capacity::Limit,
        queue::indexed::planes::{self, bound, groups},
    },
    mem::slot::Storage,
};

type Set = planes::Set<Storage<u8, usize>, 3, groups::Index<u64, Limit<4>, u64, 2, 4>>;

fn cannot_mix(first: &mut Set, second: &mut Set) {
    bound::Scope::with(first, |root| {
        let (mut entries, _) = root.split();
        let Some((key, front)) = entries.front_entry::<0>() else {
            return;
        };
        drop(front.pop());
        let (first, key) = entries.install::<0>(key, 7);
        bound::Scope::with(second, |root| {
            let (entries, _) = root.split();
            let second = entries.resident::<0>();
            let _ = second.get(key);
        });
        drop(first);
    });
}

fn main() {}
