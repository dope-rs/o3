use o3::{
    depot::{
        capacity::Limit,
        queue::indexed::planes::{self, bound, groups},
    },
    mem::slot::Storage,
};

type Set = planes::Set<Storage<u8, usize>, 3, groups::Index<u64, Limit<4>, u64, 2, 4>>;

fn keeps_hooks(owner: &mut Set) {
    bound::Scope::with(owner, |root| {
        let (mut entries, _) = root.split();
        let Some((key, front)) = entries.front_entry::<0>() else {
            return;
        };
        drop(front.pop());
        let (mut resident, key) = entries.install::<0>(key, 7);
        let value = resident.get_mut(key);
        resident.entry(key).link_back::<1>();
        *value = 9;
    });
}

fn keeps_terminal(owner: &mut Set) {
    bound::Scope::with(owner, |root| {
        let (mut entries, side) = root.split();
        let Some((key, front)) = entries.front_entry::<0>() else {
            return;
        };
        drop(front.pop());
        let (resident, key) = entries.install::<0>(key, 7);
        let (timer, mut terminal) = resident.into_select(key, side);
        drop(timer);
        let value = terminal.get_mut();
        drop(terminal.take());
        *value = 9;
    });
}

fn main() {}
