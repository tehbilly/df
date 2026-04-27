#![feature(error_generic_member_access)]

pub mod cli;
pub mod core;
pub mod error;
pub mod lua;
pub mod template;

fn main() -> core::Result<()> {
    cli::run(std::env::args())
}
