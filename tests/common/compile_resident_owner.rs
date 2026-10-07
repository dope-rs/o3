use o3::depot::map::{
    Entry, Hasher,
    resident::{Id, Table},
};

fn cannot_mix<H: Hasher<Id>>(first: &mut Table<u8, H, 1>, second: &mut Table<u8, H, 1>, id: Id) {
    first.scope(|mut first| {
        let Entry::Vacant(Some(entry)) = first.entry(id) else {
            return;
        };
        let (lease, _) = entry.insert(7).publish();
        second.scope(|mut second| {
            let _ = second.get_mut(&lease);
        });
    });
}

fn main() {}
