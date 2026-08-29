//! How an enum value is spelled on a command line.

use std::ffi::{OsStr, OsString};
use std::marker::PhantomData;

use clap::builder::{EnumValueParser, PossibleValue, TypedValueParser};
use clap::{Arg, Command, Error, ValueEnum};

/// A [`ValueEnum`] parser that takes both spellings of a value: the kebab-case
/// one CLIs are written in and the help prints (`in-progress`), and the
/// snake_case one the daemon, the API and `--format json` use
/// (`in_progress`) — which is what the same value comes back as, so a status
/// copied out of a listing is a status that can be typed back in.
///
/// Values with a single word are one spelling and reach this unchanged.
pub struct Spelling<T>(PhantomData<T>);

impl<T> Spelling<T> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

// Derived on the enum, `Clone` would ask `T: Clone` for no reason: the parser
// holds nothing but the type.
impl<T> Clone for Spelling<T> {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<T> Default for Spelling<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ValueEnum + Clone + Send + Sync + 'static> TypedValueParser for Spelling<T> {
    type Value = T;

    fn parse_ref(&self, cmd: &Command, arg: Option<&Arg>, value: &OsStr) -> Result<T, Error> {
        let parser = EnumValueParser::<T>::new();
        parser.parse_ref(cmd, arg, value).or_else(|refusal| {
            // The snake_case spelling is a second reading of the same value,
            // never a second chance at a wrong one: what comes back from a
            // failure is clap's own refusal of what was typed, which quotes it
            // and lists the values that exist.
            let Some(text) = value.to_str() else {
                return Err(refusal);
            };
            let kebab = OsString::from(text.replace('_', "-"));
            parser.parse_ref(cmd, arg, &kebab).map_err(|_| refusal)
        })
    }

    /// The kebab-case spellings, which is what the help and the completions
    /// offer: one value has one canonical way of being written down.
    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(
            T::value_variants()
                .iter()
                .filter_map(ValueEnum::to_possible_value),
        ))
    }
}
