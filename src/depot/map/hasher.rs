/// A statically dispatched 64-bit hash policy for an owned-key map.
///
/// For a given policy, equal keys must hash equally. A stored key's hash and
/// equality must remain stable until removal, including through interior mutability.
/// Violations are logic errors that can make entries unreachable or duplicate keys.
pub trait Hasher<K> {
    fn hash(&self, key: &K) -> u64;
}
