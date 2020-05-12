use rustysd::units::ServiceConfig;
use serde_json;

/// This binary will be able to perform most tasks systemd does for executing binaries.
/// It takes a json description of a ServiceConfig and applies it to itself.
/// It then execs the specified command of that config.
///
/// This should keep linux specific stuff behind feature flags, because rustysd depends on it, and rustysd should stay
/// as platform agnostic as possible.

pub fn main() {
    let args = std::env::args().collect::<Vec<_>>();

    let mut cmd = "".to_owned();
    let mut cmd_idx = None;
    let mut json = "".to_owned();
    let mut idx = 1;
    while idx < args.len() {
        if args[idx].as_str() == "--command" {
            cmd = args[idx + 1].clone();
            idx += 2;
        }
        if args[idx].as_str() == "--cmd_idx" {
            cmd_idx = Some(args[idx + 1].parse::<usize>().unwrap());
            idx += 2;
        }
        if args[idx].as_str() == "--conf" {
            json = args[idx + 1].clone();
            idx += 2;
        }
    }

    let conf: ServiceConfig = ServiceConfig::import_json(&serde_json::from_str(&json).unwrap());

    // TODO general setup like env and stuff

    match cmd.as_str() {
        "start" => {
            // TODO exec
            let cmd = &conf.exec;
        }
        "stop" => {
            let cmd = &conf.stop[cmd_idx.unwrap()];
            // TODO exec
        }
        "stoppost" => {
            let cmd = &conf.stoppost[cmd_idx.unwrap()];
            // TODO exec
        }
        "startpre" => {
            let cmd = &conf.startpre[cmd_idx.unwrap()];
            // TODO exec
        }
        "startpost" => {
            let cmd = &conf.startpost[cmd_idx.unwrap()];
            // TODO exec
        }
        _ => panic!("Unknown command: {}", cmd),
    }
}
