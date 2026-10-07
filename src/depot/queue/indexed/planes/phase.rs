/// Admitted current front of the selected plane.
#[derive(Debug)]
pub enum Front<const P: usize> {}
/// Admitted linked hook in the selected plane.
#[derive(Debug)]
pub enum Linked<const P: usize> {}
/// Admitted detached hook in the selected plane.
#[derive(Debug)]
pub enum Vacant<const P: usize> {}
