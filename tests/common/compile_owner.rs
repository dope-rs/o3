use o3::base::cell::region;

fn main() {
    region::Token::scope(|token| {
        let value = token.bind(0);
        region::Token::scope(|other| {
            let _ = other.borrow(&value);
        });
    });
}
