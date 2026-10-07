use crate::depot::queue::indexed::lanes::typed;

pub trait Sealed {}

impl Sealed for typed::Control {}
impl Sealed for typed::Data {}
impl Sealed for typed::Idle {}
