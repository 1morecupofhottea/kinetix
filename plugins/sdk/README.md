# Kinetix Plugin SDK (Rust)

Author-facing bindings for the Kinetix plugin ABI (`docs/KINETIX-PLUGIN-ARCHITECTURE.md`).

## Build a plugin component

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
wasm-tools component new \
  target/wasm32-unknown-unknown/release/<your_plugin>.wasm \
  -o plugin.wasm
```

Package the result as a `.kxp` archive (tar with `plugin.toml`, `plugin.wasm`,
`README.md`, `LICENSE`) and install with `kinetix plugin install plugin.kxp`.

## Minimal plugin

```rust
use kinetix_plugin_sdk::{export, exports, kinetix};
use kinetix::plugin::types::*;

struct Component;

impl exports::model_source::Guest for Component {
    fn discover(provider_id: String, base_url: String, models_path: String)
        -> Result<Vec<DiscoveredModel>, PluginError>
    {
        // ... use kinetix::plugin::host_http::send for control-plane calls ...
        Ok(vec![])
    }
}

// Every export interface must be implemented; return an error for the ones you
// do not provide (the manifest's [provides] is what the host actually routes on).
export!(Component with_types_in kinetix_plugin_sdk);
```

The SDK's `helpers` module wraps the common host calls (`kv_get_string`,
`cache_fact`, `error`, ...). The ABI is the WIT interface in `wit/`, not this
crate.
