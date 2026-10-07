use o3::depot::map::{
    Entry, Hasher,
    resident::{Id, Table},
};

fn cannot_reuse<H: Hasher<Id>>(table: &mut Table<u8, H, 1>, id: Id) {
    table.scope(|mut scope| {
        let Entry::Vacant(Some(entry)) = scope.entry(id) else {
            return;
        };
        let (lease, fresh) = entry.insert(7).publish();
        let Some(fresh) = fresh else {
            return;
        };
        let mut retired = scope.retire();
        let _ = scope.get_mut(&lease);
        retired.occupied(fresh).remove();
        retired.occupied(fresh).remove();
    });
}

fn main() {}
