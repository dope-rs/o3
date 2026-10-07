use o3::depot::{arena::slab::key, queue::indexed::lanes::typed};

struct Local(u8);

impl typed::Phase<Local, u16, u32> for typed::Vacant<'static, key::Handle, Local, u16, u32, 2> {
    type Item = Local;

    fn value(value: Local) -> typed::Value<Local, u16, u32> {
        typed::Value::Data(u16::from(value.0))
    }
}

fn main() {}
