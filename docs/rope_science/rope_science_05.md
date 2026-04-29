# Rope science, part 5 - incremental word wrapping
(originally written 22 Apr 2016)

This post reflects actual working code, recently committed to the xi git repo. I’m proud of it. It draws on high-level mathematical concepts, but at the end of the day the design decisions are driven by hard-nosed engineering considerations.

## Word wrapping
I’ll focus on the fragment of the word wrapping problem most useful for code editing. In particular, I’ll assume for the sake of simplicity that it is trivial to compute the width of a given substring (in fact, code that relies on this assumption will stay, as I’ll rely on a “difficulty analysis” to detect whether it’s valid). Then, word wrapping can be defined as a scan through the document’s “candidate breaks,” and breaking either when the document has a hard break, or where there’s a soft break and adding the word (defined simply as the substring between two candidate breaks) when its width would cause the total line width to overflow.

A simple way to get candidate breaks is just to consider the transition between space and non-space to be a soft break, but the Right Way is to use the Unicode [UAX #14](http://unicode.org/reports/tr14/) line breaking algorithm, so we do that. Either way, the interface is pretty simple, you basically call an iterator and on every call it gives you the next break.

The basic idea can be expressed in a dozen lines or so lines of code, and indeed you’ll see that in, say, Python scripts that emit auto-generated code with not-terrible formatting.

## Functional programming
The result of word wrapping an input string (given a target line length) is a pure function of its input. Further, there’s structure that can be exploited. At a minimum, every paragraph is independent. You can split on hard breaks (newlines, essentially), compute the word wrap of each paragraph, and stick them back together again, and get the same answer.

Given that observation, there are many, many strategies for computing this function. You could compute it on the fly every time you needed it, you could store the computed result and invalidate it every time the input changed, etc. You can also try to break it into segments and use different strategies for each segment. I seriously considered monoid constructions that could be used to parallelize or incrementalize the problem, but ended up discarding them because they don’t have the best engineering characteristics. In xi, I choose to store the result of the word wrap calculation, so that the results are immediately (with O(log n) random access cost) available for scrolling. Deferring the calculation in any way increases the likelihood of jank, and indeed I see that in many editors on large files.

Recomputing the whole thing on every edit would take too long to meet my performance targets (though it would probably still be faster than Atom). The only viable approach is some form of incremental computation.

At a high level, incremental computation is using the explicit delta between the new value and the old value, plus the old value of the function, to compute the new one. If everything goes right, you get the same answer as if you had just computed the function from scratch over the new value (and, indeed, aggressively testing for this, especially in the face of random input, is a great thing to automate).

Incremental computation is a universal pattern in GUI programming, because it’s almost always the case that rebuilding and redrawing the entire widget hierarchy from scratch every frame is too slow. Thus, the app pokes mutations at individual widgets, then those widgets propagate invalidations through the view hierarchy, and at the end of it the GUI framework redraws what it needs to. Still, making sure it’s a performance win requires care, because oftentimes the incremental computation is slower than a batch one would have been.

A simple application of the incremental computation idea to line wrapping is to store the result of each paragraph separately, and for each paragraph not affected by the delta, just reuse that value. And indeed, that’s what DynamicLayout does in Android. There’s some bookkeeping involved to handle split and merged paragraphs, but overall it’s not too bad.

However, it’s not really doing a minimal amount of work, especially if you have a very long paragraph. Worse, in the common case where only a line or two is affected, you’ve lost that information and have to redraw the entire paragraph.

## An editing approach
In xi, an edit on a rope is represented explicitly (there’s a data structure called Delta) as an interval in the original rope, and a new rope that replaces the interval. Delete and insert are special cases (the new rope and interval are empty, respectively). Then, the incremental line wrap problem is defined as a function that takes the underlying string, the delta, and the old breaks, and produces an edit on the breaks. The breaks themselves are stored and edited using the same b-tree rope infrastructure (conceptually it can be thought of as a string that is either “break” or “no break” at every code unit, but is of course encoded more efficiently than that).

Seen this way, the problem turns out to be not particularly difficult. You start out two breaks back from the start of the edit, because an edit at the start of a line can cause a word to pop to the previous line (I could be smarter and only jump back one when it’s a hard break, but it wouldn’t make that much difference). Then, you run your word wrapper forward from that point. Once you hit a break which is both after the edit, and which agrees with an break in the previous wrap result (guaranteed to happen at the next hard break, if nothing else), you stop, and just edit what you’ve got in. All successive breaks will continue to occur at the same places as before, so you don’t have to touch them.

Note that you don’t have to deal with “merging” or “splitting” of a line as special cases. If you want to know whether you need to scroll the part below the edit up or down, you can just count the number of breaks in the edited interval, before and after. Further, if the edit is small you can use that do do a minimal repaint (not yet implemented, just redrawing the screen is plenty fast enough on modern desktops, but I will do it as soon as the basics settle down).

## Results
My standard test file is a 300M chunk of ninja from the Android build process, with one line that’s 1.5M. It takes about 1s to load, and another 2s to word-wrap in bulk. This is pretty good, compared with about 30s to load and line-wrap in Sublime Text. Sublime also seems to be in the 100ms latency range when editing that long line. Atom fails to load the file entirely (and shows extremely noticeable jank in a file only 2M). Chrome also fails to scroll to the end of the 300M file (though it manages to display the first page of it pretty quick), and also janks on the 2M one.

The speed of incremental word wrapping in xi is in the “almost too fast to measure” range. Instrumentation around the word wrap function clocks in at around 40µs, even on the long line, because nowhere is it touching the whole thing. It’s just looking at a small window around the edit, to produce a small edit to the line breaks structure.

So without doing any more performance work at all, xi is quite competitive. But I have a few more tricks up my sleeve, and I’ll talk about those in an upcoming post.

## In comments
Nigel Tao asked about ligatures. I responded:

Ok, the answer to your ligature question is complicated but hopefully interesting. This will be something of a mini-post.

First, in the case of ASCII and a monospace font, ligatures and kerning aren’t in effect and can’t change the result of a word wrap operation. This is an important enough case that I will detect it and use extremely fast calculations for width.

The other cases are also important, of course. Fundamentally, what you’re trying to compute is the width of each “word”. More precisely, you’re trying to predict the total width of a line containing a sequence of words, so that it meets the necessary constraints (for editing, generally a greedy packing of the maximum allowable width into the line). And, to a first approximation, that line width is the sum of the individual words, minus the whitespace at the end.

Now, within a word (in the non-ASCII or non-monospace cases), it’s probably best not to make too many assumptions. In Latin script, editing a character will cause a small delta from the sum of the individual letter widths, but predicting that delta precisely is hard. Even with simplifying assumptions that kerning and ligature formation are pair-wise doesn’t make the problem easy. The Knuth-Plass paper on line breaking (reprinted in “Digital Typography”) considers the possibility that ligature formation will cascade, and basically recommend “don’t do that” for the font creator. But once you get into complex script territory, all bets are off - a small edit can change whether a consonant cluster forms, whether an Arabic presentation form is initial or medial, etc., all of which can have huge impacts on width.

So I don’t recommend trying to do anything incremental within a word, just re-measure its width. This is of course potentially very expensive, and among other things, it’s important to have a well-tuned cache.

The other subtlety is, of course, those “word” boundaries. To do this in a principled way, you really want to know the boundaries that don’t interact with width, ie if you segment the string at those boundaries, calculate the widths independently, and sum, you’ll get the same answer. Most of the time, those boundaries line up pretty well with the line break boundaries, but not always. For one, the width boundaries are highly dependent on the font. At one extreme, in a monospace font, per-character is completely fine. On the other hand, if the font allows space to participate in kerning (or contextual substitutions) then your calculation will be off. This will happen in a high-quality Urdu (Nastaliq) font. Android hardcodes boundaries (the details are in getNextWordBreakForCache in Minikin’s LayoutUtils.cpp) that work pretty well in practice, and xi will do something similar. HarfBuzz is also developing an API for querying a font do detect these boundaries, and in the future Android may well adopt it.

Stuff like this is one of the reasons why I’ve started to see text as a sequence of codepoints in a loose hierarchy of boundaries, with no one particular assignment of boundaries being privileged.
