//! Functions and type definitions for handling errors.
use crate::{io, BinRead, BinResult, ReadOptions};
#[cfg(all(doc, not(feature = "std")))]
extern crate std;
#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, string::String, vec::Vec};
use core::{any::Any, fmt};

/// The `CustomError` trait describes types that are usable as custom errors
/// in a [`BinResult`](crate::BinResult).
///
/// This trait is automatically implemented for any type which implements the
/// same traits as [`std::error::Error`], so anything you would normally use as
/// an error in other code is also a valid `CustomError`, with the additional
/// restriction that it must also be [`Send`] + [`Sync`].
pub trait CustomError: Any + fmt::Display + fmt::Debug + Send + Sync + 'static {
    #[doc(hidden)]
    fn as_any(&self) -> &(dyn Any + Send + Sync);

    #[doc(hidden)]
    fn as_box_any(self: Box<Self>) -> Box<dyn Any + Send + Sync>;
}

impl<T: Any + fmt::Display + fmt::Debug + Send + Sync + 'static> CustomError for T {
    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }

    fn as_box_any(self: Box<Self>) -> Box<dyn Any + Send + Sync> {
        self
    }
}

impl dyn CustomError {
    /// Attempts to downcast a boxed error to a concrete type.
    pub fn downcast<T: Any>(self: Box<Self>) -> Result<Box<T>, Box<Self>> {
        if self.is::<T>() {
            unsafe {
                let raw: *mut dyn Any = Box::into_raw(self.as_box_any());
                Ok(Box::from_raw(raw as *mut T))
            }
        } else {
            Err(self)
        }
    }

    /// Returns some reference to the boxed value if it is of type `T`, or
    /// `None` if it isn’t.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.as_any().downcast_ref()
    }

    /// Returns `true` if the boxed type is the same as `T`.
    pub fn is<T: Any>(&self) -> bool {
        core::any::TypeId::of::<T>() == self.type_id()
    }
}

/// The error type used by [`BinRead`](crate::BinRead).
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    /// An expected [magic number](crate::attribute#magic) was not found.
    BadMagic {
        /// The byte position of the unexpected magic in the reader.
        pos: u64,

        /// The value which was actually read.
        found: Box<dyn fmt::Debug + Send + Sync>,
    },

    /// An assertion failed.
    ///
    /// This variant is used for [`assert`] directives which use a string
    /// literal instead of an error object. Assertions that use error objects
    /// are represented by the [`Custom`] variant.
    ///
    /// [`assert`]: crate::attribute#assert
    /// [`Custom`]: Self::Custom
    AssertFail {
        /// The byte position of the start of the field or object that raised
        /// an error.
        pos: u64,

        /// The failure message.
        message: String,
    },

    /// An error occurred in the underlying reader while reading or seeking to
    /// data.
    Io(io::Error),

    /// A user-generated error.
    ///
    /// This variant is used for [`assert`] directives which use an error object
    /// instead of a string literal. Assertions that use string literals are
    /// represented by the [`AssertFail`] variant.
    ///
    /// [`assert`]: crate::attribute#assert
    /// [`AssertFail`]: Self::AssertFail
    Custom {
        /// The byte position of the start of the field or object that raised
        /// an error.
        pos: u64,

        /// The original error.
        err: Box<dyn CustomError>,
    },

    /// None of the variants of an enum could successfully be parsed from the
    /// data in the reader.
    ///
    /// This variant is used when the [`return_unexpected_error`] directive is
    /// set on an enum.
    ///
    /// [`return_unexpected_error`]: crate::attribute#enum-errors
    NoVariantMatch {
        /// The byte position of the unparsable data in the reader.
        pos: u64,
    },

    /// None of the variants of an enum could successfully be parsed from the
    /// data in the reader.
    ///
    /// This variant is used when the [`return_all_errors`] directive is
    /// set on an enum (which is the default).
    ///
    /// [`return_all_errors`]: crate::attribute#enum-errors
    EnumErrors {
        /// The byte position of the unparsable data in the reader.
        pos: u64,

        /// The original errors which occurred when trying to parse each
        /// variant.
        ///
        /// The first field of the tuple is the name of the variant, and the
        /// second field is the error that occurred when parsing that variant.
        variant_errors: Vec<(&'static str, Error)>,
    },
}

impl Error {
    /// Returns a reference to the boxed error object if this `Error` is a
    /// custom error of type `T`, or `None` if it isn’t.
    pub fn custom_err<T: Any>(&self) -> Option<&T> {
        if let Error::Custom { err, .. } = self {
            err.downcast_ref()
        } else {
            None
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadMagic { pos, found } => write!(f, "bad magic at 0x{:x}: {:?}", pos, found),
            Error::AssertFail { pos, message } => write!(f, "{} at 0x{:x}", message, pos),
            Error::Io(err) => fmt::Display::fmt(err, f),
            Error::Custom { pos, err } => write!(f, "{} at 0x{:x}", err, pos),
            Error::NoVariantMatch { pos } => write!(f, "no variants matched at 0x{:x}", pos),
            Error::EnumErrors {
                pos,
                variant_errors,
            } => {
                write!(f, "no variants matched at 0x{:x}:", pos)?;
                for (name, err) in variant_errors {
                    write!(f, "\n  {}: {}", name, err)?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// Read a value then check if it is the expected value
pub fn magic<R, B>(reader: &mut R, expected: B, options: &ReadOptions) -> BinResult<()>
where
    B: BinRead<Args = ()> + fmt::Debug + PartialEq + Sync + Send + 'static,
    R: io::Read + io::Seek,
{
    let pos = reader.seek(io::SeekFrom::Current(0))?;
    let val = B::read_options(reader, &options, ())?;
    if val == expected {
        Ok(())
    } else {
        Err(Error::BadMagic {
            pos,
            found: Box::new(val) as _,
        })
    }
}

/// Reads a value, then immediately finalizes it by running
/// [`after_parse()`](crate::BinRead::after_parse).
pub fn read_options_then_after_parse<Args, T, R>(
    reader: &mut R,
    ro: &ReadOptions,
    args: T::Args,
) -> BinResult<T>
where
    Args: Copy + 'static,
    T: BinRead<Args = Args>,
    R: io::Read + io::Seek,
{
    let mut val = T::read_options(reader, ro, args)?;
    val.after_parse(reader, ro, args)?;
    Ok(val)
}
