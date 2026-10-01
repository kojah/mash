use assert_cmd::Command;
use predicates::prelude::*;
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

mod support;
#[path = "../test_support.rs"]
mod test_support;
use support::*;

mod commands;
mod configuration;
mod index_destinations;
mod indexing;
mod publication;
mod readiness;
mod real_models;
mod retrieval;
mod shared_server;
mod threads;
mod watch_flow;
