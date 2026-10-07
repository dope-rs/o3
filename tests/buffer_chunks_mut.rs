use std::num::NonZero;

use o3::buffer::bytes::ChunksMut;

#[test]
fn retains_disjoint_chunks() {
    for len in 0..=9 {
        for width in (1..=11).chain([usize::MAX]) {
            let mut bytes = [0; 9];
            let mut chunks = ChunksMut::new(&mut bytes[..len], NonZero::new(width).expect("width"));
            let count = len.div_ceil(width);
            let mut retained = Vec::new();
            while let Some(chunk) = chunks.next() {
                retained.push(chunk);
                assert_eq!(chunks.len(), count - retained.len());
            }
            assert_eq!(retained.len(), count);
            assert!(chunks.next().is_none());
            for (index, chunk) in retained.into_iter().enumerate() {
                chunk.fill(index as u8 + 1);
            }
            for (index, &byte) in bytes[..len].iter().enumerate() {
                assert_eq!(byte, (index / width) as u8 + 1);
            }
        }
    }
}
