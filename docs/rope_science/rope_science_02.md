# Rope science, part 2 - metrics
(originally written 15 Apr 2016)

I’m exploring how a MapReduce-like framework can help solve difficult problems in text editing. I’m finding this exploration useful for two reasons. First, it underpins a clean, highly generic implementation in the xi editor. Second, it’s helping me understand the often messy code in a real implementation such as the Android text stack.

MapReduce is based on the concept of a monoid homomorphism. That sounds like I’m about to get all category-theoretical, but it’s pretty simple. A monoid is just an associative binary operator with an identity element; string concatenation and integer addition are two of the main monoids we’ll be looking at. The homomorphism is a mapping from one monoid to the other that preserves the identity and associativity properties. A very simple example is taking the length of a string; `len(s + t) = len(s) + len(t)`, and the mapping from the empty string to 0 has the identity properties you’d expect.

In this post, I’m setting out to explore the space of useful monoid homomorphisms where the monoid is just unsigned integer sum. I’m going to use the word “metric” to describe these, as I think they represent (simple, 1-d) metric spaces over positions in strings, and also intuitively capture the concept of measurement. A real math person might correct me or have a better idea.

## Code points and code units
A Unicode string is conceptually a sequence of code points. This is then the simplest metric - a single code point has a count of 1. However, most applications use a variable length encoding of code points, where one code point is one or more code units. By far the two most popular encodings are UTF-8, where a code point is represented as 1 to 4 bytes (the UTF-8 code unit is a byte), and UTF-16, where a code point is 1 or 2 code units, each a 16 bit integer. If you want to randomly access a position in a string, you better know its offset from the beginning of the string in whatever code units are used in the string’s representation.

In xi, it’s important to convert between the two, because most UI toolkits use UTF-16, while the file is stored in UTF-8. By storing both metrics in each node in the b-tree, it’s possible to convert between the two in O(log n) time. I could store a code point count as well, but probably won’t, because as it turns out it’s fairly rare to care about the number of code points in a string; you either want code units so you can efficiently access storage, or you want a higher level concept such as grapheme boundaries, so you know where to move the cursor on arrow key press.

## Iterators
While it’s rare to count the number of code points in a string, it’s extremely common to iterate through its code points. This, then, is the next function of metrics – they serve as the basis of iterators through the string. Using a “cursor” into the b-tree, you can find the next or previous code point in O(1) amortized time, O(log n) worst case.

At a more conceptual level, iterators find the next boundary, and a boundary represents a discontinuity in the metric. A subtle point is that the discontinuity doesn’t have to be a step of exactly 1. Thus, in xi, the “base metric” has boundaries at code points, but is in units of UTF-8 code units (bytes), so the iterator derived from it steps through code points, but it can still be used for efficient random access.

A different design choice would have been to use bytes as the base metric (with steps of 1 bytes) and explicitly convert to code points. It would require dealing with fragments of code points, which is a huge pain, but in an application with a lot of mixed text and binary data, it would probably make sense.

## Line endings
The next useful metric is line endings. Again, it’s extremely simple, the U+000A code point (or 0x0A byte in a UTF-8 representation) gets a count of 1, everything else is 0. The monoid then gives you a count of the number of lines. Further, you can access a random line number within the string using the same logic (and same O(log n) worst case running time) as above. In xi, it’s even the same generic implementation (currently Node::convert_metrics<M1, M2>).

The iterator derived from the line ending metric is also extremely useful, and is also O(log n) worst case. (It’s O(1) amortized if you also know that line lengths are bounded, but either way it’s an extremely good time complexity; many editors degrade horribly in the face of unbounded line lengths).

## Difficulty
We’re almost out of things we can count in such a simple monoid, but there is one more simple and useful property we can extract from strings – difficulty level. Printable ASCII is by far the easiest text to deal with, then you layer on non-ASCII, non-BMP, bidi, various forms of complex text processing (conceptually I think the best way to represent this is whether mapping between code points and grapheme clusters is 1-1), emoji (which might trigger a color rendering path), etc. Even within ASCII, it’s useful to detect tabs (U+0009) for special processing.

While we could store the count of code points at each difficulty level, we probably only care whether it’s zero or not (so difficulty level is represented as a set of bitflags). Even so, it certainly fits in the same monoid framework, and everything else works. For example, an iterator for the non-ASCII difficulty level would let you process a run of ASCII characters. Same for bidi.

Difficulty is also useful for making the computation of the other metrics more efficient. In ASCII, code point, UTF-8 code unit, and UTF-16 code unit are all the same (not to mention grapheme boundary), so computation involving these other metrics becomes trivial. This is a pretty common optimization in string and text systems. For example, Swift has two different in-memory representations for the ASCII and non-ASCII (UTF-16) cases, and almost all low-level string code starts with a case switch. In xi, I really like the fact that there’s a single representation so you don’t have to do a case analysis, but the difficulty level is available for optimizations that benefit from it.

(I haven’t implemented this yet, but it will certainly be the important for the use case of being able to slice through gigabyte log files like butter)

## Line breaking
The above metrics are all useful, but the framework of such a simple monoid (integer addition) is also extremely limiting. What about soft line breaks? Grapheme boundaries? An editor certainly needs these, but they need state to compute, so they don’t really work as a monoid homomorphism. (I’m hoping some smart-ass will come up with a monoid for counting grapheme boundaries, but that’s not the way I want to do it in xi. See me after class).

There is a general solution, though, which is to compute these boundaries outside the strict monoid framework, then store them in a rope-like data structure, alongside the rope storing the text itself. In the b-tree framework, the leaf is a length (in the “base units” of the text) and a set of breaks within that length. Equivalently, you can consider it to be a sequence of lengths, possibly including fragments at the beginning and end. All the usual monoid operations work just fine (conceptually, it can be considered a string of just two characters, “break” and “other,” but you’d almost definitely want to store it more efficiently than that).

What makes this all work is that the rope storing the breaks aligns with the rope storing the string. The metrics are compatible. And all of the metrics operations are extremely useful – you can iterate to the next line break, or know how many (formatted) lines deep you are into the text.

Because it’s stateful, there’s a little more work you have to do when editing, as you have to keep the two ropes in sync. But this is pretty straightforward, editing operations are basically insert and delete, so applying those to the secondary rope is not too hard.

One thing I find especially exciting about this representation is that it makes highly incremental computation of line breaks relatively easy. When you edit the text, you update a “dirty” range to include the lines just edited. Then, the update process looks at the first line in the dirty range, looks forward to find the appropriate line break position, and checks to see whether that matches the next line break in storage. If not, it updates storage and adds it to the dirty range. Either way, it bumps the start of the dirty range by one line. Even better, the processing of the dirty range can happen asynchronously, so as you type, it reformats what’s on the screen and repaints almost instantly, then takes its sweet time to reformat the rest of the paragraph in the background, even if it’s huge.

Suffice it to say, I really wish Android’s EditText had anywhere near this kind of performance characteristic.

I’m going to use the same concept of alignment on base metrics to store the rich-text spans, but that’s definitely a different topic.

## Real numbered metrics
I said I was going to limit myself to unsigned integers, but I’ll extend slightly to real numbers, namely physical dimensions of formatted text. I can get away with this by noting that digital implementations of physical units can be reduced to integers.

Of particular interest in an editor is height, as this is intimately tied with scrolling, deciding what to draw and where to draw it, hit testing a mouse click, etc. (Width is important too but has its own set of complexities and doesn’t have the same scaling pressures). In a code editor, it’s a reasonable simplification to declare that all lines are the same height, so a metric counting formatted lines is fine. But in general editing applications, line height can vary, and even in a code editor, wouldn’t it be great if Markdown headers popped to a bigger font?

The concept of metric is certainly general enough to include physical dimensions (that’s actually where it came from). The implementation is basically the same as the breaks rope above, except that you store a height with each line.

It’ll probably be a while before I implement this, but it’s nice to know the conceptual framework supports it.

## Conclusion
Sophisticated monoids are appealing, but there’s actually a lot you can do with just the simple integer addition monoid. As I explore this space, I’m finding that many of the properties that fit into this framework are reflected (often in very messy ways) in code to handle real problems in text stacks I’ve studied and worked on.

In xi, I’m trying to represent these tricky problems in clean, general abstractions, using conceptual frameworks such as monoid homomorphisms. They help me reduce the special-casing and maximize the amount of work that can be done by any given line of code. So far it seems to be working pretty well.

```[This post written entirely inside xi]```

## In comments
Some discussion of alternatives to “base unit” but no consensus on a better one.

Thomas Colthurst pointed me to Guy Steele’s talk on catamorphisms, which is quite interesting.
