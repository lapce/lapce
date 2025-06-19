use lapce_xi_rope::{Rope, rope::ChunkIter};
use tree_sitter::TextProvider;

pub struct RopeChunksIterBytes<'a> {
    chunks: ChunkIter<'a>,
}
impl<'a> Iterator for RopeChunksIterBytes<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        self.chunks.next().map(str::as_bytes)
    }
}

/// This allows tree-sitter to iterate over our Rope without us having to convert it into
/// a contiguous byte-list.
pub struct RopeProvider<'a>(pub &'a Rope);
impl<'a> TextProvider<&'a [u8]> for RopeProvider<'a> {
    type I = RopeChunksIterBytes<'a>;

    fn text(&mut self, node: tree_sitter::Node) -> Self::I {
        let start = node.start_byte();
        let end = node.end_byte().min(self.0.len());
        let chunks = self.0.iter_chunks(start..end);
        RopeChunksIterBytes { chunks }
    }
}
