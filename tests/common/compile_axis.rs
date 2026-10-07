use o3::mem::credit::fair;

fn main() {
    let _: fair::Cause<usize, 1> = fair::Cause::Overflow {
        axis: 1usize,
        held: 0,
        requested: 0,
    };
}
