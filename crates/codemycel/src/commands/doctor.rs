use super::{ExitCode, not_implemented};
use crate::cli::{Context, DoctorArgs};

pub fn run(_args: &DoctorArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    not_implemented("doctor")
}
