use crate::depot::queue::indexed::lanes::typed;

pub trait Running {}

impl Running for typed::Control {}
impl Running for typed::Data {}
