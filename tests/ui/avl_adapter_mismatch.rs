use std::pin::Pin;
use std::ptr::NonNull;

use o3::collections::intrusive::{AvlAdapter, AvlNode, AvlTree};

#[repr(C)]
struct Left {
    node: AvlNode,
}

#[repr(C)]
struct Right {
    node: AvlNode,
}

struct LeftAdapter;

unsafe impl AvlAdapter for LeftAdapter {
    type Value = Left;

    fn node(value: Pin<&Left>) -> Pin<&AvlNode> {
        unsafe { value.map_unchecked(|value| &value.node) }
    }

    unsafe fn from_node(node: NonNull<AvlNode>) -> NonNull<Left> {
        node.cast()
    }
}

fn main() {
    let tree = AvlTree::<LeftAdapter>::new();
    let right = Box::pin(Right {
        node: AvlNode::new(),
    });

    unsafe { tree.insert(right.as_ref(), |_, _| false) };
}
