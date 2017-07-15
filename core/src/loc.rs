use object::Object;
use std::cmp;
use std::hash;
use std::sync::{Arc, Mutex};
use super::errors::*;
use super::merge::Merge;

pub type Pos = (Arc<Mutex<Box<Object>>>, usize, usize);

#[derive(Clone, Serialize)]
pub struct Loc<T> {
    #[serde(rename = "value")]
    inner: T,
    #[serde(skip_serializing)]
    pos: Pos,
}

impl<T> cmp::PartialEq for Loc<T>
    where T: cmp::PartialEq
{
    fn eq(&self, other: &Self) -> bool {
        self.inner.eq(other)
    }
}

impl<T> cmp::Eq for Loc<T> where T: cmp::Eq {}

impl<T> cmp::PartialOrd for Loc<T>
    where T: cmp::PartialOrd
{
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        self.inner.partial_cmp(other)
    }
}

impl<T> cmp::Ord for Loc<T>
    where T: cmp::Ord,
          Self: cmp::PartialOrd + cmp::Eq
{
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.inner.cmp(other)
    }
}

impl<T> hash::Hash for Loc<T>
    where T: hash::Hash
{
    fn hash<H>(&self, state: &mut H)
        where H: hash::Hasher
    {
        self.inner.hash(state)
    }
}

impl<T> Merge for Loc<T>
    where T: Merge
{
    fn merge(&mut self, source: Loc<T>) -> Result<()> {
        self.as_mut().merge(source.move_inner())?;
        Ok(())
    }
}

impl<T> ::std::ops::Deref for Loc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T> ::std::borrow::Borrow<T> for Loc<T> {
    fn borrow(&self) -> &T {
        &self.inner
    }
}

impl<T> ::std::convert::AsRef<T> for Loc<T> {
    fn as_ref(&self) -> &T {
        &self.inner
    }
}

impl<T> ::std::convert::AsMut<T> for Loc<T> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T> Loc<T> {
    pub fn new(inner: T, pos: Pos) -> Loc<T> {
        Loc {
            inner: inner,
            pos: pos,
        }
    }

    pub fn move_inner(self) -> T {
        self.inner
    }

    pub fn pos(&self) -> &Pos {
        &self.pos
    }

    pub fn map_into<M, U>(self, map: M) -> Loc<U>
        where M: FnOnce(T) -> U
    {
        Loc::new(map(self.inner), self.pos)
    }

    pub fn map<'a, M, U>(&'a self, map: M) -> Loc<U>
        where M: FnOnce(&'a T) -> U,
              U: 'a
    {
        Loc::new(map(&self.inner), self.pos.clone())
    }

    pub fn both(self) -> (T, Pos) {
        (self.inner, self.pos)
    }

    pub fn ref_both(&self) -> (&T, &Pos) {
        (&self.inner, &self.pos)
    }
}

impl<T> ::std::fmt::Display for Loc<T>
    where T: ::std::fmt::Display
{
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl<T> ::std::fmt::Debug for Loc<T>
    where T: ::std::fmt::Debug
{
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(f, "<{:?}@{:?}>", self.inner, self.pos)
    }
}
