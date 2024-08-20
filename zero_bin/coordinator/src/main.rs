//! This file provides a means of setting up a web-server to handle multi-block
//! proofs
use std::{
    env,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use anyhow::Result;
use coordinator::manyprover::ManyProver;
pub use coordinator::{
    benchmarking, fetch,
    input::{self, ProveBlocksInput},
    manyprover, proofout, psm,
    cfgld::build_paladin_config_from_env,
};
use dotenvy::dotenv;
use ops::register;
use paladin::{
    config::{Config, Serializer},
    runtime::Runtime,
};
// use leader::init;
use tracing::{debug, error, info, warn};
use zero_bin_common::prover_state;

pub const SERVER_ADDR_ENVKEY: &str = "SERVER_ADDR";
pub const DFLT_SERVER_ADDR: &str = "0.0.0.0:8080";
pub const NUM_SERVER_WORKERS: usize = 4;

#[tokio::main]
async fn main() -> Result<()> {
    //========================================================================
    // Setup
    //========================================================================

    // Load in the environment
    debug!("Loading dotenv");
    leader::init::load_dotenvy_vars_if_present();
    leader::init::tracing();

    if env::var_os(leader::init::EVM_ARITH_VER_KEY).is_none() {
        // Safety:
        // - we're early enough in main that nothing else should race
        unsafe {
            env::set_var(
                leader::init::EVM_ARITH_VER_KEY,
                // see build.rs
                env!("EVM_ARITHMETIZATION_PACKAGE_VERSION"),
            );
        }
    };

    //------------------------------------------------------------------------
    // Request queue
    //------------------------------------------------------------------------

    info!("Initializing the request queue");
    let (mut tx, mut rx) = tokio::sync::mpsc::channel::<ProveBlocksInput>(50);

    // Store it in a Data for server
    let post_queue = web::Data::new(tx);

    //------------------------------------------------------------------------
    // Runtime
    //------------------------------------------------------------------------

    info!("Starting to build Paladin Runtime");

    let runtime = {
        info!("Attempting to build paladin config for Runtime");
        let config = build_paladin_config_from_env();

        debug!("Determining if should initialize a prover state config...");
        match &config.runtime {
            paladin::config::Runtime::InMemory => {
                info!("InMemory runtime, initializing a prover_state_manager");
                let psm = psm::load_psm_from_env();
                info!("Attempting to initialize the Prover State Manager.");

                match psm.initialize() {
                    Ok(_) => {
                        info!("Initialized the ProverStateManager");
                    }
                    Err(err) => {
                        error!("Failed to initialize the ProverStateManager: {}", err);
                        panic!("Failed to initialize the ProverStateManager: {}", err);
                    }
                }
            }
            paladin_runtime => {
                info!(
                    "Not initializing prover_state_manager due to Paladin Runtime: {:?}",
                    paladin_runtime
                );
            }
        }

        info!("Building Paladin Runtime");
        match Runtime::from_config(&config, register()).await {
            Ok(runtime) => {
                info!("Created Paladin Runtime");
                runtime
            }
            Err(err) => {
                error!("Config: {:#?}", config);
                error!("Error while constructing the runtime: {}", err);
                panic!("Failed to build Paladin runtime from config: {}", err);
            }
        }
    };

    debug!("Wrapping Paladin Runtime in Arc");
    let runtime_arc = Arc::new(runtime);

    //------------------------------------------------------------------------
    // Server
    //------------------------------------------------------------------------

    debug!("Setting up server endpoint");

    let server_addr = match env::var(SERVER_ADDR_ENVKEY) {
        Ok(addr) => {
            info!("Retrieved server address: {}", addr);
            addr
        }
        Err(env::VarError::NotPresent) => {
            warn!("Using default server address: {}", DFLT_SERVER_ADDR);
            String::from(DFLT_SERVER_ADDR)
        }
        Err(env::VarError::NotUnicode(os_str)) => {
            error!("Non-unicode server address: {:?}", os_str);
            panic!("Non-unicode server address: {:?}", os_str);
        }
    };

    // Set up the server
    let server = match HttpServer::new(move || {
        App::new()
            .app_data(post_queue.clone())
            .service(web::resource("/").route(web::post().to(handle_post)))
            .route("/health", web::get().to(handle_health))
    })
    .workers(NUM_SERVER_WORKERS)
    .bind(server_addr.as_str())
    {
        Ok(item) => item,
        Err(err) => panic!("Failed to start the server: {}", err),
    };

    // Move the http server to its own tokio thread
    info!("Starting HTTP Server: {}", server_addr);
    tokio::task::spawn(server.run());

    // Start the processing loop
    info!("Starting the processing loop.");
    let mut run_cnt: usize = 0;
    loop {
        run_cnt += 1;
        info!("Awaiting request for run {} in current session.", run_cnt);
        match rx.recv().await {
            Some(input) => {
                info!("Received request for run #{} in current session", run_cnt);
                info!("From queue: {:?}", input);
                match ManyProver::new(input, runtime_arc.clone()).await {
                    Ok(mut manyprover) => {
                        match manyprover.prove_blocks().await {
                            Ok(_) => info!("Completed a request."),
                            Err(err) => error!("Critical error: {}", err),
                        };
                    }
                    Err(err) => error!("Critical configuration error: {}", err),
                }
            }
            None => {
                info!("Channel to process posts is closed.");
                // Attempt to close the runtime proper.
                match runtime_arc.close().await {
                    Ok(_) => info!("Successfully terminated the runtime."),
                    Err(err) => error!("Error closing the runtime: {}", err),
                }
                break;
            }
        }
    }

    info!("Closing Coordinator");
    Ok(())
}

/// Returns [HttpResponse] ([HttpResponse::Ok]) to respond that we are healthy
async fn handle_health() -> impl Responder {
    debug!("Received health check, responding `OK`");
    HttpResponse::Ok().body("OK")
}

/// Recevies a request for [manyprover::ManyProver::prove_blocks]
async fn handle_post(
    wdtx: web::Data<tokio::sync::mpsc::Sender<ProveBlocksInput>>,
    input: web::Json<ProveBlocksInput>,
) -> impl Responder {
    let start_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(err) => {
            panic!("Unable to determine current time: {}", err);
        }
    };
    info!("Received request to prove blocks Request {}", start_time);

    match wdtx.send(input.0).await {
        Ok(_) => info!("Successfully queued Request {}", start_time),
        Err(err) => {
            error!(
                "Critical error while trying to queue Request {}: {}",
                start_time, err
            );
            return HttpResponse::InternalServerError();
        }
    }

    // Respond the Accepted response
    HttpResponse::Accepted()
}


