//! Generated host-side bindings for Brain's four component contracts.

mod generated;
mod runtime;

pub use generated::*;
pub use runtime::*;

pub mod agentloop {
    wasmtime::component::bindgen!({
        path: "../../contracts/agentloop/v1",
        world: "agentloop",
        imports: { default: async },
        exports: { default: async },
    });
}

pub mod tool {
    wasmtime::component::bindgen!({
        path: "../../contracts/tool/v1",
        world: "tool",
        imports: { default: async },
        exports: { default: async },
    });
}

pub mod environment {
    wasmtime::component::bindgen!({
        path: "../../contracts/environment/v1",
        world: "environment",
        imports: { default: async },
        exports: { default: async },
    });
}

pub mod model {
    wasmtime::component::bindgen!({
        path: "../../contracts/model/v1",
        world: "model",
        imports: { default: async },
        exports: { default: async },
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_identities_are_versioned_and_distinct() {
        let worlds = [AGENTLOOP_WORLD, TOOL_WORLD, ENVIRONMENT_WORLD, MODEL_WORLD];
        assert!(worlds.iter().all(|world| world.ends_with("@1.0.0")));
        for (index, world) in worlds.iter().enumerate() {
            assert!(!worlds[..index].contains(world));
        }
    }
}
