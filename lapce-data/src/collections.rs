use core::borrow::Borrow;
use core::hash::Hash;
use core::iter::{Flatten, Map};
use core::slice;
use hashbrown;
use indexmap::{self, IndexMap};
use std::collections;

/// Key equivalence trait.
///
/// This trait allows hash table lookup to be customized.
/// It has one blanket implementation that uses the regular `Borrow` solution,
/// just like `HashMap` and `BTreeMap` do, so that you can pass `&str` to lookup
/// into a map with `String` keys and so on.
///
/// # Correctness
///
/// Equivalent values must hash to the same value.
pub trait Equivalent<K: ?Sized>: hashbrown::Equivalent<K> + indexmap::Equivalent<K> {
    /// Checks if this value is equivalent to the given key.
    ///
    /// Returns `true` if both values are equivalent, and `false` otherwise.
    ///
    /// # Correctness
    ///
    /// When this function returns `true`, both `self` and `key` must hash to
    /// the same value.
    fn equivalent(&self, key: &K) -> bool;
}

impl<Q: ?Sized, K: ?Sized> Equivalent<K> for Q
where
    Q: Eq,
    K: Borrow<Q>,
{
    #[inline]
    fn equivalent(&self, key: &K) -> bool {
        *self == *key.borrow()
    }
}

pub trait AllContainerRef<E: ?Sized + Equivalent<Self::Key>> {
    type Key;
    type Value;
    type Keys<'a>: Iterator<Item = &'a Self::Key>
    where
        Self: 'a,
        Self::Key: 'a,
        Self::Value: 'a;

    type Values<'a>: Iterator<Item = &'a Self::Value>
    where
        Self: 'a,
        Self::Key: 'a,
        Self::Value: 'a;

    fn len(&self) -> usize;
    fn contains_key(&self, key: &E) -> bool;
    fn get_with(&self, key: &E) -> Option<&Self::Value>;
    fn get_equivalent<'a, E2>(&'a self, key: &E) -> Option<E2>
    where
        E2: Equivalent<Self::Value> + From<&'a Self::Value>,
        Self::Value: 'a,
    {
        if let Some(value) = self.get_with(key) {
            return Some(value.into());
        }
        None
    }

    fn keys<'a>(&'a self) -> Self::Keys<'a>;

    fn values<'a>(&'a self) -> Self::Values<'a>;
}

pub trait MapContainerRef<E: ?Sized + Equivalent<Self::Key>>: AllContainerRef<E> {
    fn get_key_value(&self, k: &E) -> Option<(&Self::Key, &Self::Value)>;
}

pub trait SetContainerRef<E: ?Sized + Equivalent<Self::Key>>: AllContainerRef<E> {
    type Difference<'a>
    where
        Self: 'a,
        Self::Key: 'a;
    type Intersection<'a>
    where
        Self: 'a,
        Self::Key: 'a;
    type SymmetricDifference<'a>
    where
        Self: 'a,
        Self::Key: 'a;
    fn difference<'a>(&'a self, other: &'a Self) -> Self::Difference<'a>;
    fn intersection<'a>(&'a self, other: &'a Self) -> Self::Intersection<'a>;
    fn symmetric_difference<'a>(&'a self, other: &'a Self) -> Self::SymmetricDifference<'a>;
    fn is_disjoint(&self, other: &Self) -> bool;
    fn is_subset(&self, other: &Self) -> bool;
}

pub trait VecContainerRef<E: ?Sized + Equivalent<Self::Key>>: AllContainerRef<E> {
    type Slice;
    fn as_slice(&self) -> &[Self::Slice];
}

pub trait SomeContainerMut<E: ?Sized + Equivalent<Self::Key>>: AllContainerRef<E> {
    type DrainValues<'a>: Iterator<Item = Self::Value>
    where
        Self: 'a;
    type ValuesMut<'a>: Iterator<Item = &'a mut Self::Value>
    where
        Self: 'a,
        Self::Key: 'a,
        Self::Value: 'a;

    fn get_mut_with(&mut self, key: &E) -> Option<&mut Self::Value>;
    fn clear(&mut self);
    fn reserve(&mut self, additional: usize);
    fn shrink_to(&mut self, min_capacity: usize);
    fn shrink_to_fit(&mut self);
    fn values_mut<'a>(&'a mut self) -> Self::ValuesMut<'a>;
    fn drain_values<'a>(&'a mut self) -> Self::DrainValues<'a>;
}

pub trait MapContainerMut<E: ?Sized + Equivalent<Self::Key>>:
    AllContainerRef<E> + MapContainerRef<E> + SomeContainerMut<E>
{
    type Drain<'a>: Iterator<Item = (Self::Key, Self::Value)>
    where
        Self: 'a;
    fn insert(&mut self, k: Self::Key, v: Self::Value) -> Option<Self::Value>;
    fn remove(&mut self, key: &E) -> Option<Self::Value>;
    fn remove_entry(&mut self, k: &E) -> Option<(Self::Key, Self::Value)>;
    fn drain<'a>(&'a mut self) -> Self::Drain<'a>;
}

pub trait SetContainerMut<E: ?Sized + Equivalent<Self::Key>>:
    AllContainerRef<E> + SetContainerRef<E>
{
    type Drain<'a>: Iterator<Item = Self::Key>
    where
        Self: 'a;
    fn reserve(&mut self, additional: usize);
    fn shrink_to(&mut self, min_capacity: usize);
    fn shrink_to_fit(&mut self);
    fn insert(&mut self, value: Self::Key) -> bool;
    fn remove(&mut self, key: &E) -> Option<Self::Key>;
    fn replace(&mut self, value: Self::Key) -> Option<Self::Key>;
    fn clear(&mut self);
    fn drain<'a>(&'a mut self) -> Self::Drain<'a>;
}

pub trait VecContainerMut<E: ?Sized + Equivalent<Self::Key>>:
    AllContainerRef<E> + VecContainerRef<E> + SomeContainerMut<E>
{
    type Drain<'a>: Iterator<Item = Self::Value>
    where
        Self: 'a;
    fn insert(&mut self, index: usize, element: Self::Value);
    fn push(&mut self, value: Self::Value);
    fn pop(&mut self) -> Option<Self::Value>;
    fn remove(&mut self, index: usize) -> Self::Value;
    fn replace(&mut self, value: Self::Key) -> Option<Self::Key>;
    fn drain<'a>(&'a mut self) -> Self::Drain<'a>;
}

pub trait AllContainer<E: ?Sized + Equivalent<Self::Key>>: AllContainerRef<E> {
    type IntoKeys: Iterator<Item = Self::Key>;
    type IntoValues: Iterator<Item = Self::Value>;
    fn into_keys(self) -> Self::IntoKeys;
    fn into_values(self) -> Self::IntoValues;
}

pub trait SetContainer<E: ?Sized + Equivalent<Self::Key>>:
    AllContainerRef<E> + SetContainerRef<E> + SetContainerMut<E> + AllContainer<E>
{
    // type Drain<'a>: Iterator<Item = Self::Key>
    // where
    //     Self: 'a;
    // fn drain<'a>(&'a mut self) -> Self::Drain<'a>;
}

pub trait VecContainer<E: ?Sized + Equivalent<Self::Key>>: AllContainerRef<E> {
    type IntoKeys: Iterator<Item = Self::Key>;
    type IntoValues: Iterator<Item = Self::Value>;

    fn insert(&mut self, k: Self::Key, v: Self::Value) -> Option<Self::Value>;
    fn remove(&mut self, k: &E) -> Option<Self::Value>;
    fn into_keys(self) -> Self::IntoKeys;
    fn into_values(self) -> Self::IntoValues;
}

impl<E: Equivalent<T::Key>, T: AllContainerRef<E>> AllContainerRef<E> for &T {
    type Key = T::Key;
    type Value = T::Value;

    type Keys<'a> = T::Keys<'a>
    where
        Self: 'a,
        Self::Key: 'a,
        Self::Value: 'a;

    type Values<'a> = T::Values<'a>
    where
        Self: 'a,
        Self::Key: 'a,
        Self::Value: 'a;

    fn len(&self) -> usize {
        <T as AllContainerRef<E>>::len(self)
    }

    fn contains_key(&self, key: &E) -> bool {
        <T as AllContainerRef<E>>::contains_key(self, key)
    }

    fn get_with(&self, key: &E) -> Option<&Self::Value> {
        <T as AllContainerRef<E>>::get_with(self, key)
    }

    fn keys<'a>(&'a self) -> Self::Keys<'a> {
        <T as AllContainerRef<E>>::keys(self)
    }

    fn values<'a>(&'a self) -> Self::Values<'a> {
        <T as AllContainerRef<E>>::values(self)
    }
}

impl<E: Equivalent<T::Key>, T: AllContainerRef<E>> AllContainerRef<E> for &mut T {
    type Key = T::Key;
    type Value = T::Value;

    type Keys<'a> = T::Keys<'a>
    where
        Self: 'a,
        Self::Key: 'a,
        Self::Value: 'a;

    type Values<'a> = T::Values<'a>
    where
        Self: 'a,
        Self::Key: 'a,
        Self::Value: 'a;

    fn len(&self) -> usize {
        <T as AllContainerRef<E>>::len(self)
    }

    fn contains_key(&self, key: &E) -> bool {
        <T as AllContainerRef<E>>::contains_key(self, key)
    }

    fn get_with(&self, key: &E) -> Option<&Self::Value> {
        <T as AllContainerRef<E>>::get_with(self, key)
    }

    fn keys<'a>(&'a self) -> Self::Keys<'a> {
        <T as AllContainerRef<E>>::keys(self)
    }

    fn values<'a>(&'a self) -> Self::Values<'a> {
        <T as AllContainerRef<E>>::values(self)
    }
}

impl<E: Equivalent<T::Key>, T: SomeContainerMut<E>> SomeContainerMut<E> for &mut T {
    type ValuesMut<'a> = T::ValuesMut<'a>
    where
        Self: 'a,
        Self::Key: 'a,
        Self::Value: 'a;

    fn get_mut_with(&mut self, key: &E) -> Option<&mut Self::Value> {
        <T as SomeContainerMut<E>>::get_mut_with(self, key)
    }

    fn values_mut<'a>(&'a mut self) -> Self::ValuesMut<'a> {
        <T as SomeContainerMut<E>>::values_mut(self)
    }
}

impl<T: Eq, E: Equivalent<T>> AllContainerRef<E> for [T] {
    type Key = T;
    type Value = T;
    type Keys<'a> = slice::Iter<'a, T> where Self:'a, T: 'a;
    type Values<'a> = slice::Iter<'a, T> where Self:'a, T: 'a;

    fn len(&self) -> usize {
        self.len()
    }

    fn contains_key(&self, key: &E) -> bool {
        self.iter().any(|x| Equivalent::equivalent(key, x))
    }

    fn get_with(&self, key: &E) -> Option<&Self::Value> {
        self.iter().find(|x| Equivalent::equivalent(key, *x))
    }

    fn keys<'a>(&'a self) -> Self::Keys<'a> {
        self.iter()
    }

    fn values<'a>(&'a self) -> Self::Values<'a> {
        self.iter()
    }
}

impl<T: Eq, E: Equivalent<T>> AllContainerRef<E> for Vec<T> {
    type Key = T;
    type Value = T;
    type Keys<'a>  = slice::Iter<'a,T>where Self:'a,T:'a;
    type Values<'a>  = slice::Iter<'a,T>where Self:'a,T:'a;
    fn len(&self) -> usize {
        self.len()
    }
    fn contains_key(&self, key: &E) -> bool {
        self.as_slice().contains_key(key)
    }
    fn get_with(&self, key: &E) -> Option<&Self::Value> {
        self.as_slice().get_with(key)
    }
    fn keys<'a>(&'a self) -> Self::Keys<'a> {
        self.iter()
    }
    fn values<'a>(&'a self) -> Self::Values<'a> {
        self.iter()
    }
}

impl<K, V, Q> AllContainerRef<Q> for collections::HashMap<K, V>
where
    K: Hash + Eq + Borrow<Q>,
    Q: Hash + Eq,
{
    type Key = K;
    type Value = V;
    type Keys<'a> = collections::hash_map::Keys<'a, K, V> where Self: 'a, K: 'a, V: 'a;
    type Values<'a> = collections::hash_map::Values<'a, K, V> where Self: 'a, K: 'a, V: 'a;

    fn len(&self) -> usize {
        self.len()
    }

    fn contains_key(&self, eq: &Q) -> bool {
        self.contains_key(eq)
    }

    fn get_with(&self, key: &Q) -> Option<&Self::Value> {
        self.get(key)
    }

    // fn get_mut_with(&mut self, key: &Q) -> Option<&mut Self::Value> {
    //     self.get_mut(key)
    // }

    fn keys<'a>(&'a self) -> Self::Keys<'a> {
        self.keys()
    }

    fn values<'a>(&'a self) -> Self::Values<'a> {
        self.values()
    }
}

impl<K, V, Q> AllContainerRef<Q> for hashbrown::HashMap<K, V>
where
    K: Hash + Eq,
    Q: Hash + Equivalent<K>,
{
    type Key = K;
    type Value = V;
    type Keys<'a> = hashbrown::hash_map::Keys<'a, K, V> where Self: 'a, K: 'a, V: 'a;
    type Values<'a> = hashbrown::hash_map::Values<'a, K, V> where Self: 'a, K: 'a, V: 'a;

    fn len(&self) -> usize {
        self.len()
    }

    fn contains_key(&self, eq: &Q) -> bool {
        self.contains_key(eq)
    }

    fn get_with(&self, key: &Q) -> Option<&Self::Value> {
        self.get(key)
    }

    // fn get_mut_with(&mut self, key: &Q) -> Option<&mut Self::Value> {
    //     self.get_mut(key)
    // }

    fn keys<'a>(&'a self) -> Self::Keys<'a> {
        self.keys()
    }

    fn values<'a>(&'a self) -> Self::Values<'a> {
        self.values()
    }
}

impl<K, V, Q> AllContainerRef<Q> for IndexMap<K, V>
where
    K: Hash + Eq,
    Q: Hash + Equivalent<K>,
{
    type Key = K;
    type Value = V;
    type Keys<'a> = indexmap::map::Keys<'a, K, V> where Self: 'a, K: 'a, V: 'a;
    type Values<'a> = indexmap::map::Values<'a, K, V> where Self: 'a, K: 'a, V: 'a;

    fn len(&self) -> usize {
        self.len()
    }

    fn contains_key(&self, eq: &Q) -> bool {
        self.contains_key(eq)
    }

    fn get_with(&self, key: &Q) -> Option<&Self::Value> {
        self.get(key)
    }

    fn keys<'a>(&'a self) -> Self::Keys<'a> {
        self.keys()
    }

    fn values<'a>(&'a self) -> Self::Values<'a> {
        self.values()
    }
}

impl<E: Equivalent<T::Key>, T: AllContainerRef<E>, const N: usize> AllContainerRef<E> for [T; N] {
    type Key = T::Key;
    type Value = T::Value;

    type Keys<'a> = Flatten<Map<slice::Iter<'a, T>, fn(&'a T) -> T::Keys<'a>>>
    where
        Self: 'a,
        Self::Key: 'a,
        Self::Value: 'a;

    type Values<'a> = Flatten<Map<slice::Iter<'a, T>, fn(&'a T) -> T::Values<'a>>>
    where
        Self: 'a,
        Self::Key: 'a,
        Self::Value: 'a;

    fn len(&self) -> usize {
        self.iter().map(|x| x.len()).sum()
    }

    fn contains_key(&self, key: &E) -> bool {
        self.iter().any(|x| x.contains_key(key))
    }

    fn get_with(&self, key: &E) -> Option<&Self::Value> {
        if let Some(container) = self.iter().find(|x| x.contains_key(key)) {
            return container.get_with(key);
        }
        None
    }

    fn keys<'a>(&'a self) -> Self::Keys<'a> {
        fn keys_iter<'a, E1: Equivalent<T1::Key>, T1: AllContainerRef<E1>>(
            item: &'a T1,
        ) -> T1::Keys<'a> {
            item.keys()
        }
        self.iter()
            .map(keys_iter as fn(&'a T) -> T::Keys<'a>)
            .flatten()
    }

    fn values<'a>(&'a self) -> Self::Values<'a> {
        fn values_iter<'a, E1: Equivalent<T1::Key>, T1: AllContainerRef<E1>>(
            item: &'a T1,
        ) -> T1::Values<'a> {
            item.values()
        }
        self.iter()
            .map(values_iter as fn(&'a T) -> T::Values<'a>)
            .flatten()
    }
}

impl<T: Eq, E: Equivalent<T>> SomeContainerMut<E> for [T] {
    type ValuesMut<'a> = slice::IterMut<'a,T> where Self:'a, T:'a;

    fn get_mut_with(&mut self, key: &E) -> Option<&mut Self::Value> {
        self.iter_mut().find(|x| Equivalent::equivalent(key, *x))
    }

    fn values_mut<'a>(&'a mut self) -> Self::ValuesMut<'a> {
        self.iter_mut()
    }
}

impl<T: Eq, E: Equivalent<T>> SomeContainerMut<E> for Vec<T> {
    type ValuesMut<'a>  = slice::IterMut<'a,T> where Self:'a,T:'a;

    fn get_mut_with(&mut self, key: &E) -> Option<&mut Self::Value> {
        self.iter_mut().find(|x| Equivalent::equivalent(key, *x))
    }

    fn values_mut<'a>(&'a mut self) -> Self::ValuesMut<'a> {
        self.iter_mut()
    }
}

impl<K, V, Q> SomeContainerMut<Q> for collections::HashMap<K, V>
where
    K: Hash + Eq + Borrow<Q>,
    Q: Hash + Eq,
{
    type ValuesMut<'a> = collections::hash_map::ValuesMut<'a, K, V> where Self: 'a, K: 'a, V: 'a;

    fn get_mut_with(&mut self, key: &Q) -> Option<&mut Self::Value> {
        self.get_mut(key)
    }

    fn values_mut<'a>(&'a mut self) -> Self::ValuesMut<'a> {
        self.values_mut()
    }
}

impl<K, V, Q> SomeContainerMut<Q> for hashbrown::HashMap<K, V>
where
    K: Hash + Eq,
    Q: Hash + Equivalent<K>,
{
    type ValuesMut<'a> = hashbrown::hash_map::ValuesMut<'a, K, V> where Self: 'a, K: 'a, V: 'a;

    fn get_mut_with(&mut self, key: &Q) -> Option<&mut Self::Value> {
        self.get_mut(key)
    }

    fn values_mut<'a>(&'a mut self) -> Self::ValuesMut<'a> {
        self.values_mut()
    }
}

impl<K, V, Q> SomeContainerMut<Q> for IndexMap<K, V>
where
    K: Hash + Eq,
    Q: Hash + Equivalent<K>,
{
    type ValuesMut<'a> = indexmap::map::ValuesMut<'a, K, V> where Self: 'a, K: 'a, V: 'a;

    fn get_mut_with(&mut self, key: &Q) -> Option<&mut Self::Value> {
        self.get_mut(key)
    }

    fn values_mut<'a>(&'a mut self) -> Self::ValuesMut<'a> {
        self.values_mut()
    }
}

impl<E: Equivalent<T::Key>, T: SomeContainerMut<E>, const N: usize> SomeContainerMut<E> for [T; N] {
    type ValuesMut<'a> = Flatten<Map<slice::IterMut<'a, T>, fn(&'a mut T) -> T::ValuesMut<'a>>>
    where
        Self: 'a,
        Self::Key: 'a,
        Self::Value: 'a;

    fn get_mut_with(&mut self, key: &E) -> Option<&mut Self::Value> {
        if let Some(container) = self.iter_mut().find(|x| x.contains_key(key)) {
            return container.get_mut_with(key);
        }
        None
    }

    fn values_mut<'a>(&'a mut self) -> Self::ValuesMut<'a> {
        fn values_iter<'a, E1: Equivalent<T1::Key>, T1: SomeContainerMut<E1>>(
            item: &'a mut T1,
        ) -> T1::ValuesMut<'a> {
            item.values_mut()
        }
        self.iter_mut()
            .map(values_iter as fn(&'a mut T) -> T::ValuesMut<'a>)
            .flatten()
    }
}

#[test]
fn test() {
    let mut map1 = collections::HashMap::new();
    let mut map2 = collections::HashMap::new();
    let mut map3 = collections::HashMap::new();
    for i in 0..10 {
        map1.insert(i, 100 + i);
        map2.insert(i + 10, 100 + i + 10);
        map3.insert(i + 20, 100 + i + 20);
    }

    let array = [map1, map2, map3];

    let mut vec = array.keys().cloned().collect::<Vec<_>>();
    vec.sort_unstable();

    for (item, count) in vec.into_iter().zip(0..30) {
        assert_eq!(item, count);
    }

    let mut vec = array.values().cloned().collect::<Vec<_>>();
    vec.sort_unstable();

    for (item, count) in vec.into_iter().zip(100..130) {
        assert_eq!(item, count);
    }
}
