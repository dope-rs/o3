use o3::depot::queue::round::{self, phase};

fn clone(head: round::Location<&round::Lanes<(), 1>, phase::Head>) {
    let _ = head.clone();
}

fn main() {}
