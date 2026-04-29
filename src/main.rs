#![feature(error_generic_member_access)]

pub mod cli;
pub mod core;
pub mod error;
pub mod lua;
pub mod template;

fn main() -> color_eyre::Result<()> {
    // Set up nicer errors for users
    color_eyre::install()?;
    cli::run(std::env::args()).map_err(|err| color_eyre::eyre::eyre!("{}", err))?;
    Ok(())
}
