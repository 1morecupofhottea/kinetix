//! Kinetix: a multi-protocol LLM proxy. OpenAI Chat Completions and Anthropic
//! Messages in (streaming first), admin-configured upstreams out (Gemini,
//! OpenAI-compatible, Anthropic), with virtual keys, account pools with
//! automatic fallback routes, and cost tracking.
//!
//! This binary is a thin CLI wrapper around the `kinetix` library crate (see
//! `src/lib.rs`), which holds all modules so that integration tests can drive
//! the wire encoders/decoders directly. Run `kinetix --help` for the command
//! surface; `kinetix serve` starts the proxy.

use anyhow::Result;
use clap::Parser;

// Optional allocation accounting (NFR-1.8). Enabled with `--features alloc-stats`.
#[cfg(feature = "alloc-stats")]
#[global_allocator]
static GLOBAL_ALLOC: kinetix::alloc::CountingAllocator = kinetix::alloc::CountingAllocator;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = kinetix::cli::Cli::parse();
    kinetix::cli::run(cli).await
}
