use serde::{Deserialize, Serialize};
use std::{fmt, num::NonZeroU32};
use thiserror::Error;

/// Explicit version for a serialized schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SchemaVersion(pub u32);

/// A validated stable name used by experiment contracts.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Name(String);

impl Name {
    /// Maximum encoded name length.
    pub const MAX_LEN: usize = 128;
    /// Create a name, rejecting empty, oversized, or whitespace/control-only values.
    ///
    /// # Errors
    /// Returns [`NameError`] when the value is empty, too long, or contains controls.
    pub fn new(value: impl Into<String>) -> Result<Self, NameError> {
        let value = value.into();
        if value.is_empty() || value.trim().is_empty() {
            return Err(NameError::Empty);
        }
        if value.len() > Self::MAX_LEN {
            return Err(NameError::TooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(NameError::ControlCharacter);
        }
        Ok(Self(value))
    }
    /// Borrow the name text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Name {
    type Error = NameError;
    fn try_from(v: String) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}
impl From<Name> for String {
    fn from(v: Name) -> Self {
        v.0
    }
}
impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Name").field(&self.0).finish()
    }
}
impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Name construction failures.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NameError {
    /// Empty or whitespace-only value.
    #[error("name must not be empty")]
    Empty,
    /// Value exceeded the bounded length.
    #[error("name exceeds the maximum length of 128 bytes")]
    TooLong,
    /// Value contains a control character.
    #[error("name contains a control character")]
    ControlCharacter,
}

/// Secret-bearing inputs are represented only by references.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRef {
    /// Reference name, never the secret itself.
    pub reference: Name,
}
impl fmt::Debug for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretRef")
            .field("reference", &self.reference)
            .field("value", &"[REDACTED]")
            .finish()
    }
}
impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED:{}]", self.reference)
    }
}

/// A positive duration in whole milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct DurationMs(u64);
impl DurationMs {
    /// Construct a positive duration bounded to one year.
    ///
    /// # Errors
    /// Returns [`BoundError`] when the value is zero or above one year.
    pub fn new(value: u64) -> Result<Self, BoundError> {
        if value == 0 || value > 31_536_000_000 {
            Err(BoundError("duration must be between 1ms and one year"))
        } else {
            Ok(Self(value))
        }
    }
    /// Duration value in milliseconds.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
impl TryFrom<u64> for DurationMs {
    type Error = BoundError;
    fn try_from(v: u64) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}
impl From<DurationMs> for u64 {
    fn from(v: DurationMs) -> Self {
        v.0
    }
}

/// A positive offered rate represented in requests per second (milli-request precision).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct RateMilliRps(u64);
impl RateMilliRps {
    /// Construct a positive rate up to one million requests/second.
    ///
    /// # Errors
    /// Returns [`BoundError`] when the rate is zero or exceeds the supported bound.
    pub fn new(value: u64) -> Result<Self, BoundError> {
        if value == 0 || value > 1_000_000_000 {
            Err(BoundError(
                "rate must be between 0.001 and 1,000,000 requests/second",
            ))
        } else {
            Ok(Self(value))
        }
    }
    /// Rate in milli-requests per second.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
impl TryFrom<u64> for RateMilliRps {
    type Error = BoundError;
    fn try_from(v: u64) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}
impl From<RateMilliRps> for u64 {
    fn from(v: RateMilliRps) -> Self {
        v.0
    }
}

/// A bounded percentage in basis points (1/100 of one percent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct BasisPoints(u16);
impl BasisPoints {
    /// Construct a percentage from 0 to 10000 basis points.
    ///
    /// # Errors
    /// Returns [`BoundError`] when the percentage exceeds 100 percent.
    pub fn new(value: u16) -> Result<Self, BoundError> {
        if value > 10_000 {
            Err(BoundError("percentage must not exceed 10000 basis points"))
        } else {
            Ok(Self(value))
        }
    }
    /// Basis-point value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}
impl TryFrom<u16> for BasisPoints {
    type Error = BoundError;
    fn try_from(v: u16) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}
impl From<BasisPoints> for u16 {
    fn from(v: BasisPoints) -> Self {
        v.0
    }
}

/// A positive bounded count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct PositiveCount(NonZeroU32);
impl PositiveCount {
    /// Create a positive count up to one million.
    ///
    /// # Errors
    /// Returns [`BoundError`] when the count is zero or exceeds one million.
    pub fn new(value: u32) -> Result<Self, BoundError> {
        if value > 1_000_000 {
            return Err(BoundError("count exceeds one million"));
        }
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(BoundError("count must be positive"))
    }
    /// Count value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}
impl TryFrom<u32> for PositiveCount {
    type Error = BoundError;
    fn try_from(v: u32) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}
impl From<PositiveCount> for u32 {
    fn from(v: PositiveCount) -> Self {
        v.get()
    }
}

/// Numeric bound failure.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("{0}")]
pub struct BoundError(pub &'static str);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secret_reference_debug_and_display_redact_values() {
        let secret = "bearer-super-secret";
        let reference = SecretRef {
            reference: Name::new("TOKEN_ENV").unwrap(),
        };
        assert!(!format!("{reference:?}").contains(secret));
        assert!(!reference.to_string().contains(secret));
        assert!(format!("{reference:?}").contains("REDACTED"));
    }
}
