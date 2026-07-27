//! `gallo-mcp` — Model Context Protocol server for Pico de Gallo.

// Helpers land before their consumers during scaffolding; consumers arrive in a
// later task. Remove this allow once the tool modules use these items.
#![allow(dead_code)]

mod encoding;

fn main() {
    eprintln!("gallo-mcp scaffold");
}
