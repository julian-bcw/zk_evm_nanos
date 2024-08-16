use paladin::{
    config::{Config, Serializer},
    runtime::Runtime,
};

use std::env;
use tracing::log::{info, warn, error};

//=======================================================================================================
// Paladin Runtime Construction
//=======================================================================================================

/// The environment key for the Paladin Serializer, used to construct
/// [paladin::config::Serializer]
pub const PALADIN_SERIALIZER_ENVKEY: &str = "PALADIN_SERIALIZER";
/// The environment key for the Paladin Runtime, used to construct
/// [paladin::config::Runtime]
pub const PALADIN_RUNTIME_ENVKEY: &str = "PALADIN_RUNTIME";
/// The environment key for the number of workers (only matters in operating in
/// memory)
pub const PALADIN_AMQP_NUM_WORKERS_ENVKEY: &str = "PALADIN_AMQP_NUM_WORKERS";
/// The environment key for the amqp uri (only matters in operating w/ AMQP)
pub const PALADIN_AMQP_URI_ENVKEY: &str = "PALADIN_AMQP_URI";
/// The default number of workers to be used when operating in memory if not
/// specified in the environment
pub const DFLT_NUM_WORKERS: usize = 1;

/// Constructs the [Config] given environment variables.
pub fn build_paladin_config_from_env() -> Config {
    let serializer = match env::var(PALADIN_SERIALIZER_ENVKEY) {
        Ok(serializer) if serializer.contains("POSTCARD") => Serializer::Postcard,
        Err(env::VarError::NotPresent) => {
            info!("Paladin Serializer not specified, using Default");
            Serializer::default()
        }
        Ok(serializer) if serializer.contains("CBOR") => Serializer::Cbor,
        Ok(unknown_serializer) => {
            panic!("Unsure what Paladin Serializer: {}", unknown_serializer);
        }
        Err(env::VarError::NotUnicode(os_str)) => {
            panic!("Non-Unicode input for Paladin Serializer: {:?}", os_str);
        }
    };

    let runtime = match env::var(PALADIN_RUNTIME_ENVKEY) {
        Ok(paladin_runtime) if paladin_runtime.contains("AMQP") => paladin::config::Runtime::Amqp,
        Ok(paladin_runtime) if paladin_runtime.contains("MEMORY") => {
            paladin::config::Runtime::InMemory
        }
        Ok(unknown_runtime) => {
            panic!("Unsure what Paladin Runtime: {}", unknown_runtime);
        }
        Err(env::VarError::NotPresent) => {
            info!("Paladin Runtime not specified, using default");
            paladin::config::Runtime::InMemory
        }
        Err(env::VarError::NotUnicode(os_str)) => {
            panic!("Non-Unicode input for Paladin Runtime: {:?}", os_str);
        }
    };

    let num_workers = match (runtime, env::var(PALADIN_AMQP_NUM_WORKERS_ENVKEY)) {
        (paladin::config::Runtime::InMemory, Ok(num_workers)) => {
            match num_workers.parse::<usize>() {
                Ok(num_workers) => Some(num_workers),
                Err(err) => {
                    error!("Failed to parse number of workers: {}", err);
                    panic!("Failed to parse number of workers: {}", err);
                }
            }
        }
        (paladin::config::Runtime::InMemory, Err(env::VarError::NotPresent)) => {
            info!(
                "Number of workers not specified for InMemory runtime, using default: {}",
                DFLT_NUM_WORKERS
            );
            None //Some(DFLT_NUM_WORKERS)
        }
        (paladin::config::Runtime::InMemory, Err(env::VarError::NotUnicode(os_str))) => {
            info!("Non-Unicode input for number of workers: {:?}", os_str);
            panic!("Non-Unicode input for number of workers: {:?}", os_str);
        }
        (_, Ok(num_workers)) => {
            info!(
                "Not operating in memory, disregarding number of workers from env ({})",
                num_workers
            );
            None
        }
        (_, _) => None,
    };

    let amqp_uri = match (runtime, env::var(PALADIN_AMQP_URI_ENVKEY)) {
        (paladin::config::Runtime::Amqp, Ok(uri)) => Some(uri),
        (paladin::config::Runtime::Amqp, Err(env::VarError::NotPresent)) => {
            panic!("If AMQP runtime, must specify amqp uri in environment");
        }
        (paladin::config::Runtime::Amqp, Err(env::VarError::NotUnicode(os_str))) => {
            panic!("Non-Unicode input for amqp uri string: {:?}", os_str);
        }
        (_, Ok(uri)) => {
            info!(
                "Ignoring AMQP Uri string since we are operating InMemory ({})",
                uri
            );
            None
        }
        (_, _) => None,
    };

    // Construct the Config
    Config {
        serializer,
        runtime,
        num_workers,
        amqp_uri,
    }
}
