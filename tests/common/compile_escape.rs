use o3::base::cell::region;

fn main() {
    let mut escaped = None;
    region::Token::scope(|token| {
        escaped = Some(token.bind(0));
    });
    drop(escaped);
}
