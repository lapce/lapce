# Rope science, part 12 - minimal invalidation
24 Nov 2017

This post describes some of the motivations, theory, and implementation behind “minimal invalidation” (also tracked in issue `#317`).

A major part of the philosophy of performance in xi is that as much of the processing as possible is incremental. Basically, this means that a change to the document is represented as an explicit delta, then this delta propagates through the rendering pipeline. Ideally, the code touches only a tiny part of the document.

Here we will talk mostly about the implementation in the core, but getting small deltas to the front-end is also important. In an incremental style the front-end can re-render only what’s in the delta, and then ideally use graphics hardware to re-composite the document view, resulting in much improved latency and power consumption, among other things.

Note that an incremental style is not the only way to write a performant editor. If rendering is fast, then it’s much simpler to just re-render the entire document window on every update. That is very much the style of video games, for example, where they have to re-draw the world on every frame in any case.

## The render function
Here, we’ll consider rendering the document as a purely functional program. The core’s responsibility is to produce a sequence of rendered lines, which are basically strings with attributes for syntax highlighting and selection carets. Obviously, further stages in the pipeline (all in the front-end) convert this representation into pixels displayed on a screen.

The input to the render function consists of the text (conceptually just a string), style spans, the selection, and the line breaks (when word wrapping is in effect). There’s also some other stuff, like the results of a “find” command, but let’s keep it simple for now. In fact, to keep things really simple, let’s just focus on the text, as the concepts are similar, it’s just more merging of more inputs.

The line breaks structure is conceptually just a sequence of offsets corresponding to the end of each line. (See the rope science posts on [Metrics](rope_science_02.md) and [Word Wrapping](rope_science_06.md) for more on how these breaks are determined and represented).

Thus, as an imperative program, the simplified render function is almost trivial:

```
def render(text, breaks):
    rendered = []
    last = 0
    for break in breaks:
        rendered.append(text[last:break])
        last = break
    return rendered
```

Obviously, adding styles and selections makes it more complicated, but the basic structure is the same. However, if the document is very large, then recomputing this on every keystroke is wasteful, much less serializing it and sending it over an RPC channel.

Can we systematically transform this into an incremental algorithm? Why yes, we can.

The first thing to notice is that every line is independent of every other line. In addition, to actually draw pixels, we don’t need all the lines, just the ones that appear inside the viewport. When changes happen outside this viewport, ideally we’d like to avoid sending an update at all (one way this can happen in practice is when making a [syntax highlighting](rope_science_11.md) change that ripples to the end of the document). Thus, we want to go beyond making it incremental and also make it lazy in a way, only spending the work to compute the slice or view that’s actually needed. The front-end then holds not the entire result of the render function, but a cache of it, with each line either valid (and thus guaranteed to match the result of the render function), or invalid. When the front-end needs a line not in the cache (for example, when scrolling), it requests it from the core.

The version of the render function designed to compute a single rendered line at a time is in a way even simpler:

```
def render_line(text, breaks, line_num):
    return text[breaks[line_num - 1] : breaks[line_num]]
```
(Assume here that breaks is a fancy object that’s designed to return 0 when indexed with -1.)

## The update protocol
Given that the output of our incremental render algorithm is a delta, we need a way to represent it explicitly. We then serialize the delta and send it from the core to the front-end as an asyncronous (but in-order) notification.

The delta can be interpreted as a function from the previous value of the render function (a sequence of lines) to the next value. It’s also worth being able to introspect into this function, for example to know what’s changed so only some layers need to be re-rendered in a compositing UI pipeline.

A slight complication is that lines in the front-end’s cache might be invalid. In our architecture, the core is in charge of which lines are valid (see `#280` for discussion of this decision).

The representation of deltas of this kind is reasonably well understood. The output of Unix [diff](https://en.wikipedia.org/wiki/Diff_utility), for example, is a sequence of insert and delete operations interspersed with unmodified runs. The details of representation are not terribly important. In xi, we ended up with a sequence of `invalidate`, `skip`, `copy`, `ins`, and `update` operations (this last is for updating only styles and cursors when the text is otherwise unchanged). The ins operation is the same as diff, while deletion is represented as two copy operations with a skip in the middle. See [Xi view update protocol](https://xi-editor.io/docs/frontend-protocol.html#xi-view-update-protocol) for detailed documentation on the update method.

## The render plan
In the current architecture, the core is in charge of the cache state, especially which lines are valid and which lines are invalid. The core tracks the scrolled viewport (through the `scroll` notification) but might lag behind. All lines inside the viewport must be valid, in order to draw correctly. For lines outside the viewport, either choice is reasonable. Keeping a line valid can be helpful on scrolling, as it then doesn’t need to be requested from the core, but it comes at a storage cost, so should be bounded (especially for large documents). In addition, when updating, actually visible lines must be the priority, as re-validating additional lines takes extra time to compute, serialize, and process.

Thus, at every opportunity to update, xi produces a render plan. For every line, one of three things can happen: it can be discarded even if it was valid, it can be preserved if valid, or it can be rendered if invalid. The render plan chooses rendering for the visible viewport (plus a very small “slop” for scrolling), preserving for a range extending 1000 lines from the viewport, and discards the rest. The theory is that preserving existing valid lines comes at a very small cost; no additional computation or communication is needed, just the storage in the cache.

Changing the viewport (for example, when scrolling) updates the render plan. In addition, the front-end can explicitly request additional lines, and those are added to the render plan as well. For any given render plan, it’s possible that an update would be a no-op, in which case it’s not sent at all.

The render plan is stored in the `RenderPlan` struct, in `line_cache_shadow.rs`.

## Computing minimal deltas
With these requirements, we can start looking at how to actually produce the deltas. For any given editing operation, it would be possible to directly work out the corresponding delta to the render, but that is potentially a large number of cases, and also doesn’t smoothly handle aggregating a sequence of changes into a single render. There is a more systematic way.

We propose a data structure here called the line cache shadow. It is essentially the skeleton of the render result, but stored in extremely lightweight form, and easy to update. Then, to actually produce the delta, we traverse the render plan and the line cache shadow. Along with producing the delta is a new line cache shadow, which, before any additional edits, just tracks which lines are valid.

Conceptually, each line in the line cache shadow is either a reference to a line in the front-end’s line cache, or an indication it is invalid. Updating the shadow is straightforward: to insert a line, insert “invalid”, and to delete a line, delete it in the shadow. Note that in the delete case, a gap occurs in the references to the existing valid lines; when synthesizing the delta, this is the cue to issue a `skip` command.

Similarly, to rewrap a paragraph (changing line breaks in it), just replace the range of lines of the paragraph with a new range, all invalid. And, using the [incremental re-wrap technique](rope_science_06.md), possibly the entire paragraph need not be invalidated.

Synthesizing the delta from the shadow and render plan is then straightforward. When the plan calls for discarding, issue skip. When it calls for preserving, issue `invalid` for invalid lines in the shadow, or `copy` for valid lines. And when it calls for rendering, re-render and issue `ins` for invalid lines, and `copy` for valid lines. And for each `copy`, add a `skip` if the line number is not sequential.

As a further refinement in practice, a line may be partially valid. A common and important case is that the text and styles are valid, but the cursor has changed. We use a bitset to keep track of partial validity, and then when rendering partially valid lines send an `update` rather than an `ins`. For cursor movement, a sophisticated front-end might then just update the cursor layer without needing to re-render any of the text.

The line cache shadow data structure itself (`LineCacheShadow`) is extremely small and lightweight to compute, as it’s stored in run-length form. In the absolute worst case, upon edit the cache can just be replaced with a single span indicating all lines are invalid.

## Conclusion
The mechanisms described here are fairly elaborate, but they all flow from a clear specification of the problem as a functional program, and from that we can systematically derive the incremental algorithm. Further, the correctness criterion is clear (applying the resulting delta should yield the same result as recomputing from scratch), and we hope to use property testing to ensure that.

Producing minimal deltas is key to xi delivering on its promised performance goals, and should result in best-in-class latency and editing smoothness, all at minimal power costs.
