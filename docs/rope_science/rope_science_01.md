# Rope science, part 1 - MapReduce for text
(or, MapReduce for text)

When people think of MapReduce, they usually think of it as a tool for exploiting parallelism in “big data” applications, where you’re grinding through terabytes of data to do some analysis. The idea is powerful and has applications in other spaces. Here I’ll show how it can solve a tricky problem in text editing.

The horizontal scrollbar at the bottom of the window needs to know the width of the content inside. For text editing with word wrapping off, that’s the width of the longest line. Of course, “width” can be quite tricky in its own right, but let’s assume ASCII and a monospace font to keep things simple. Computing this sequentially is trivial, here’s some Python:

```reduce(max, map(len, text.split('\n')))```

This isn’t very Pythonic, you’d never explicitly write the “reduce,” but bear with me.

Now let’s say we wanted to parallelize this computation. Again, maybe a little silly for this application, but hey, we’ve got a lot of cores, might as well use them. First, instead of storing the string in a single contiguous array, store it in a tree data structure, with actual slices at each leaf (this is known as a rope data structure). Now, map, reduce, and profit!

The problem is that the line boundaries (the output of the split operation above) don’t necessarily line up with the text. Maybe you’ve got a very long line that doesn’t fit into a single leaf in the rope. But never fear, we can still fit this computation into the MapReduce framework, with a little cleverness.

Instead of bubbling up a simple scalar (the length of the line), use a slightly more complex analysis. Each substring either has no line boundaries, in which case you record the length of the substring, or it has at least one, in which you record the length up to the first boundary, the max length of all complete lines, and the length from the last boundary to the end. There are two cases, so doing reduce on two children has four cases, none of which are very hard.

Ok, we’ve made computing the maximum length a little faster, woohoo. But the real juice comes when we want to make this an incremental computation. 99% of the time when you’re editing, the max line width doesn’t change, and it’s pretty easy to detect those cases (the line you’re editing is smaller than the max, or it newly becomes the longest). But writing out all the cases (breaking and merging lines, copy-paste, etc) gets tricky, and you’ll probably end up sometimes having to scan the entire buffer to find the second-longest line, which might cause a hiccup in UI responsiveness.

MapReduce can solve this problem. You simply maintain the summary information at each node in your tree, and when you edit the text, you recompute the summary only for the nodes that changed, then bubble those up to the root. That’s O(log n), so we’re talking microseconds worst case per user action. And it’s very principled, all the tricky general cases fall out of the simple 4-way case analysis at the reduce step, plus the generic framework implemented in the rope.

As a side benefit, with this framework in place, it might be possible to exploit multiple cores to make file loading faster, though I haven’t implemented that yet. In Rust, Rayon looks like a very promising approach to adding parallelism, and at some point I hope to integrate it with my rope data structure.

## In comments
Paul Hankin pointed out this was a monoid, starting my fondness for monoid homomorphisms.
