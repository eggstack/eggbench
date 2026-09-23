//! External command-driver substrate: trusted resolution, bounded argv
//! execution, version probing, raw artifacts, and a versioned parser contract.

mod artifact;
mod command;
mod error;
mod parser;
mod resolver;
mod version;

pub use artifact::{artifact_candidates, command_metadata_json, workload_output_from_outcome};
pub use command::{CapturedStream, ExternalCommandOutcome, ExternalCommandSpec, run_command};
pub use error::{DriverError, ErrorCategory};
pub use parser::{
    ExternalOutputParser, ExternalParseError, FixtureParsedOutput, FixtureParser,
    ParsedExternalOutput,
};
pub use resolver::{BinaryResolver, ResolvedExecutable};
pub use version::{ToolVersion, VersionProbe, VersionProbeSpec};
