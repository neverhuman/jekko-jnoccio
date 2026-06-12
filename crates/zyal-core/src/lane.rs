//! Id newtypes used across crates to identify reasoning lanes, runs, and
//! artifacts. Crate-local rich `Lane` / `Run` / `Artifact` structs keep their
//! existing shapes; these newtypes are the *handles* that cross crate
//! boundaries.

use serde::{Deserialize, Serialize};

macro_rules! id_newtype {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_newtype!(
    LaneId,
    "Identifier for a single reasoning lane (one strategy / one role)."
);
id_newtype!(
    RunId,
    "Identifier for a single ZYAL run (one orchestrated reasoning cycle)."
);
id_newtype!(ArtifactRef, "Reference to a reasoning artifact by id.");
