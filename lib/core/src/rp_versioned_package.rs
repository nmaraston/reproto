//! A versioned package declaration

use super::{RpPackage, RpPackageFormat, Version};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Serialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RpVersionedPackage {
    pub package: RpPackage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<Version>,
}

impl RpVersionedPackage {
    pub fn new(package: RpPackage, version: Option<Version>) -> RpVersionedPackage {
        RpVersionedPackage {
            package: package,
            version: version,
        }
    }

    /// Convert into a package by piping the version through the provided function.
    pub fn as_package<F>(&self, version_fn: F) -> RpPackage
    where
        F: FnOnce(&Version) -> String,
    {
        let mut parts = Vec::new();

        parts.extend(self.package.parts.iter().cloned());

        if let Some(ref version) = self.version {
            parts.push(version_fn(version));
        }

        RpPackage::new(parts)
    }

    pub fn without_version(self) -> RpVersionedPackage {
        RpVersionedPackage::new(self.package, None)
    }

    /// Replace all keyword components in this package.
    pub fn with_replacements(self, keywords: &HashMap<String, String>) -> Self {
        Self {
            package: self.package.with_replacements(keywords),
            ..self
        }
    }

    /// Apply the given naming policy to this package.
    pub fn with_naming<N>(self, naming: N) -> Self
    where
        N: Fn(&str) -> String,
    {
        Self {
            package: self.package.with_naming(naming),
            ..self
        }
    }
}

impl fmt::Display for RpVersionedPackage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        RpPackageFormat(&self.package, self.version.as_ref()).fmt(f)
    }
}
