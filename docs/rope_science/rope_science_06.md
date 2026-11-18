# Rope science, part 6 - parallel and asynchronous word wrapping
(originally written 24 Apr 2016)

The xi approach to incremental word wrapping really only covers two of the use cases we care about: scrolling, and doing small edits to a large file. The other two main use cases are loading a file, and changing the line width. In both of those cases, the current implementation will exhibit noticeable pauses. Here I’ll talk about what I plan to do to address those.

Computing word wrap when loading a file basically requires doing the operation in bulk. An incremental approach is not applicable because there is no “previous value” to compute a delta against. You basically have to do the work of computing the break locations, line widths, etc. My implementation runs at about 150MB/s, but for large files (over a couple MB) that’s still a pause.

Fortunately, there’s more we can do, notably exploit multi-core parallelism. Most computers that would run xi have at least 4 cores, so it’s worth going after if we can get a near-linear speedup. I think we can, at least in a lot of cases.

## Parallel word wrapping
There are a few ways to slice the problem. By far the easiest is to divide the input by hard breaks (newlines, essentially). Between any two hard breaks, the result of word wrapping is independent. Obviously, there’s one case where this won’t help, which is if you have a large file with no newlines (this can happen when serializing to XML or JSON without any formatting). Oh well.

Doing this in xi should be relatively easy, especially with the help of a framework like [Rayon](https://github.com/nikomatsakis/rayon), which I haven’t used yet but looks like it will be good for this kind of work. In fact, getting the computation started is fairly straightforward assuming the input file has already been put into a rope with the number of newlines summarized at each node - you can just enumerate the n lines in parallel, and for each, find the start and end of the line (an O(log n) operation), run the word wrapper, and return the result to an accumulator which just concatenates the wrapping results together.

I think this would fit pretty well into Rayon’s parallel iterator framework, but haven’t tried it yet, so am not 100% sure. All shards of the computation can share a reference to the source text (it doesn’t even need to bump the reference counter, as the type system can guarantee that the input’s lifetime exceeds any of the shards, as long as the reference is held til the end), and, similarly, the wrap results can just be concatenated together in functional style, rather than mutably appending to a result buffer.

This sounds like it would be fun to experiment with, and not too complicated, as the complexity shouldn’t leak beyond the word wrap computation itself.

## Asynchronous word wrapping
It’s always nice to make things go faster, and bringing the load time of a 300M file to under a second would be appealing (it would be in the ballpark of vim, though I haven’t measured it carefully), but it’s still not as good as it might be, and also not very satisfying when changing the line width (for example, by dynamically resizing the window). There’s also that case of no hard breaks in the text.

Making the word wrapping asynchronous can help with all this, but is definitely trickier to implement. It’s also not just a computer science problem of finding the result of the computation more quickly, it has implications for user interaction.

When loading a file, the basic idea is to aggressively append chunks of the file to the buffer. This has the potential to interfere with editing operations, but basically editing the interior of the prefix should be relatively clean with respect to appending more at the end. Another UX consequence is that the scrollbar (if visible) will show the buffer expanding as the file is loaded. I find this actually to be a pleasant affordance, reminiscent of how browsers used to display scrolls of HTML as they loaded from the network.

One subtlety is how to handle a “save” action while the file is still loading. The Right Way would be to store a snapshot of the current buffer, while still allowing edits (the snapshot is easy with immutable ropes), then append the rest of the file to both the snapshot and the buffer under edit, deferring the save until the load is complete. The file contents wouldn’t even be duplicated in RAM, as the rope structure is reference-counted trees, so the references would be shared.

A considerably tricker interaction would be dynamic changes to the line width. Here, you want to minimize the amount of vertical bouncing as the segment above the cursor is rewrapped and the number of lines changes. A good heuristic would be to preserve the scrolled position of the cursor relative to the top of the window. Another tricky aspect is ordering the sequence async rewraps - you’d want to do what’s on the screen first, for immediate update, then the segment from the start of the screen back to the nearest hard break before it, then maybe work backward to the beginning of the buffer, then from the bottom of the visible region to the end of the buffer. In the meantime, if the change to the width is large, the scrollbar might do “interesting” things - perhaps an even more high tech approach would be to switch back and forth between the “before visible” and “after visible” segments, to keep the effect on the scroll bar minimal.

Even with all this trickiness, it’s probably worth doing, as the result would be immediate update of the display to a “pretty good” state after almost any change. It’s also likely that I’ll have to plan for asynchronous updates in any case, to tolerate slowness in plugins such as syntax highlighting. I’ll talk about that more later, as the general case certainly has some interesting aspects compared to just async wrapping.

I’m not planning on implementing any of this right now, as I don’t feel it’s blocking my goal of delivering a high quality editing experience, and I think the (relatively very simple) code I’m writing now can be adapted when the time comes.

Next up, probably: spans and interval trees.
