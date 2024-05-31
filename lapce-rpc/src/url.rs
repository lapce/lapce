// use std::iter::FusedIterator;

// #[derive(Debug, Clone)]
// #[repr(transparent)]
// pub struct Url {
//     pub(crate) inner: url::Url,
// }

// impl Url {
//     pub fn parent(&mut self) -> Option<&mut Self> {
//         self.inner.path_segments_mut().ok()?.pop();
//         Some(self)
//     }

//     pub fn ancestors(&self) -> Ancestors {
//         Ancestors { next: Some(self) }
//     }
// }

// #[derive(Debug)]
// #[must_use = "iterators are lazy and do nothing unless consumed"]
// pub struct Ancestors<'a> {
//     next: Option<&'a mut Url>,
// }

// impl<'a> Iterator for Ancestors<'a> {
//     type Item = &'a mut Url;

//     #[inline]
//     fn next(&mut self) -> std::option::Option<Self::Item> {
//         let next = &self.next;
//         self.next = next.and_then(self::Url::parent);
//         next.as_deref()
//     }
// }

// impl FusedIterator for Ancestors<'_> {}
