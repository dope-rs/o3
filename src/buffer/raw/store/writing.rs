/// Remaining unpublished bytes of one exclusive staged writer.
pub(in crate::buffer) trait Space {
    fn remaining(&self) -> usize;
}
