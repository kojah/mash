use assert_cmd::Command;
use predicates::prelude::*;
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

mod support;
use mash_model::test_support;
use support::*;

mod chats;
mod commands;
mod configuration;
mod export;
mod index_destinations;
mod indexing;
mod init;
mod installation;
mod publication;
mod readiness;
mod real_models;
mod redaction;
mod retrieval;
mod shared_server;
mod tools;
mod vector_overrides;
mod watch_flow;
