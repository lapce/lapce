# Rope science, part 10 - designing for a conflict-free world
(originally written 11 May 2016)

In today’s post, we’ll deep dive into a very specific problem (namely, the design of an asynchronous plugin that does indentation) and then try to derive some more general lessons from it.

The indentation problem seems relatively simple on the surface. Let’s say the buffer starts out with “foo {|}” (the cursor position represented by a pipe), and the user presses Enter. Let’s let the core synchronously insert a newline, and notify a plugin to compute the correct indentation. Of course, correct indentation is a hard problem, relying on an accurate parse of the source text, which is why many editors give up and compute approximate indentation only, usually using regexes. I find that annoying.

In a synchronous plugin model, it’s not conceptually hard. The plugin thinks for a (hopefully short) while, then inserts “….\n” after the newline (dots representing spaces), and moves the cursor to a point between the spaces and the newline. Now the editor is ready to accept edits again, and hopefully any keys pressed were buffered in-order. The buffer and cursor now read “foo {\n….|\n}”.

We want to do even better, allowing the user to type while the plugin is thinking. So, if the user types “bar” concurrently with the plugin’s edit, we want the buffer to read “foo {\nbar|}” transiently, converging on “foo\n…bar|\n}” when the plugin posts its edit.

If you were to just apply the edit commands (insert, move cursor) out of order, you’d end up with an interleaved mess, arguably a much worse experience than just the latency spike. Unfortunately, I’ve seen quite a bit of this kind of behavior. How to avoid it?

The mini-CRDT I’ve talked about earlier helps with the “insert” part of the plugin’s edit, but the cursor move turns out to be a lot trickier. The fact is, cursor movement just doesn’t commute in any meaningful way with edit operations dependent on cursor position. You could imagine trying to do some kind of replay of the user’s keystrokes so eventually you get the sequential answer, but that makes the system even more complicated in ways I find unappealing, and also fails to scale to actual collaborative editing.

How to fix it? The answer, I think, rests on thinking in terms of conflict-free primitives, as opposed to trying to bolt concurrency onto a synchronous design. Basically, I propose to take cursor movement out of the plugin’s edit operation entirely.

Deep in the WOOT paper is the concept of assigning an id to each character, so that in the case of concurrent edits where you end up with only a partial ordering of character, you can use these id’s to break ties. There’s no suggestion you can use these id’s for anything other than forcing some order so you can get consistent results.

However, I think we can repurpose these id’s into priority, repurposing the tie-breaking logic to give us the results we want. In this model, instead of each insert being annotated with just a unique id of the actor, it’s annotated with a tuple of (priority, unique id). We can sort these in lexicographical order, so the CRDT goal of ensuring a consistent order is still met. But by assigning appropriate priorities, we can now get the edits in the correct order. In particular, give “….” the priority “before cursor”, “bar” the priority “just before cursor” (which is the default priority of all characters inserted from the keyboard), and “\n” the priority “after cursor”. Insert all of these strings at the same location, and voila, you get the desired answer. Further, you can include the cursor position itself in the ordering logic, resulting in it naturally landing after “bar” without it having to be explicitly positioned. I will implement exactly this, because figuring out the correct position of the cursor after a complex sequence of undo/redo and other edits is otherwise not trivial.

The semantics of conflict-free datatypes are subtly different from their sequential counterparts. There are certain things you can easily do in the sequential world that don’t work conflict-free. However, you can make the datatype richer in other ways that still fit in the conflict-free framework, and solve the original problem just as well, or even better. I think you’ll see a lot more instances of this general pattern as CRDT becomes more popular.

Thanks to Yigit Boyar for a lunch conversation which helped me clarify many of these thoughts.
