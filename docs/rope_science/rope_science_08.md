# Rope science, part 8 - CRDTs for concurrent editing
(originally written 3 May 2016)

To briefly recap: we want to run plugins asynchronously, so they have a little time to “think” (especially for things like deeper analysis of programs), but still not get in the way of typing responsiveness. Many problems of dealing with input methods are also fundamentally concurrent edits at heart. Asynchronous loading of a large file (appending chunks at the end) is also a form of concurrent edit, and it is essential not to get into inconsistent states.

It would be a lot easier to just treat all editing operations as synchronous, but in xi we want to do better. For a while, operational transforms were considered the state of the art, but as Xoogler Joseph Gentle wrote, “Unfortunately, implementing OT sucks. There’s a million algorithms with different tradeoffs, mostly trapped in academic papers. The algorithms are really hard and time consuming to implement correctly. … Wave took 2 years to write and if we rewrote it today, it would take almost as long to write a second time.”

Conflict-free Replicated Data Types (CRDTs) are a solution. They are mostly discussed in a peer-to-peer context, with the usual assumptions of networks being partitioned, nodes failing, and mobile devices going offline for extended periods, because CRDTs actually work in that environment. A text editor is much easier. Is it possible to apply the ideas of CRDT, while exploiting the assumption that the editor core is fast and reliable, to create a simple implementation that nonetheless does the Right Thing? I believe the answer is yes.

Let’s start with highly simplified assumptions, to make life easier, and then back those off. First, let’s assume we’re only adding text, never deleting. Second, let’s assume we have an oracle that knows the location where each character will land in the string at the end of the edit session. Obviously, such an oracle is unrealistic, but we’ll figure out how to compute it on-the-fly after we’ve got the basic concurrent editing operations done.

Given those assumptions, the basic editing operation is nearly trivial. You just represent your text as a sequence of cells, and the editing operation takes contents of the cell from empty to “some character.” For those familiar with CRDT theory, this operation is obviously a monotonic semi-lattice, and the update operation is equally obviously commutative - you can apply the updates in any order and get to the same result.

Now, how would you go about computing the oracle? The key insight is that it’s fairly easy to make any two snapshots align, given knowledge of the deltas. A snapshot is conceptually a view of the final string in which some of the cells are empty and some are not. So when you look at two snapshots, it’s got some cells filled in common, some cells filled only in one, and some cells filled only in the other. The representation of a snapshot is simply the filled cells concatenated together. Then aligning two snapshots is basically figuring out a coordinate transform so that all of the filled cells in common line up.

Take the simple case of inserting a sequence at some point. Then all coordinates less than that point are preserved, and for all coordinates greater, you add the length of the inserted sequence. This coordinate transform is, funny enough, pretty much the same as what you do for a gapped buffer. The coordinate exactly at that point is an interesting case, but I think the best way to approach it is tie-breaking based on the identity of who made the insert. For example, you’d want the spaces added by an autoindent plugin to come before keys typed by the user, because that more faithfully sustains the illusion that the autoindent plugin is infinitely fast.

So, to make this a little more concrete: an actor (the user, a plugin) makes an editing request, which is an insertion of a sequence at a point relative to its snapshot of the buffer. The core then commits that request, which might require a coordinate transform based on other edits that have arrived in the meantime (ie between the current state and the snapshot referenced by the plugin). Actually very simple.

But what about deletes? The straightforward way (deleting the sequence from the buffer, trying to do the same kind of coordinate transform, this time subtracting the length of the deleted subsequence) ruins commutativity. In fact, the whole model falls apart, making OT fiendishly complex and messy. The key insight of CRDT is that you can get back a monotonic update operation if you give each cell three states: empty, filled, and deleted, and each update progresses only forward through these states. In mathematical terms, this is a monotonic semi-lattice again. In a concrete implementation, you keep the spans around in your shared state, but mark them as deleted (these are known as “tombstones”), and of course do the deletion before actually rendering to the screen.

Optionally, you can add some form of garbage collection to clean up the removed spans. In an editor, it’s reasonable to assume that the amount of concurrency is very small. In fact, in most normal operation the buffer will often quiesce (no pending edits), at which point in the simplest case you can just delete out the spans. In practice, I suspect we’ll need to keep some of the deleted spans around for undo (if you undo a delete, you really want the deleted text to appear in the same place relative to its context), but in even so I expect the number of edits in flight will be small enough you can use really simple algorithms. Let’s say it’s bounded by 20, then O(n^2) or even O(n^3) is fine.

So I think it’s possible to make the implementation very simple based on the fact that the core is fast and reliable, and that the scale of concurrency is limited. And if that does ever need to scale up (say, because this thing would get used to do actual collaborative editing), then the CRDT literature is rich in techniques for that.

It feels like I can prototype this in a few days, as opposed to the long hard slog of trying to do OT from scratch. I’m looking forward to it, and looking forward to seeing what I learn, as these ideas still feel a little rough.

## Context
I had written this post the day before:

Homework for the next installment of the rope science series.

I crammed some readings on CRDTs the past few days, and last night believe I came up with a clean, simple solution for dealing with concurrent editing by asynchronous plugins. I really recommend the CRDT concept, it hugely helped me focus my thinking. To make sense of my next post, it’s kinda necessary to catch up on CRDTs.

I recommend starting with [Marc Shapiro’s 2011](https://www.microsoft.com/en-us/research/video/strong-eventual-consistency-and-conflict-free-replicated-data-types/) talk at Microsoft Research explaining the CRDT concept and the underlying mathematical concept of monotonic semi-lattice.

The concept of CRDT actually started with the collaborative editing use case, before they generalized it to more general cloud-flavored problems (it now powers eventual consistency in Riak, among other things). The [WOOT paper](https://hal.inria.fr/inria-00071240/document) (2006) is very good at comparing the new approach to operational transforms, but I’m definitely not going to adapt the details of that implementation.

Lastly, I recommend reading [A comprehensive study of Convergent and Commutative Replicated Data Types](http://hal.upmc.fr/inria-00555588/document) (2011), which surveys many of the use cases and goes into more detail about the duality of the state-based and operation-based approaches to implementing CRDTs. It also provides a very useful retrospective on the application to collaborative text editing (WOOT and TreeDoc), placing the somewhat complex implementation details into a clean mathematical framework.

Having read all this, at this point it’s clear to me that CRDTs are the One True Way to do collaborative editing. The framework tells you clearly what you’d need to do to make operational transforms actually work (make the update operation commutative), why that’s so difficult (delete operations lose state) and how to make your life much easier (retain delete state and do some form of GC after the fact). These talks and papers really helped me focus on what concurrency problem I’m trying to solve in xi and how to go about it.

Similarly, differential synchronization is an interesting branch to explore (and a great demonstration that the full complexity of OT is not needed), but when the history of collaborative editing techniques is written, I believe it will have gotten lost in a merge conflict.

If you’ve followed along with me so far, I believe the next post will be pretty exciting
