use o3::depot::queue::round::{self, phase};

fn reuse(head: round::Location<&round::Lanes<(), 1>, phase::Head>) {
    head.pop();
    head.pop();
}

fn main() {}
