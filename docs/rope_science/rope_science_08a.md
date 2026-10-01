# Rope science, part 8a - CRDT follow-up
(originally written 5 May 2016)

Some notes and observations, won’t be published externally in the “rope science” series, but some of the stuff I talk about will be folded into the part 8 writeup.

It feels like I almost have a working design that handles undo as well as basic merging of async results described in the last post. Right now, it feels like wrestling with a bear. A lot of the papers on OTs and related topics are just bad. Also, in tricky situations, the “right thing” is poorly defined. Frankly, in an editing context it’s possible to tolerate quite a bit of weirdness; if what you see on the screen isn’t what you expected, just change it.

Even so, I’m still feeling that there’s value in requiring update operations to be commutative (the fundamental correctness criterion of CRDTs). If this requirement forced significant complexity or performance loss, I’d definitely reconsider, but at this point it seems like a clean solution is possible, and likely well within my budget on both.

Touching some points in my last write-up. I think I’ll abandon the “oracle” concept, it’s probably confusing. I think the best way to think about the model is that each character has a coordinate, and that there’s a total order on these coordinates, which is derived both from explicit partial order constraints (when you insert ‘x’ between ‘a’ and ‘b’, you know ‘a’ < ‘x’ and ‘x’ < ‘b’) and tie-breaking rules. In a highly distributed CRDT implementation such as WOOT, the coordinate is a unique ID and you pass around explicit partial order edges.

In my much more constrained world (a central core which can serialize all the requests), you could do the same thing, but a much simpler implementation is to just use integers for the coordinates. Because you’re dealing with multiple revisions at a time, you do coordinate transformations, so a character with the same “unique id” has a different index in different revisions. That actually gives the implementation a feel a lot closer to OTs, but I think it’s important to emphasize that what it’s computing is the same as a CRDT. I’m planning to use simple O(n^2) algorithms to represent these transformations explicitly, where n is the number of updates “in flight”. I wouldn’t expect this to work well in an actual distributed p2p environment, and, if someone (likely not me) were to pull xi in the direction of being a collaborative editor, what I’d recommend is ripping out the simple serialized CRDT implementation and replacing it with something along similar lines as WOOT. Ideally, this wouldn’t break anything in the rest of the editor, because everything is designed to handle asynchrony, and unless I screw things up

Undo fits into the model, but is not a trivial extension, in fact it increases the overall complexity quite a bit. This is not surprising, as I personally would consider undo a hard problem, especially in the face of asynchronous updates. We’ve certainly got Android bugs to prove it :) However, I feel like I’ve almost got my mind wrapped around it. I am going to simplify it further by only allowing one agent (the user, operating through a UI front-end) to control undos, rather than allowing arbitrary plugins to undo each other’s changes.

[In a collaborative editing environment, this assumption would have to be revisited. I can see how you can build a CRDT for undo choices, basically where for every “undo group” each participant has a nonnegative integer indicating undo preference, the semilattice join operation being pointwise max of this vector, and the “undo group” being considered undone when the sum of the vector is odd. But for the purposes of making Ctrl-Z do something reasonable, this would be serious overengineering].

I’m hoping to have code over the weekend. I’m feeling torn whether to write or code, especially as the design is not yet fully formed.

Thanks to David Espinosa and Adam Sadovsky for a great (if unfortunately rushed) conversation yesterday (4 May 2016) which helped clarify a lot of the tricky bits for me.

## In comments:
Malcolm Rowe asked whether the asynchronous autoformatting would fight with manual formatting in practice. I answered:

I think that mostly depends on the distribution of delays. If it’s a couple hundred ms, then the chance of actual conflict is low, and the gain in both real and perceived responsiveness significant. Unfortunately, in Eclipse, multi-second delays are common, which is one reason I hate using Eclipse so much. There are other factors than delay, probably mostly how predictable the automatic edits are (if they constantly need manual fixup, then the chance of conflict is higher), but I suspect delays will dominate.

