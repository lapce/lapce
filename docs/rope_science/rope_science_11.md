# Rope science, part 11 - practical syntax highlighting
23 Apr 2017

In this post, we present an incremental algorithm for syntax highlighting. It has very good performance, measured primarily by latency but also memory usage and power consumption. It does not require a large amount of code, but the analysis is subtle and sophisticated. Your favorite code editor would almost certainly benefit from adopting it.

Pedagogically, this post also gives a case study in systematically transforming a simple functional program into an incremental algorithm, meaning an algorithm that takes a delta on input and produces a delta on output, so that applying that delta gives the same result as running the entire function from scratch, beginning to end. Such algorithms are the backbone of xi editor, the basis of near-instant response even for very large files.

## The syntax highlighting function
Most syntax highlighting schemes (including the TextMate/Sublime/Atom format) follow this function signature for the basic syntax highlighting operation (code is in pseudo-rust):

```fn syntax (previous_state, line) -> (next_state, spans);```

Typically this “state” is a stack of finite states, i.e. this is a pushdown automaton. Such automata can express a large family of grammars. In fact, the incredibly general class of LR parsers could be accomodated by adding one additional token of lookahead in addition to the line, a fairly straightforward extension to this algorithm.

I won’t go into more detail about the syntax function itself; within this framework, the algorithms described in this post are entirely generic.

## A batch algorithm
The simplest algorithm to apply syntax highlighting to a file is to run the function on each line from beginning to end:

```
let mut state = State::initial();
for line in input_file.lines() {
    let (new_state, spans) = syntax(state, line);
    output.render(line, spans);
    state = new_state;
}
```

This algorithm has some appealing properties. In addition to being quite simple, it also has minimal memory requirements: one line of text, plus whatever state is required by the syntax function. It’s useful for highlighting a file on initial load, and also for applications such as statically generating documentation files.

For this post, it’s also something of a correctness spec; all the fancy stuff we do has to give the same answer in the end.

# Random access; caching
Let’s say we’re not processing the file in batch mode, but will be displaying it in a window with ability to scroll to a random point, and want to be able to compute the highlighting on the fly. In particular, let’s say we don’t want to store all the spans for the whole file. Even in a compact representation, such spans are comparable to the size of the input text, potentially much more.

We can write the following functional program:

```
fn get_state(file, line_number) -> state {
    file.iter_lines(0, line_number).fold(
        State::initial(),
        |state, line| syntax(state, line).state
    )
}

fn get_spans(file, line_number) -> spans {
    let state = get_state(file, line_number);
    syntax(state, file.get_line(line_number)).spans
}
```

This will work very well for lines near the beginning of the file, but has a serious performance problem; it is O(n) to retrieve one line’s worth of spans, so O(n^2) to process the file.

Fortunately, [memoization](https://en.wikipedia.org/wiki/Memoization), a traditional technique for optimizing functional programs, can come to the rescue. Storing the intermediate results of `get_state` reduces the runtime back to O(n). We also see the algorithm start to become incremental, in that it’s possible to render the first screen of the file quickly, without having to process the whole thing.

However, these benefits come at a cost, namely the memory required to store the intermediate results. In this case, we only need store the state per line (which, in a compact representation, need only be one machine word), so it might be acceptable. But to handle extremely large files, we might want to do better.

One good compromise would be to use a cache with only partial coverage of the `get_state` function; when the cache overflows, we evict some entry in the cache to make room. Then, to compute `get_state` for an arbitrary line, we find closest previous cache entry, and run the fold forward from there.

This cache is a classic speed/space tradeoff. The amount of time to compute a query is essentially proportional to the gap length between one entry and the next. For random access patterns, it follows that the optimal pattern would be evenly spaced entries. Then the time required for a query is O(n/m), where m is the cache size.

Tuning such a cache, in particular choosing a cache replacement strategy, is tricky. We’ll defer discussion of that for later.

# Handling mutation
Of course, we really want to be able to do interactive syntax highlighting on a file being edited. Fortunately, the above cache can be extended to handle this use case as well.

As the file is mutated, existing cache entries might become invalid. We define a cache entry (line_number, state) as being valid if that state is actually equal to computing `get_state(line_number)` from scratch. Editing a line need not only change the spans for that line; it might cause state changes that ripple down from there. A classic example would be inserting `/*` to open a comment; then the entire rest of the file would be rendered as a comment. So, unlike a typical cache, changing one line might invalidate an arbitrary fraction of the cache contents.

We augment the cache with a frontier, a set of cache entries. All operations maintain the following invariant:

If a cache entry is valid and it is not in the frontier, then the next entry in the cache is also valid.

From this invariant immediately follows a number of useful properties. All lines up to the first element of the frontier are valid. Thus, if the frontier is empty, the entire cache is valid.

This invariant is carefully designed so that it can be easily restored after an editing operation, specifically that all operations take minimal time (I think it’s O(1) amortized, but establishing that would take careful analysis).

Specifically, after changing the contents of a single line, it suffices to add the closest previous cache entry to the frontier. Other editing operations are similarly easy; to replace an arbitrary region of text, also delete cache entries for which the starts of the lines are in strictly in the interior of the region. For inserts and deletes, the line numbers after the edit will also need to be fixed up.

Of course, it’s not enough to properly invalidate the cache, it’s also important to make progress towards re-validating it. Here is the algorithm to do one granule of work:

 - Take the first element of the frontier. It refers to a cache entry: `(line_number, state)`.
 - Evaluate `syntax(state, file.get_line(line_number))`, resulting in a new_state.
 - If `line_number + 1` does not have an entry in the cache, or if it does and the entry’s state != new_state, then insert `(line_number + 1, new_state)` into the cache, and move this element of the frontier to that entry.
 - Otherwise, just delete this element from the frontier.

The only other subtle operation is deleting an entry from the cache (especially evictions). If that entry is in the frontier, then the element of the frontier must be moved to the previous entry.

## On the representation of the frontier
It’s tempting to truncate the frontier, rather than storing it as a set. In particular, it’s perfectly correct to just store it as a reference to the first entry. Then, the operation of adding an element to the frontier reduces to just taking the minimum.

However, this temptation should be resisted. Let’s say the user opens a comment at the beginning of a large file. The frontier slowly ripples through the file, recomputing highlighting so that all lines are in a “commented” state. Then say the user closes the comment when the frontier is about halfway through the file. This edit will cause a new frontier to ripple down, restoring the uncommented state. With the full set representation of the frontier, the old position halfway through the file will be retained, and when the new frontier reaches it, states will match, so processing can stop.

If that old position were not retained, then the frontier would need to ripple all the way to the end of the file before there would be confidence the entire cache was valid. So, for a relatively small cost of maintaining the frontier as a set, we get a pretty nice optimization, which will improve power consumption and also latency (the editor can respond more quickly when it has quiesced as opposed to doing computation in the background).

## Tuning the cache
This is where the rocket science starts. Please check your flight harnesses.

### Access patterns
Before we can start tuning the cache, we have to characterize the access patterns. In an interactive editing session, the workload will consist of a mix of three fundamental patterns: sequential, local, and random.

Sequential is familiar from the first algorithm we presented. It’s an important case when first loading a file. It will also happen when edits (such as changing comment balance) cause state changes to ripple through the file. The cache is basically irrelevant to this access pattern; the computation has to happen in any case, so the only purpose of the cache is not to have significant overhead.

By “local,” we mean edits within a small region of the file, typically around one screenful. Most such edits won’t cause extensive state changes, in fact should result in re-highlighting of just a line or two. In this access pattern, we want our algorithm to recompute tiny deltas, so the cache should be dense, meaning that the gap between the closest previous cache entry and the line being edited be zero or very small.

The random access pattern is the most difficult for a cache to deal with. The best we can possibly do is O(n/m), as above. We expect these cases to be rare compared with the other two, but it is still important to have reasonable worst-case behavior.

Any given editing session will consist of all three of these patterns, interleaved, in some relative proportions. This is significant for designing a well-tuned cache, especially because processing some work from one pattern may leave the cache in poor condition for the next.

### Analyzing the cache performance
In most applications, cache performance is characterized almost entirely by its hit rate, meaning the probability that any given query will be present in the cache. Most [cache eviction policies](https://en.wikipedia.org/wiki/Cache_replacement_policies) are chosen to optimize this quantity.

However, for this algorithm, the cost of a cache miss is highly dependent on the gap between entries, and the goal should be to minimize this gap.

From this viewpoint, we can see that the LRU (least recently used) policy, while fine for local access patterns, is absolutely worst case when mixing sequential with anything else; after sequential procesing, the cache will consist of a dense block (often at the end of the file), with a huge gap between the beginning of the file and that block. As Dan Luu’s excellent [case study](http://danluu.com/2choices-eviction/) points out, LRU can also have this kind of pathological performance in more traditional applications such as memory hierarchies.

For the “random” access pattern, the metric we care about is maximum gap; this establishes a worst case. For LRU, it is O(n), which is terrible. We want to do better.

The obvious next eviction policy candidate to consider is randomized. In traditional cache applications, random eviction fixes the pathology with perfectly sequential workloads, and performs reasonably well overall (in Dan’s analysis, it is better than LRU for some real-world workloads, worse in others, and in no case has a hit rate more than about 10% different).

I tried simulating it [TODO: a more polished version of this document would contain lots of beautiful visualizations, plus a cleaned up version of the simulation code], and the maximum-gap metric was horrible, almost as bad as it can get. In scanning the file from beginning to end, in the final state the entries near the beginning are decimated; a typical result is that the first entry remaining in the cache is about halfway through the file.

For a purely random workload, an ideal replacement policy would be to choose the entry with the smallest gap between previous and next entries. A bit of analysis shows that this policy would yield a maximum gap of 2n/m in the worst case. However, it won’t perform well for local access patterns - basically, the state of the cache will become stuck, as lines most recently added are likely to also have the smallest gap. Thus, local edits will still have a cost around n/m lines re-highlighted. It doesn’t make sense to optimize for the random case at the expense of the local one.

Inspired by Dan’s post, I sought a hybrid. My proposed cache eviction policy is to probe some small number k of random candidates, and of those choose the one with the smallest gap as defined above. In my simulations [TODO: I know, this really needs graphs; what I have now is too rough], it performs excellently.

There’s no obvious best choice of k, it’s a tradeoff between the expected mix of local (where smaller is better) and random (where larger is better). However, there seems to be a magic threshold of 5; for any smaller value, the maximum gap grows very quickly with the file size, but for 5 or larger it levels off. In a simulation of an 8k entry cache and a sequential scan through an 8M line file, k=5 yielded a maximum gap of ~9k lines (keep in mind that 2k is the best possible result here). Beyond that, increasing k doesn’t have dramatic consequences, even at k=10 this metric improves only to ~3600, and that’s at the expense of degrading local access patterns.

Obviously it’s possible to do a more rigorous analysis and more fine-tuning, but my gut feeling is that this very simple policy will perform within a small factor of anything more sophisticated; I’d be shocked if any policy could improve performance more than a doubling of the cache size, and with the cache sizes I have in mind, that should be well affordable. And a larger cache size always has the advantage that any file with a number of lines that fits entirely within the cache will have perfect effectiveness.

### Cache size and representation
Choosing cache size is always a tradeoff between cache effectiveness (whether hit rate or maximum-gap) and the cost of the cache itself. A larger cache should increase effectiveness, but how much?

This is an empirical question, but we can try to analyze it. Cache effectiveness is irrelevant for sequential access. For the local case, it would be reasonable to expect that the “working set” is quite small, typically on the order of 1000 lines or so.

And for the random case, the cache only has to perform reasonably well; we expect these cases to be rare.

From this, we can guess that the cache doesn’t have to be very large to be effective. Thus, a very simple representation is a dense vector of entries. Some operations (such as deletion and fixup of line numbers) are O(m) in the size of the cache, but with a very good constant factor due to the vector representation. So, while it’s tempting to use a fancy O(log m) data structure such as a B-tree, this is probably a case where simpler is better.

My gut feeling is that a fixed maximum size of 10k entries will yield near-optimal results in all cases.

## Implementation state and summary
I haven’t implemented this yet (beyond the simulations), but really look forward to it.

Based on my analysis, this algorithm should provide truly excellent performance, producing minimal deltas with very modest memory requirements. I’m also pleased that the code and data structures are relatively easy; I have considered much more sophisticated approaches (including of course my beloved balanced-tree representation for the cache), which in analysis wouldn’t perform nearly as well.

I think it would be interesting to do a more rigorous analysis. It’s possible this technique has already been investigated somewhere, but I’m not aware of it; I’d love to find such a literature.

Thanks to Colin Rofls for stimulating discussions about caching in plugins that inspired many of these ideas.
