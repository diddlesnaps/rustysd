use rustysd::units::Commandline;
use rustysd::units::ServiceConfig;
use serde_json;

/// This binary will be able to perform most tasks systemd does for executing binaries.
/// It takes a json description of a ServiceConfig and applies it to itself.
/// It then execs the specified command of that config.
///
/// This should keep linux specific stuff behind feature flags, because rustysd depends on it, and rustysd should stay
/// as platform agnostic as possible.

fn prepare_exec_args(cmd_line: &Commandline) -> (std::ffi::CString, Vec<std::ffi::CString>) {
    let cmd = std::ffi::CString::new(cmd_line.cmd.as_str()).unwrap();

    let exec_name = std::path::PathBuf::from(&cmd_line.cmd);
    let exec_name = exec_name.file_name().unwrap();
    let exec_name: Vec<u8> = exec_name.to_str().unwrap().bytes().collect();
    let exec_name = std::ffi::CString::new(exec_name).unwrap();

    let mut args = Vec::new();
    args.push(exec_name);

    for word in &cmd_line.args {
        args.push(std::ffi::CString::new(word.as_str()).unwrap());
    }

    (cmd, args)
}

pub fn main() {
    let args = std::env::args().collect::<Vec<_>>();

    let mut cmd = "".to_owned();
    let mut cmd_idx = None;
    let mut srvc_conf_json = "".to_owned();
    let mut env_json = "".to_owned();
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
            srvc_conf_json = args[idx + 1].clone();
            idx += 2;
        }
        if args[idx].as_str() == "--env" {
            env_json = args[idx + 1].clone();
            idx += 2;
        }
    }
    let env: serde_json::Value = serde_json::from_str(&env_json).unwrap();

    let conf: ServiceConfig =
        ServiceConfig::import_json(&serde_json::from_str(&srvc_conf_json).unwrap());

    if nix::unistd::getuid().is_root() {
        match rustysd::platform::drop_privileges(
            conf.exec_config.group,
            &conf.exec_config.supplementary_groups,
            conf.exec_config.user,
        ) {
            Ok(()) => { /* Happy */ }
            Err(e) => {
                eprintln!("Could not drop privileges because: {}", e);
                std::process::exit(1);
            }
        }
    }

    for var in env.as_array().unwrap() {
        let name = var["name"].as_str().unwrap();
        let value = var["value"].as_str().unwrap();
        std::env::set_var(name, value);
    }

    // TODO general setup like env and stuff

    let cmd_line = match cmd.as_str() {
        "start" => {
            // TODO exec
            &conf.exec
        }
        "stop" => {
            &conf.stop[cmd_idx.unwrap()]
            // TODO exec
        }
        "stoppost" => {
            &conf.stoppost[cmd_idx.unwrap()]
            // TODO exec
        }
        "startpre" => {
            &conf.startpre[cmd_idx.unwrap()]
            // TODO exec
        }
        "startpost" => {
            &conf.startpost[cmd_idx.unwrap()]
            // TODO exec
        }
        _ => panic!("Unknown command: {}", cmd),
    };
    let (cmd, args) = prepare_exec_args(cmd_line);
    let args_brrw = args.iter().map(|arg| arg.as_c_str()).collect::<Vec<_>>();
    nix::unistd::execvp(&cmd, &args_brrw).unwrap();
}
