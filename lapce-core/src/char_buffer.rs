extern crate alloc;

use alloc::{borrow::Cow, rc::Rc, sync::Arc};
use core::{
    borrow::Borrow, cmp::Ordering, convert::AsRef, fmt, hash, ops::Deref, str,
};

/// This is a small memory buffer allocated on the stack to store a
/// ‘string slice’ of exactly one character in length. That is, this
/// structure stores the result of converting [`char`] to [`prim@str`]
/// (which can be accessed as [`&str`]).
///
/// In other words, this struct is a helper for performing `char -> &str`
/// type conversion without heap allocation.
///
/// # Note
///
/// In general, it is not recommended to perform `char -> CharBuffer -> char`
/// type conversions, as this may affect performance.
///
/// [`&str`]: https://doc.rust-lang.org/core/primitive.str.html
///
/// # Examples
///
/// ```
/// use lapce_core::char_buffer::CharBuffer;
///
/// let word = "goodbye";
///
/// let mut chars_buf = word.chars().map(CharBuffer::new);
///
/// assert_eq!("g", chars_buf.next().unwrap().as_ref());
/// assert_eq!("o", chars_buf.next().unwrap().as_ref());
/// assert_eq!("o", chars_buf.next().unwrap().as_ref());
/// assert_eq!("d", chars_buf.next().unwrap().as_ref());
/// assert_eq!("b", chars_buf.next().unwrap().as_ref());
/// assert_eq!("y", chars_buf.next().unwrap().as_ref());
/// assert_eq!("e", chars_buf.next().unwrap().as_ref());
///
/// assert_eq!(None, chars_buf.next());
///
/// for (char, char_buf) in word.chars().zip(word.chars().map(CharBuffer::new)) {
///     assert_eq!(char.to_string(), char_buf);
/// }
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CharBuffer {
    len: usize,
    buf: [u8; 4],
}

/// The type of error returned when the conversion from [`prim@str`], [`String`] or their borrowed
/// or native forms to [`CharBuffer`] fails.
///
/// This `structure` is created by various `CharBuffer::try_from` methods (for example,
/// by the [`CharBuffer::try_from<&str>`] method).
///
/// See its documentation for more.
///
/// [`CharBuffer::try_from<&str>`]: CharBuffer::try_from<&str>
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CharBufferTryFromError(());

impl CharBuffer {
    /// Creates a new `CharBuffer` from the given [`char`].
    ///
    /// # Examples
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let char_buf = CharBuffer::new('a');
    /// assert_eq!("a", &char_buf);
    ///
    /// let string = "Some string";
    /// let char_vec = string.chars().map(CharBuffer::new).collect::<Vec<_>>();
    /// assert_eq!(
    ///     ["S", "o", "m", "e", " ", "s", "t", "r", "i", "n", "g"].as_ref(),
    ///     &char_vec
    /// );
    /// ```
    #[inline]
    pub fn new(char: char) -> Self {
        let mut buf = [0; 4];
        let len = char.encode_utf8(&mut buf).as_bytes().len();
        Self { len, buf }
    }

    /// Converts a `CharBuffer` into an immutable string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let char_buf = CharBuffer::from('r');
    /// assert_eq!("r", char_buf.as_str());
    /// ```
    #[inline]
    pub fn as_str(&self) -> &str {
        self
    }

    /// Returns the length of a `&str` stored inside the `CharBuffer`, in bytes,
    /// not [`char`]s or graphemes. In other words, it might not be what a human
    /// considers the length of the string.
    ///
    /// # Examples
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let f = CharBuffer::new('f');
    /// assert_eq!(f.len(), 1);
    ///
    /// let fancy_f = CharBuffer::new('ƒ');
    /// assert_eq!(fancy_f.len(), 2);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Always returns `false` since this structure can only be created from
    /// [`char`], which cannot be empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let c = CharBuffer::new('\0');
    /// assert!(!c.is_empty());
    /// assert_eq!(c.len(), 1);
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        false
    }
}

impl From<char> for CharBuffer {
    /// Creates a new [`CharBuffer`] from the given [`char`].
    ///
    /// Calling this function is the same as calling [`new`](CharBuffer::new)
    /// function.
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let char_buf = CharBuffer::from('a');
    /// assert_eq!("a", &char_buf);
    ///
    /// let string = "Some string";
    /// let char_vec = string.chars().map(CharBuffer::from).collect::<Vec<_>>();
    /// assert_eq!(
    ///     ["S", "o", "m", "e", " ", "s", "t", "r", "i", "n", "g"].as_ref(),
    ///     &char_vec
    /// );
    /// ```
    #[inline]
    fn from(char: char) -> Self {
        Self::new(char)
    }
}

impl From<&char> for CharBuffer {
    /// Converts a `&char` into a [`CharBuffer`].
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let string = "Some string";
    /// let char_vec = string.chars().collect::<Vec<_>>();
    /// assert_eq!(
    ///     ['S', 'o', 'm', 'e', ' ', 's', 't', 'r', 'i', 'n', 'g'].as_ref(),
    ///     &char_vec
    /// );
    ///
    /// let string_vec = char_vec.iter().map(CharBuffer::from).collect::<Vec<_>>();
    ///
    /// assert_eq!(
    ///     ["S", "o", "m", "e", " ", "s", "t", "r", "i", "n", "g"].as_ref(),
    ///     &string_vec
    /// );
    /// ````
    #[inline]
    fn from(char: &char) -> Self {
        Self::new(*char)
    }
}

impl From<&mut char> for CharBuffer {
    /// Converts a `&mut char` into a [`CharBuffer`].
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let string = "Some string";
    /// let mut char_vec = string.chars().collect::<Vec<_>>();
    /// assert_eq!(
    ///     ['S', 'o', 'm', 'e', ' ', 's', 't', 'r', 'i', 'n', 'g'].as_ref(),
    ///     &char_vec
    /// );
    ///
    /// let string_vec = char_vec
    ///     .iter_mut()
    ///     .map(CharBuffer::from)
    ///     .collect::<Vec<_>>();
    ///
    /// assert_eq!(
    ///     ["S", "o", "m", "e", " ", "s", "t", "r", "i", "n", "g"].as_ref(),
    ///     &string_vec
    /// );
    /// ````
    #[inline]
    fn from(char: &mut char) -> Self {
        Self::new(*char)
    }
}

impl From<CharBuffer> for char {
    /// Creates a new [`char`] from the given reference to [`CharBuffer`].
    ///
    /// # Note
    ///
    /// In general, it is not recommended to perform `char -> CharBuffer -> char`
    /// type conversions, as this may affect performance.
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let char_buf = CharBuffer::from('a');
    /// let char: char = char_buf.into();
    /// assert_eq!('a', char);
    ///
    /// let string = "Some string";
    ///
    /// // Such type conversions are not recommended, use `char` directly
    /// let char_vec = string
    ///     .chars()
    ///     .map(CharBuffer::from)
    ///     .map(char::from)
    ///     .collect::<Vec<_>>();
    ///
    /// assert_eq!(
    ///     ['S', 'o', 'm', 'e', ' ', 's', 't', 'r', 'i', 'n', 'g'].as_ref(),
    ///     &char_vec
    /// );
    /// ````
    #[inline]
    fn from(char: CharBuffer) -> Self {
        // SAFETY: The structure stores a valid utf8 character
        unsafe { char.chars().next().unwrap_unchecked() }
    }
}

impl From<&CharBuffer> for char {
    /// Converts a `&CharBuffer` into a [`char`].
    ///
    /// # Note
    ///
    /// In general, it is not recommended to perform `char -> CharBuffer -> char`
    /// type conversions, as this may affect performance.
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let char_buf = CharBuffer::from('a');
    /// let char: char = char::from(&char_buf);
    /// assert_eq!('a', char);
    ///
    /// let string = "Some string";
    ///
    /// // Such type conversions are not recommended, use `char` directly
    /// let char_buf_vec = string.chars().map(CharBuffer::from).collect::<Vec<_>>();
    /// let char_vec = char_buf_vec.iter().map(char::from).collect::<Vec<_>>();
    ///
    /// assert_eq!(
    ///     ['S', 'o', 'm', 'e', ' ', 's', 't', 'r', 'i', 'n', 'g'].as_ref(),
    ///     &char_vec
    /// );
    /// ````
    #[inline]
    fn from(char: &CharBuffer) -> Self {
        // SAFETY: The structure stores a valid utf8 character
        unsafe { char.chars().next().unwrap_unchecked() }
    }
}

impl From<&CharBuffer> for CharBuffer {
    /// Converts a `&CharBuffer` into a [`CharBuffer`].
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let char_buf1 = CharBuffer::from('a');
    /// let char_buf2: CharBuffer = CharBuffer::from(&char_buf1);
    /// assert_eq!(char_buf1, char_buf2);
    ///
    /// let string = "Some string";
    /// let char_vec1 = string.chars().map(CharBuffer::from).collect::<Vec<_>>();
    /// let char_vec2 = char_vec1.iter().map(CharBuffer::from).collect::<Vec<_>>();
    ///
    /// assert_eq!(char_vec1, char_vec2);
    /// ````
    #[inline]
    fn from(char: &CharBuffer) -> Self {
        *char
    }
}

impl From<CharBuffer> for String {
    /// Allocates an owned [`String`] from a single character.
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let c: CharBuffer = CharBuffer::from('a');
    /// let s: String = String::from(c);
    /// assert_eq!("a", &s[..]);
    /// ```
    #[inline]
    fn from(char: CharBuffer) -> Self {
        char.as_ref().to_string()
    }
}

impl From<&CharBuffer> for String {
    /// Allocates an owned [`String`] from a single character.
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let c: CharBuffer = CharBuffer::from('a');
    /// let s: String = String::from(&c);
    /// assert_eq!("a", &s[..]);
    /// ```
    #[inline]
    fn from(char: &CharBuffer) -> Self {
        char.as_ref().to_string()
    }
}

impl<'a> From<&'a CharBuffer> for &'a str {
    /// Converts a `&CharBuffer` into a [`prim@str`].
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let c: CharBuffer = CharBuffer::from('a');
    /// let s: &str = From::from(&c);
    /// assert_eq!("a", &s[..]);
    /// ```
    #[inline]
    fn from(char: &'a CharBuffer) -> Self {
        char
    }
}

impl<'a> From<&'a CharBuffer> for Cow<'a, str> {
    /// Converts a `&'a CharBuffer` into a [`Cow<'a, str>`].
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    /// use std::borrow::Cow;
    ///
    /// let c: CharBuffer = CharBuffer::from('a');
    /// let s: Cow<str> = From::from(&c);
    /// assert_eq!("a", &s[..]);
    /// ```
    /// [`Cow<'a, str>`]: https://doc.rust-lang.org/std/borrow/enum.Cow.html
    #[inline]
    fn from(s: &'a CharBuffer) -> Self {
        Cow::Borrowed(&**s)
    }
}

impl From<CharBuffer> for Cow<'_, CharBuffer> {
    /// Converts a `CharBuffer` into a [`Cow<'_, CharBuffer>`].
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    /// use std::borrow::Cow;
    ///
    /// let c: CharBuffer = CharBuffer::from('a');
    /// let s: Cow<CharBuffer> = From::from(c);
    /// assert_eq!("a", &s[..]);
    /// ```
    /// [`Cow<'_, CharBuffer>`]: https://doc.rust-lang.org/std/borrow/enum.Cow.html
    #[inline]
    fn from(s: CharBuffer) -> Self {
        Cow::Owned(s)
    }
}

macro_rules! impl_from_to_ptr {
    (
        $(#[$meta:meta])*
        $ptr:ident
    ) => {
        $(#[$meta])*
        impl From<CharBuffer> for $ptr<str> {
            #[inline]
            fn from(s: CharBuffer) -> Self {
                Self::from(&*s)
            }
        }

        $(#[$meta])*
        impl From<&CharBuffer> for $ptr<str> {
            #[inline]
            fn from(s: &CharBuffer) -> Self {
                Self::from(&**s)
            }
        }
    }
}

impl_from_to_ptr! {
    /// Converts a `CharBuffer` into a [`Arc<str>`].
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    /// use std::sync::Arc;
    ///
    /// let c: CharBuffer = CharBuffer::from('a');
    /// let s1: Arc<str> = From::from(&c);
    /// assert_eq!("a", &s1[..]);
    ///
    /// let s2: Arc<str> = From::from(c);
    /// assert_eq!("a", &s2[..]);
    /// ```
    /// [`Arc<str>`]: https://doc.rust-lang.org/std/sync/struct.Arc.html
    Arc
}

impl_from_to_ptr! {
    /// Converts a `CharBuffer` into a [`Box<str>`].
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    ///
    /// let c: CharBuffer = CharBuffer::from('a');
    /// let s1: Box<str> = From::from(&c);
    /// assert_eq!("a", &s1[..]);
    ///
    /// let s2: Box<str> = From::from(c);
    /// assert_eq!("a", &s2[..]);
    /// ```
    /// [`Box<str>`]: https://doc.rust-lang.org/std/boxed/struct.Box.html
    Box
}

impl_from_to_ptr! {
    /// Converts a `CharBuffer` into a [`Rc<str>`].
    ///
    /// # Example
    ///
    /// ```
    /// use lapce_core::char_buffer::CharBuffer;
    /// use std::rc::Rc;
    ///
    /// let c: CharBuffer = CharBuffer::from('a');
    /// let s1: Rc<str> = From::from(&c);
    /// assert_eq!("a", &s1[..]);
    ///
    /// let s2: Rc<str> = From::from(c);
    /// assert_eq!("a", &s2[..]);
    /// ```
    /// [`Rc<str>`]: https://doc.rust-lang.org/std/rc/struct.Rc.html
    Rc
}

macro_rules! impl_try_from {
    ($lhs:ty) => {
        impl TryFrom<$lhs> for CharBuffer {
            type Error = CharBufferTryFromError;

            fn try_from(str: $lhs) -> Result<Self, Self::Error> {
                let mut chars = str.chars();
                match (chars.next(), chars.next()) {
                    (Some(char), None) => Ok(Self::new(char)),
                    _ => Err(CharBufferTryFromError(())),
                }
            }
        }
    };
}

impl_try_from!(&str);
impl_try_from!(&mut str);

impl_try_from!(String);
impl_try_from!(&String);
impl_try_from!(&mut String);

impl_try_from!(Box<str>);
impl_try_from!(&Box<str>);
impl_try_from!(&mut Box<str>);

impl_try_from!(Arc<str>);
impl_try_from!(&Arc<str>);
impl_try_from!(&mut Arc<str>);

impl_try_from!(Rc<str>);
impl_try_from!(&Rc<str>);
impl_try_from!(&mut Rc<str>);

impl Deref for CharBuffer {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        // SAFETY:
        // - This is the same buffer that we passed to `encode_utf8` during creating this structure,
        //   so valid utf8 is stored there;
        // - The length was directly calculated from the `&str` returned by the `encode_utf8` function
        unsafe { str::from_utf8_unchecked(self.buf.get_unchecked(..self.len)) }
    }
}

impl AsRef<str> for CharBuffer {
    #[inline]
    fn as_ref(&self) -> &str {
        self
    }
}

impl Borrow<str> for CharBuffer {
    #[inline]
    fn borrow(&self) -> &str {
        self
    }
}

#[allow(clippy::derived_hash_with_manual_eq)]
impl hash::Hash for CharBuffer {
    #[inline]
    fn hash<H: hash::Hasher>(&self, hasher: &mut H) {
        (**self).hash(hasher)
    }
}

impl fmt::Debug for CharBuffer {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl fmt::Display for CharBuffer {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl PartialOrd for CharBuffer {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CharBuffer {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        (**self).cmp(&**other)
    }
}

macro_rules! impl_eq {
    ($lhs:ty, $rhs: ty) => {
        #[allow(unused_lifetimes)]
        impl<'a, 'b> PartialEq<$rhs> for $lhs {
            #[inline]
            fn eq(&self, other: &$rhs) -> bool {
                PartialEq::eq(&self[..], &other[..])
            }
        }

        #[allow(unused_lifetimes)]
        impl<'a, 'b> PartialEq<$lhs> for $rhs {
            #[inline]
            fn eq(&self, other: &$lhs) -> bool {
                PartialEq::eq(&self[..], &other[..])
            }
        }
    };
}

impl_eq! { CharBuffer, str }
impl_eq! { CharBuffer, &'a str }
impl_eq! { CharBuffer, &'a mut str }

impl_eq! { CharBuffer, String }
impl_eq! { CharBuffer, &'a String }
impl_eq! { CharBuffer, &'a mut String }

impl_eq! { Cow<'a, str>, CharBuffer }
impl_eq! { Cow<'_, CharBuffer>, CharBuffer }

#[allow(clippy::single_match)]
#[test]
fn test_char_buffer() {
    #[cfg(miri)]
    let mut string = String::from("
    This is some text. Это некоторый текст. Αυτό είναι κάποιο κείμενο. 這是一些文字。"
    );
    #[cfg(not(miri))]
    let mut string = String::from(
        "
    https://www.w3.org/2001/06/utf-8-test/UTF-8-demo.html

    Original by Markus Kuhn, adapted for HTML by Martin Dürst.

    UTF-8 encoded sample plain-text file
    ‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾

    Markus Kuhn [ˈmaʳkʊs kuːn] <mkuhn@acm.org> — 1999-08-20


    The ASCII compatible UTF-8 encoding of ISO 10646 and Unicode
    plain-text files is defined in RFC 2279 and in ISO 10646-1 Annex R.


    Using Unicode/UTF-8, you can write in emails and source code things such as

    Mathematics and Sciences:

      ∮ E⋅da = Q,  n → ∞, ∑ f(i) = ∏ g(i), ∀x∈ℝ: ⌈x⌉ = −⌊−x⌋, α ∧ ¬β = ¬(¬α ∨ β),

      ℕ ⊆ ℕ₀ ⊂ ℤ ⊂ ℚ ⊂ ℝ ⊂ ℂ, ⊥ < a ≠ b ≡ c ≤ d ≪ ⊤ ⇒ (A ⇔ B),

      2H₂ + O₂ ⇌ 2H₂O, R = 4.7 kΩ, ⌀ 200 mm

    Linguistics and dictionaries:

      ði ıntəˈnæʃənəl fəˈnɛtık əsoʊsiˈeıʃn
      Y [ˈʏpsilɔn], Yen [jɛn], Yoga [ˈjoːgɑ]

    APL:

      ((V⍳V)=⍳⍴V)/V←,V    ⌷←⍳→⍴∆∇⊃‾⍎⍕⌈

    Nicer typography in plain text files:

      ╔══════════════════════════════════════════╗
      ║                                          ║
      ║   • ‘single’ and “double” quotes         ║
      ║                                          ║
      ║   • Curly apostrophes: “We’ve been here” ║
      ║                                          ║
      ║   • Latin-1 apostrophe and accents: '´`  ║
      ║                                          ║
      ║   • ‚deutsche‘ „Anführungszeichen“       ║
      ║                                          ║
      ║   • †, ‡, ‰, •, 3–4, —, −5/+5, ™, …      ║
      ║                                          ║
      ║   • ASCII safety test: 1lI|, 0OD, 8B     ║
      ║                      ╭─────────╮         ║
      ║   • the euro symbol: │ 14.95 € │         ║
      ║                      ╰─────────╯         ║
      ╚══════════════════════════════════════════╝

    Greek (in Polytonic):

      The Greek anthem:

      Σὲ γνωρίζω ἀπὸ τὴν κόψη
      τοῦ σπαθιοῦ τὴν τρομερή,
      σὲ γνωρίζω ἀπὸ τὴν ὄψη
      ποὺ μὲ βία μετράει τὴ γῆ.

      ᾿Απ᾿ τὰ κόκκαλα βγαλμένη
      τῶν ῾Ελλήνων τὰ ἱερά
      καὶ σὰν πρῶτα ἀνδρειωμένη
      χαῖρε, ὦ χαῖρε, ᾿Ελευθεριά!

      From a speech of Demosthenes in the 4th century BC:

      Οὐχὶ ταὐτὰ παρίσταταί μοι γιγνώσκειν, ὦ ἄνδρες ᾿Αθηναῖοι,
      ὅταν τ᾿ εἰς τὰ πράγματα ἀποβλέψω καὶ ὅταν πρὸς τοὺς
      λόγους οὓς ἀκούω· τοὺς μὲν γὰρ λόγους περὶ τοῦ
      τιμωρήσασθαι Φίλιππον ὁρῶ γιγνομένους, τὰ δὲ πράγματ᾿
      εἰς τοῦτο προήκοντα,  ὥσθ᾿ ὅπως μὴ πεισόμεθ᾿ αὐτοὶ
      πρότερον κακῶς σκέψασθαι δέον. οὐδέν οὖν ἄλλο μοι δοκοῦσιν
      οἱ τὰ τοιαῦτα λέγοντες ἢ τὴν ὑπόθεσιν, περὶ ἧς βουλεύεσθαι,
      οὐχὶ τὴν οὖσαν παριστάντες ὑμῖν ἁμαρτάνειν. ἐγὼ δέ, ὅτι μέν
      ποτ᾿ ἐξῆν τῇ πόλει καὶ τὰ αὑτῆς ἔχειν ἀσφαλῶς καὶ Φίλιππον
      τιμωρήσασθαι, καὶ μάλ᾿ ἀκριβῶς οἶδα· ἐπ᾿ ἐμοῦ γάρ, οὐ πάλαι
      γέγονεν ταῦτ᾿ ἀμφότερα· νῦν μέντοι πέπεισμαι τοῦθ᾿ ἱκανὸν
      προλαβεῖν ἡμῖν εἶναι τὴν πρώτην, ὅπως τοὺς συμμάχους
      σώσομεν. ἐὰν γὰρ τοῦτο βεβαίως ὑπάρξῃ, τότε καὶ περὶ τοῦ
      τίνα τιμωρήσεταί τις καὶ ὃν τρόπον ἐξέσται σκοπεῖν· πρὶν δὲ
      τὴν ἀρχὴν ὀρθῶς ὑποθέσθαι, μάταιον ἡγοῦμαι περὶ τῆς
      τελευτῆς ὁντινοῦν ποιεῖσθαι λόγον.

      Δημοσθένους, Γ´ ᾿Ολυνθιακὸς

    Georgian:

      From a Unicode conference invitation:

      გთხოვთ ახლავე გაიაროთ რეგისტრაცია Unicode-ის მეათე საერთაშორისო
      კონფერენციაზე დასასწრებად, რომელიც გაიმართება 10-12 მარტს,
      ქ. მაინცში, გერმანიაში. კონფერენცია შეჰკრებს ერთად მსოფლიოს
      ექსპერტებს ისეთ დარგებში როგორიცაა ინტერნეტი და Unicode-ი,
      ინტერნაციონალიზაცია და ლოკალიზაცია, Unicode-ის გამოყენება
      ოპერაციულ სისტემებსა, და გამოყენებით პროგრამებში, შრიფტებში,
      ტექსტების დამუშავებასა და მრავალენოვან კომპიუტერულ სისტემებში.

    Russian:

      From a Unicode conference invitation:

      Зарегистрируйтесь сейчас на Десятую Международную Конференцию по
      Unicode, которая состоится 10-12 марта 1997 года в Майнце в Германии.
      Конференция соберет широкий круг экспертов по  вопросам глобального
      Интернета и Unicode, локализации и интернационализации, воплощению и
      применению Unicode в различных операционных системах и программных
      приложениях, шрифтах, верстке и многоязычных компьютерных системах.

    Thai (UCS Level 2):

      Excerpt from a poetry on The Romance of The Three Kingdoms (a Chinese
      classic 'San Gua'):

      [----------------------------|------------------------]
        ๏ แผ่นดินฮั่นเสื่อมโทรมแสนสังเวช  พระปกเกศกองบู๊กู้ขึ้นใหม่
      สิบสองกษัตริย์ก่อนหน้าแลถัดไป       สององค์ไซร้โง่เขลาเบาปัญญา
        ทรงนับถือขันทีเป็นที่พึ่ง           บ้านเมืองจึงวิปริตเป็นนักหนา
      โฮจิ๋นเรียกทัพทั่วหัวเมืองมา         หมายจะฆ่ามดชั่วตัวสำคัญ
        เหมือนขับไสไล่เสือจากเคหา      รับหมาป่าเข้ามาเลยอาสัญ
      ฝ่ายอ้องอุ้นยุแยกให้แตกกัน          ใช้สาวนั้นเป็นชนวนชื่นชวนใจ
        พลันลิฉุยกุยกีกลับก่อเหตุ          ช่างอาเพศจริงหนาฟ้าร้องไห้
      ต้องรบราฆ่าฟันจนบรรลัย           ฤๅหาใครค้ำชูกู้บรรลังก์ ฯ

      (The above is a two-column text. If combining characters are handled
      correctly, the lines of the second column should be aligned with the
      | character above.)

    Ethiopian:

      Proverbs in the Amharic language:

      ሰማይ አይታረስ ንጉሥ አይከሰስ።
      ብላ ካለኝ እንደአባቴ በቆመጠኝ።
      ጌጥ ያለቤቱ ቁምጥና ነው።
      ደሀ በሕልሙ ቅቤ ባይጠጣ ንጣት በገደለው።
      የአፍ ወለምታ በቅቤ አይታሽም።
      አይጥ በበላ ዳዋ ተመታ።
      ሲተረጉሙ ይደረግሙ።
      ቀስ በቀስ፥ ዕንቁላል በእግሩ ይሄዳል።
      ድር ቢያብር አንበሳ ያስር።
      ሰው እንደቤቱ እንጅ እንደ ጉረቤቱ አይተዳደርም።
      እግዜር የከፈተውን ጉሮሮ ሳይዘጋው አይድርም።
      የጎረቤት ሌባ፥ ቢያዩት ይስቅ ባያዩት ያጠልቅ።
      ሥራ ከመፍታት ልጄን ላፋታት።
      ዓባይ ማደሪያ የለው፥ ግንድ ይዞ ይዞራል።
      የእስላም አገሩ መካ የአሞራ አገሩ ዋርካ።
      ተንጋሎ ቢተፉ ተመልሶ ባፉ።
      ወዳጅህ ማር ቢሆን ጨርስህ አትላሰው።
      እግርህን በፍራሽህ ልክ ዘርጋ።

    Runes:

      ᚻᛖ ᚳᚹᚫᚦ ᚦᚫᛏ ᚻᛖ ᛒᚢᛞᛖ ᚩᚾ ᚦᚫᛗ ᛚᚪᚾᛞᛖ ᚾᚩᚱᚦᚹᛖᚪᚱᛞᚢᛗ ᚹᛁᚦ ᚦᚪ ᚹᛖᛥᚫ

      (Old English, which transcribed into Latin reads 'He cwaeth that he
      bude thaem lande northweardum with tha Westsae.' and means 'He said
      that he lived in the northern land near the Western Sea.')

    Braille:

      ⡌⠁⠧⠑ ⠼⠁⠒  ⡍⠜⠇⠑⠹⠰⠎ ⡣⠕⠌

      ⡍⠜⠇⠑⠹ ⠺⠁⠎ ⠙⠑⠁⠙⠒ ⠞⠕ ⠃⠑⠛⠔ ⠺⠊⠹⠲ ⡹⠻⠑ ⠊⠎ ⠝⠕ ⠙⠳⠃⠞
      ⠱⠁⠞⠑⠧⠻ ⠁⠃⠳⠞ ⠹⠁⠞⠲ ⡹⠑ ⠗⠑⠛⠊⠌⠻ ⠕⠋ ⠙⠊⠎ ⠃⠥⠗⠊⠁⠇ ⠺⠁⠎
      ⠎⠊⠛⠝⠫ ⠃⠹ ⠹⠑ ⠊⠇⠻⠛⠹⠍⠁⠝⠂ ⠹⠑ ⠊⠇⠻⠅⠂ ⠹⠑ ⠥⠝⠙⠻⠞⠁⠅⠻⠂
      ⠁⠝⠙ ⠹⠑ ⠡⠊⠑⠋ ⠍⠳⠗⠝⠻⠲ ⡎⠊⠗⠕⠕⠛⠑ ⠎⠊⠛⠝⠫ ⠊⠞⠲ ⡁⠝⠙
      ⡎⠊⠗⠕⠕⠛⠑⠰⠎ ⠝⠁⠍⠑ ⠺⠁⠎ ⠛⠕⠕⠙ ⠥⠏⠕⠝ ⠰⡡⠁⠝⠛⠑⠂ ⠋⠕⠗ ⠁⠝⠹⠹⠔⠛ ⠙⠑
      ⠡⠕⠎⠑ ⠞⠕ ⠏⠥⠞ ⠙⠊⠎ ⠙⠁⠝⠙ ⠞⠕⠲

      ⡕⠇⠙ ⡍⠜⠇⠑⠹ ⠺⠁⠎ ⠁⠎ ⠙⠑⠁⠙ ⠁⠎ ⠁ ⠙⠕⠕⠗⠤⠝⠁⠊⠇⠲

      ⡍⠔⠙⠖ ⡊ ⠙⠕⠝⠰⠞ ⠍⠑⠁⠝ ⠞⠕ ⠎⠁⠹ ⠹⠁⠞ ⡊ ⠅⠝⠪⠂ ⠕⠋ ⠍⠹
      ⠪⠝ ⠅⠝⠪⠇⠫⠛⠑⠂ ⠱⠁⠞ ⠹⠻⠑ ⠊⠎ ⠏⠜⠞⠊⠊⠥⠇⠜⠇⠹ ⠙⠑⠁⠙ ⠁⠃⠳⠞
      ⠁ ⠙⠕⠕⠗⠤⠝⠁⠊⠇⠲ ⡊ ⠍⠊⠣⠞ ⠙⠁⠧⠑ ⠃⠑⠲ ⠔⠊⠇⠔⠫⠂ ⠍⠹⠎⠑⠇⠋⠂ ⠞⠕
      ⠗⠑⠛⠜⠙ ⠁ ⠊⠕⠋⠋⠔⠤⠝⠁⠊⠇ ⠁⠎ ⠹⠑ ⠙⠑⠁⠙⠑⠌ ⠏⠊⠑⠊⠑ ⠕⠋ ⠊⠗⠕⠝⠍⠕⠝⠛⠻⠹
      ⠔ ⠹⠑ ⠞⠗⠁⠙⠑⠲ ⡃⠥⠞ ⠹⠑ ⠺⠊⠎⠙⠕⠍ ⠕⠋ ⠳⠗ ⠁⠝⠊⠑⠌⠕⠗⠎
      ⠊⠎ ⠔ ⠹⠑ ⠎⠊⠍⠊⠇⠑⠆ ⠁⠝⠙ ⠍⠹ ⠥⠝⠙⠁⠇⠇⠪⠫ ⠙⠁⠝⠙⠎
      ⠩⠁⠇⠇ ⠝⠕⠞ ⠙⠊⠌⠥⠗⠃ ⠊⠞⠂ ⠕⠗ ⠹⠑ ⡊⠳⠝⠞⠗⠹⠰⠎ ⠙⠕⠝⠑ ⠋⠕⠗⠲ ⡹⠳
      ⠺⠊⠇⠇ ⠹⠻⠑⠋⠕⠗⠑ ⠏⠻⠍⠊⠞ ⠍⠑ ⠞⠕ ⠗⠑⠏⠑⠁⠞⠂ ⠑⠍⠏⠙⠁⠞⠊⠊⠁⠇⠇⠹⠂ ⠹⠁⠞
      ⡍⠜⠇⠑⠹ ⠺⠁⠎ ⠁⠎ ⠙⠑⠁⠙ ⠁⠎ ⠁ ⠙⠕⠕⠗⠤⠝⠁⠊⠇⠲

      (The first couple of paragraphs of \"A Christmas Carol\" by Dickens)

    Compact font selection example text:

      ABCDEFGHIJKLMNOPQRSTUVWXYZ /0123456789
      abcdefghijklmnopqrstuvwxyz £©µÀÆÖÞßéöÿ
      –—‘“”„†•…‰™œŠŸž€ ΑΒΓΔΩαβγδω АБВГДабвгд
      ∀∂∈ℝ∧∪≡∞ ↑↗↨↻⇣ ┐┼╔╘░►☺♀ ﬁ�⑀₂ἠḂӥẄɐː⍎אԱა

    Greetings in various languages:

      Hello world, Καλημέρα κόσμε, コンニチハ

    Box drawing alignment tests:                                          █
                                                                          ▉
      ╔══╦══╗  ┌──┬──┐  ╭──┬──╮  ╭──┬──╮  ┏━━┳━━┓  ┎┒┏┑   ╷  ╻ ┏┯┓ ┌┰┐    ▊ ╱╲╱╲╳╳╳
      ║┌─╨─┐║  │╔═╧═╗│  │╒═╪═╕│  │╓─╁─╖│  ┃┌─╂─┐┃  ┗╃╄┙  ╶┼╴╺╋╸┠┼┨ ┝╋┥    ▋ ╲╱╲╱╳╳╳
      ║│╲ ╱│║  │║   ║│  ││ │ ││  │║ ┃ ║│  ┃│ ╿ │┃  ┍╅╆┓   ╵  ╹ ┗┷┛ └┸┘    ▌ ╱╲╱╲╳╳╳
      ╠╡ ╳ ╞╣  ├╢   ╟┤  ├┼─┼─┼┤  ├╫─╂─╫┤  ┣┿╾┼╼┿┫  ┕┛┖┚     ┌┄┄┐ ╎ ┏┅┅┓ ┋ ▍ ╲╱╲╱╳╳╳
      ║│╱ ╲│║  │║   ║│  ││ │ ││  │║ ┃ ║│  ┃│ ╽ │┃  ░░▒▒▓▓██ ┊  ┆ ╎ ╏  ┇ ┋ ▎
      ║└─╥─┘║  │╚═╤═╝│  │╘═╪═╛│  │╙─╀─╜│  ┃└─╂─┘┃  ░░▒▒▓▓██ ┊  ┆ ╎ ╏  ┇ ┋ ▏
      ╚══╩══╝  └──┴──┘  ╰──┴──╯  ╰──┴──╯  ┗━━┻━━┛           └╌╌┘ ╎ ┗╍╍┛ ┋  ▁▂▃▄▅▆▇█

",
    );

    match CharBuffer::try_from(string.as_str()) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    match CharBuffer::try_from(string.as_mut_str()) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    match CharBuffer::try_from(&string) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    match CharBuffer::try_from(&mut string) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    match CharBuffer::try_from(string.clone()) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    let mut some_box: Box<str> = Box::from(string.clone());

    match CharBuffer::try_from(&some_box) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    match CharBuffer::try_from(&mut some_box) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    match CharBuffer::try_from(some_box) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    let mut some_arc: Arc<str> = Arc::from(string.clone());

    match CharBuffer::try_from(&some_arc) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    match CharBuffer::try_from(&mut some_arc) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    match CharBuffer::try_from(some_arc) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    let mut some_rc: Rc<str> = Rc::from(string.clone());

    match CharBuffer::try_from(&some_rc) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    match CharBuffer::try_from(&mut some_rc) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    match CharBuffer::try_from(some_rc) {
        Ok(_) => panic!("This should fail because of long string"),
        Err(_) => {}
    }

    let hash_builder = std::collections::hash_map::RandomState::default();

    fn make_hash<Q, S>(hash_builder: &S, val: &Q) -> u64
    where
        Q: std::hash::Hash + ?Sized,
        S: core::hash::BuildHasher,
    {
        hash_builder.hash_one(val)
    }

    for mut char in string.chars() {
        let mut char_string = char.to_string();

        assert_eq!(CharBuffer::new(char), char_string);
        assert_eq!(CharBuffer::new(char).as_str(), char_string);
        assert_eq!(CharBuffer::new(char).len(), char_string.len());
        assert_eq!(CharBuffer::from(char), char_string);
        assert_eq!(CharBuffer::from(&char), char_string);
        assert_eq!(CharBuffer::from(&mut char), char_string);

        let char_buf = CharBuffer::new(char);

        assert_eq!(
            make_hash(&hash_builder, &char_buf),
            make_hash(&hash_builder, &char_string)
        );

        assert_eq!(CharBuffer::new(char), char_buf);
        assert_eq!(CharBuffer::new(char), CharBuffer::from(&char_buf));
        assert_eq!(&*char_buf, char_string.as_str());
        assert_eq!(char_buf.as_ref(), char_string.as_str());
        let str: &str = char_buf.borrow();
        assert_eq!(str, char_string.as_str());

        assert_eq!(char::from(&char_buf), char);
        assert_eq!(char::from(char_buf), char);
        assert_eq!(String::from(char_buf), char_string);
        assert_eq!(String::from(&char_buf), char_string);

        let str: &str = From::from(&char_buf);
        assert_eq!(str, char_string);

        let str: Cow<str> = From::from(&char_buf);
        assert_eq!(str, char_string);

        let str: Cow<CharBuffer> = From::from(char_buf);
        assert_eq!(str.as_str(), char_string);

        let str: Arc<str> = From::from(char_buf);
        assert_eq!(&str[..], char_string);

        let str: Arc<str> = From::from(&char_buf);
        assert_eq!(&str[..], char_string);

        let str: Box<str> = From::from(char_buf);
        assert_eq!(&str[..], char_string);

        let str: Box<str> = From::from(&char_buf);
        assert_eq!(&str[..], char_string);

        let str: Rc<str> = From::from(char_buf);
        assert_eq!(&str[..], char_string);

        let str: Rc<str> = From::from(&char_buf);
        assert_eq!(&str[..], char_string);

        match CharBuffer::try_from(char_string.as_str()) {
            Ok(char_buf) => {
                assert_eq!(char_buf, char_string.as_str());
                assert_eq!(char_string.as_str(), char_buf);
            }
            Err(_) => panic!("This should not fail because of single char"),
        }
        match CharBuffer::try_from(char_string.as_mut_str()) {
            Ok(char_buf) => {
                assert_eq!(char_buf, char_string.as_mut_str());
                assert_eq!(char_string.as_mut_str(), char_buf);
            }
            Err(_) => panic!("This should not fail because of single char"),
        }

        match CharBuffer::try_from(&char_string) {
            Ok(char_buf) => {
                assert_eq!(char_buf, &char_string);
                assert_eq!(&char_string, char_buf);
            }
            Err(_) => panic!("This should not fail because of single char"),
        }
        match CharBuffer::try_from(&mut char_string) {
            Ok(char_buf) => {
                assert_eq!(char_buf, &mut char_string);
                assert_eq!(&mut char_string, char_buf);
            }
            Err(_) => panic!("This should not fail because of single char"),
        }
        match CharBuffer::try_from(char_string.clone()) {
            Ok(char_buf) => {
                assert_eq!(char_buf, char_string);
                assert_eq!(char_string, char_buf);
            }
            Err(_) => panic!("This should not fail because of single char"),
        }

        let mut some_box: Box<str> = Box::from(char_string.clone());

        match CharBuffer::try_from(&some_box) {
            Ok(char_buf) => assert_eq!(char_buf, some_box.as_ref()),
            Err(_) => panic!("This should not fail because of single char"),
        }
        match CharBuffer::try_from(&mut some_box) {
            Ok(char_buf) => assert_eq!(char_buf, some_box.as_ref()),
            Err(_) => panic!("This should not fail because of single char"),
        }
        match CharBuffer::try_from(some_box) {
            Ok(char_buf) => assert_eq!(char_buf, char_string),
            Err(_) => panic!("This should not fail because of single char"),
        }

        let mut some_arc: Arc<str> = Arc::from(char_string.clone());

        match CharBuffer::try_from(&some_arc) {
            Ok(char_buf) => assert_eq!(char_buf, some_arc.as_ref()),
            Err(_) => panic!("This should not fail because of single char"),
        }
        match CharBuffer::try_from(&mut some_arc) {
            Ok(char_buf) => assert_eq!(char_buf, some_arc.as_ref()),
            Err(_) => panic!("This should not fail because of single char"),
        }
        match CharBuffer::try_from(some_arc) {
            Ok(char_buf) => assert_eq!(char_buf, char_string),
            Err(_) => panic!("This should not fail because of single char"),
        }

        let mut some_rc: Rc<str> = Rc::from(char_string.clone());

        match CharBuffer::try_from(&some_rc) {
            Ok(char_buf) => assert_eq!(char_buf, some_rc.as_ref()),
            Err(_) => panic!("This should not fail because of single char"),
        }
        match CharBuffer::try_from(&mut some_rc) {
            Ok(char_buf) => assert_eq!(char_buf, some_rc.as_ref()),
            Err(_) => panic!("This should not fail because of single char"),
        }
        match CharBuffer::try_from(some_rc) {
            Ok(char_buf) => assert_eq!(char_buf, char_string),
            Err(_) => panic!("This should not fail because of single char"),
        }
    }
}
