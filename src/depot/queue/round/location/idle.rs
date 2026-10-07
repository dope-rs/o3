/// Exclusive detached membership; ASSIGN records whether the destination must be written.
#[derive(Debug)]
pub struct Idle<const ASSIGN: bool>;
