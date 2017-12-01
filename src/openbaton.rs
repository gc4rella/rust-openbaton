extern crate serde;

use serde::de::DeserializeOwned;
use serde::ser::Serialize;
use std::marker::Sync;
use std::marker::Send;

pub trait VimDriver: Sized + Sync + Clone + Send {
    type T: DeserializeOwned + Serialize;
    fn refresh(&self, vim_instance: Self::T) -> Self::T;
}