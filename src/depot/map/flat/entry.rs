use crate::depot::{map, map::flat};

pub enum Entry<'a, V> {
    Occupied(map::Found<&'a mut flat::Table<V>>),
    Vacant(map::Vacancy<&'a mut flat::Table<V>, u64>),
}
